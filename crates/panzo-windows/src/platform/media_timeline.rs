//! Typed timeline rows share one viewport; gestures commit once on release.
use super::*;
use panzo_core::{
    media_edit::TimeAnchor,
    timeline::{Track, TrackKind},
};
use windows::Win32::UI::WindowsAndMessaging::{IDOK, MB_ICONQUESTION, MB_OKCANCEL, MessageBoxW};
pub(super) struct RecordingPreview {
    pub start: TimeTick,
    pub frames: u64,
    pub peaks: Vec<f32>,
}
#[derive(Clone)]
pub(super) struct MediaDrag {
    subtitle: bool,
    visual: bool,
    follow: bool,
    legacy: bool,
    kind: TrackKind,
    origin_track: String,
    target_track: Option<String>,
    view_start: TimeTick,
    id: String,
    start_x: i32,
    pub(super) current_x: i32,
    pub(super) current_y: i32,
    mode: TimelineDragMode,
    original: panzo_core::WorkbenchSettings,
    range: (TimeTick, TimeTick),
}
impl WindowState {
    pub(super) fn row_track(&self, row: usize) -> Option<&Track> {
        if row < 3 {
            let id = ["narration", "music", "subtitle"][row];
            self.editor.settings().tracks.iter().find(|t| t.id == id)
        } else {
            self.editor
                .settings()
                .tracks
                .iter()
                .filter(|t| {
                    !["video", "follow", "narration", "music", "subtitle"].contains(&t.id.as_str())
                })
                .nth(row - 3)
        }
    }
    pub(super) fn media_layout(&self, c: ControlLayout) -> ControlLayout {
        let mut c = c.with_tracks(true, true, 0, self.theme);
        let area = c.track_area;
        let gap = self.theme.scale(5);
        let rh = c.media_tracks[0].bottom - c.media_tracks[0].top;
        let vh = c.video_track.bottom - c.video_track.top;
        let base = self
            .editor
            .settings()
            .tracks
            .iter()
            .any(|t| t.id == "video");
        let camera = self
            .editor
            .settings()
            .tracks
            .iter()
            .any(|t| t.id == "follow");
        let count = (0..35).filter(|i| self.row_track(*i).is_some()).count() as i32;
        let total = if base { vh + gap } else { 0 }
            + if camera { rh + gap } else { 0 }
            + count * (rh + gap);
        c.track_scroll_max = (total - (area.bottom - area.top)).max(0);
        let mut top = area.top - self.track_scroll.clamp(0, c.track_scroll_max);
        let mut row = |height: i32| {
            let r = HitRect(RECT {
                left: area.left,
                right: area.right,
                top,
                bottom: top + height,
            });
            if height > 0 {
                top += height + gap;
            }
            r
        };
        c.video_track = row(if base { vh } else { 0 });
        c.click_track = HitRect(RECT {
            top: c.video_track.bottom - 2,
            bottom: c.video_track.bottom,
            ..c.video_track.0
        });
        c.camera_track = row(if camera { rh } else { 0 });
        for (i, r) in c.media_tracks.iter_mut().enumerate() {
            *r = row(if self.row_track(i).is_some() { rh } else { 0 });
        }
        c
    }
    fn media_rect(&self, track: HitRect, a: TimeTick, b: TimeTick) -> RECT {
        RECT {
            left: timeline_x(track, a, self.timeline).max(track.left),
            right: timeline_x(track, b, self.timeline).min(track.right),
            top: track.top + self.theme.scale(3),
            bottom: track.bottom - self.theme.scale(3),
        }
    }
    fn track_rows<'a>(&'a self, c: &ControlLayout) -> Vec<(&'a Track, HitRect)> {
        let mut rows = Vec::new();
        for (id, r) in [("video", c.video_track), ("follow", c.camera_track)] {
            if let Some(t) = self.editor.settings().tracks.iter().find(|t| t.id == id) {
                rows.push((t, r));
            }
        }
        rows.extend(
            c.media_tracks
                .iter()
                .enumerate()
                .filter_map(|(i, r)| self.row_track(i).map(|t| (t, *r))),
        );
        rows
    }
    fn row_ranges(&self, t: &Track) -> Vec<(bool, bool, String, TimeTick, TimeTick, String)> {
        let s = self.editor.settings();
        match t.kind {
            TrackKind::Narration | TrackKind::Music => s
                .audio_clips
                .iter()
                .filter(|a| s.audio_track(a) == t.id)
                .map(|a| (false, false, a.id.clone(), a.start, a.end(), a.name.clone()))
                .collect(),
            TrackKind::Subtitle => s
                .subtitles
                .iter()
                .filter(|a| s.subtitle_track(&a.id) == t.id)
                .flat_map(|a| {
                    (if a.anchor == TimeAnchor::Project {
                        vec![(a.start, a.end)]
                    } else {
                        s.source_ranges(self.editor.duration(), a.start, a.end)
                    })
                    .into_iter()
                    .map(|(b, e)| (true, false, a.id.clone(), b, e, a.text.replace('\n', " ")))
                })
                .collect(),
            TrackKind::Video => s
                .visual_clips
                .iter()
                .filter(|a| a.track_id == t.id)
                .map(|a| {
                    (
                        false,
                        true,
                        a.id.clone(),
                        a.start,
                        a.end(),
                        s.visual_assets
                            .iter()
                            .find(|v| v.id == a.asset_id)
                            .map_or("视频", |v| v.name.as_str())
                            .to_owned(),
                    )
                })
                .collect(),
            TrackKind::Follow => self
                .display_segments()
                .into_iter()
                .filter(|c| s.clip_tracks.get(&c.id).map_or("follow", String::as_str) == t.id)
                .map(|c| (false, false, c.id, c.start_tick, c.end_tick, "跟随".into()))
                .collect(),
        }
    }
    fn track_on(&self, t: &Track) -> bool {
        let s = self.editor.settings();
        !t.muted
            && match t.kind {
                TrackKind::Video if t.id == "video" && s.source_video_enabled => {
                    !s.audio.source_muted
                }
                TrackKind::Follow => s.camera_enabled,
                TrackKind::Subtitle => s.subtitle_style.enabled,
                _ => true,
            }
    }
    pub(super) fn track_switch_at(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let c = self.control_layout(client_rect(hwnd));
        if x < c.ruler.left - self.theme.scale(27) || x >= c.ruler.left - self.theme.scale(3) {
            return false;
        }
        let Some(t) = self
            .track_rows(&c)
            .into_iter()
            .find(|(_, r)| {
                r.bottom > r.top
                    && y >= r.top
                    && y < r.bottom
                    && y >= c.track_area.top
                    && y < c.track_area.bottom
            })
            .map(|(t, _)| t.clone())
        else {
            return false;
        };
        let mut s = self.editor.settings().clone();
        let muted = self.track_on(&t);
        if !muted {
            if t.kind == TrackKind::Follow {
                s.camera_enabled = true;
            }
            if t.kind == TrackKind::Subtitle {
                s.subtitle_style.enabled = true;
            }
        }
        if let Some(track) = s.tracks.iter_mut().find(|v| v.id == t.id) {
            track.muted = muted;
        }
        if t.id == "video" && s.source_video_enabled {
            s.audio.source_muted = muted;
        }
        let result = self
            .editor
            .replace_media_settings(s)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "轨道状态已更新 · 可撤销");
        self.refresh(hwnd, true);
        true
    }
    pub(super) fn draw_track_switches(
        &self,
        dc: windows::Win32::Graphics::Gdi::HDC,
        c: &ControlLayout,
    ) {
        for (t, r) in self.track_rows(c) {
            if r.bottom <= r.top {
                continue;
            }
            let on = self.track_on(t);
            draw_icon(
                dc,
                RECT {
                    left: c.ruler.left - self.theme.scale(24),
                    right: c.ruler.left - self.theme.scale(8),
                    top: (r.top + r.bottom) / 2 - self.theme.scale(8),
                    bottom: (r.top + r.bottom) / 2 + self.theme.scale(8),
                },
                if matches!(t.kind, TrackKind::Subtitle | TrackKind::Follow) {
                    IconKind::Eye
                } else if on {
                    IconKind::Sound
                } else {
                    IconKind::Muted
                },
                if on {
                    self.theme.palette.text_secondary
                } else {
                    self.theme.palette.text_muted
                },
            );
        }
    }
    pub(super) fn draw_media_tracks(
        &self,
        dc: windows::Win32::Graphics::Gdi::HDC,
        c: &ControlLayout,
    ) {
        let p = self.theme.palette;
        for (t, track) in self.track_rows(c) {
            if track.bottom <= track.top {
                continue;
            }
            if t.id != "video" && t.id != "follow" {
                fill(dc, &track.0, p.track);
                let icon = match t.kind {
                    TrackKind::Video => IconKind::Monitor,
                    TrackKind::Follow => IconKind::Camera,
                    TrackKind::Narration => IconKind::Microphone,
                    TrackKind::Music => IconKind::Music,
                    TrackKind::Subtitle => IconKind::Subtitles,
                };
                draw_icon(
                    dc,
                    RECT {
                        left: c.track_labels.left,
                        right: c.track_labels.left + self.theme.scale(14),
                        top: (track.top + track.bottom) / 2 - self.theme.scale(7),
                        bottom: (track.top + track.bottom) / 2 + self.theme.scale(7),
                    },
                    icon,
                    p.text_muted,
                );
                draw_text_styled(
                    dc,
                    RECT {
                        left: c.track_labels.left + self.theme.scale(22),
                        right: track.left - self.theme.scale(28),
                        top: track.top,
                        bottom: track.bottom,
                    },
                    &t.name,
                    p.text_secondary,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                    TextStyle::Micro,
                );
            }
            for (sub, visual, id, a, b, label) in self.row_ranges(t) {
                if t.id == "follow" {
                    continue;
                }
                let r = self.media_rect(track, a, b);
                if r.right <= r.left {
                    continue;
                }
                let selected = self
                    .media_selected
                    .as_ref()
                    .is_some_and(|(s, i)| *s == sub && *i == id)
                    || self.selected_segment.as_deref() == Some(id.as_str())
                    || self.selected_video.as_deref() == Some(id.as_str());
                crate::platform::editor_ui::track_clip(
                    dc,
                    r,
                    if visual { p.video_clip } else { p.camera_clip },
                    p.text_muted,
                    selected,
                    self.theme,
                );
                let lh = self.theme.scale(15).min((r.bottom - r.top) / 2);
                draw_text_styled(
                    dc,
                    RECT {
                        left: r.left + self.theme.scale(6),
                        right: r.right - self.theme.scale(6),
                        top: r.top,
                        bottom: if sub { r.bottom } else { r.top + lh },
                    },
                    &label,
                    p.text,
                    DT_LEFT
                        | DT_VCENTER
                        | DT_SINGLELINE
                        | windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS,
                    TextStyle::Micro,
                );
                if !sub && !visual {
                    if let Some(clip) = self
                        .editor
                        .settings()
                        .audio_clips
                        .iter()
                        .find(|a| a.id == id)
                    {
                        let duration =
                            crate::platform::audio::tick_at(clip.asset_frames).0.max(1) as f64;
                        let middle = (r.top + lh + r.bottom) / 2;
                        let half = (r.bottom - r.top - lh - 4).max(1) / 2;
                        for x in (r.left + self.theme.scale(6)..r.right - self.theme.scale(6))
                            .step_by(self.theme.scale(4).max(1) as usize)
                        {
                            let source = clip.source_in.0
                                + (timeline_tick(track, x, self.timeline).0 - clip.start.0)
                                    * i64::from(clip.speed_percent)
                                    / 100;
                            let end_source=clip.source_in.0
                                +(timeline_tick(track,x+self.theme.scale(4),self.timeline).0-clip.start.0)*i64::from(clip.speed_percent)/100;
                            let peak=panzo_core::media_edit::waveform_peak(&clip.waveform,source as f64/duration,end_source as f64/duration);
                            let amplitude=(peak as f64*clip.gain).clamp(0.0,1.0);
                            let height=if amplitude>0.0 {(amplitude.sqrt()*half as f64).round().max(1.0) as i32}else{0};
                            fill(
                                dc,
                                &RECT {
                                    left: x,
                                    right: x + self.theme.scale(2),
                                    top: middle - height,
                                    bottom: middle + height + 1,
                                },
                                p.accent,
                            );
                        }
                    }
                }
                if visual {
                    if let Some(clip) = self
                        .editor
                        .settings()
                        .visual_clips
                        .iter()
                        .find(|v| v.id == id)
                    {
                        if let Some(bin) = &self.project_bin {
                            project_bin::draw_track_thumbnail(
                                bin,
                                dc,
                                clip,
                                (clip.source_tick(a.max(self.timeline.visible_start())),clip.source_tick(b.min(self.timeline.visible_end()))),
                                RECT {
                                    left: r.left + self.theme.scale(4),
                                    right: r.right - self.theme.scale(4),
                                    top: r.top + lh + 1,
                                    bottom: r.bottom - self.theme.scale(3),
                                },
                                self.theme,
                            );
                        }
                    }
                }
                crate::platform::editor_ui::track_grips(
                    dc,
                    r,
                    p.text_secondary,
                    selected,
                    self.theme,
                );
            }
        }
        if let Some(d) = &self.media_drag {
            if let Some((a, b)) = self.media_drag_range(d, c) {
                if let Some((_, r)) = self
                    .track_rows(c)
                    .into_iter()
                    .find(|(t, _)| d.target_track.as_ref().is_some_and(|id| id == &t.id))
                {
                    border(dc, self.media_rect(r, a, b), p.accent, 2);
                }
            }
        }
    }

    fn media_hit(
        &self,
        c: &ControlLayout,
        x: i32,
        y: i32,
    ) -> Option<(bool, bool, String, TimeTick, TimeTick, RECT)> {
        if !c.track_area.contains(x, y) {
            return None;
        }
        for (t, r) in self.track_rows(c) {
            if r.bottom <= r.top || !r.contains(x, y) {
                continue;
            }
            for (sub, visual, id, a, b, _) in self.row_ranges(t).into_iter().rev() {
                let rect = self.media_rect(r, a, b);
                if HitRect(rect).contains(x, y) {
                    return Some((sub, visual, id, a, b, rect));
                }
            }
        }
        None
    }
    pub(super) fn media_down(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let c = self.control_layout(client_rect(hwnd));
        let Some((subtitle, visual, id, a, b, r)) = self.media_hit(&c, x, y) else {
            if c.track_area.contains(x, y)
                && c.media_tracks
                    .iter()
                    .any(|r| r.bottom > r.top && r.contains(x, y))
            {
                self.begin_timeline_scrub(hwnd, c.ruler, x);
                return true;
            }
            return false;
        };
        self.session.pause();
        self.selected_segment = None;
        self.selected_video = None;
        self.media_selected = Some((subtitle, id.clone()));
        if visual { self.select_video_properties(); }
        let follow = self.editor.camera().segments.iter().any(|c| c.id == id);
        let (kind, origin_track) = self
            .track_rows(&c)
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(t, _)| (t.kind, t.id.clone()))
            .unwrap();
        self.media_drag = Some(MediaDrag {
            subtitle,
            visual,
            follow,
            legacy: false,
            kind,
            origin_track: origin_track.clone(),
            target_track: Some(origin_track),
            view_start: self.timeline.visible_start(),
            id,
            start_x: x,
            current_x: x,
            current_y: y,
            mode: timeline_edge_mode(x, r.left, r.right, self.theme.scale(6)),
            original: self.editor.settings().clone(),
            range: (a, b),
        });
        unsafe {
            SetCapture(hwnd);
        }
        self.refresh(hwnd, true);
        true
    }
    pub(super) fn begin_source_move(
        &mut self,
        hwnd: HWND,
        x: i32,
        y: i32,
        id: String,
        a: TimeTick,
        b: TimeTick,
    ) {
        self.session.pause();
        self.deferred_playback.cancel();
        self.media_drag = Some(MediaDrag {
            subtitle: false,
            visual: true,
            follow: false,
            legacy: true,
            kind: TrackKind::Video,
            origin_track: "video".into(),
            target_track: Some("video".into()),
            view_start: self.timeline.visible_start(),
            id,
            start_x: x,
            current_x: x,
            current_y: y,
            mode: TimelineDragMode::Move,
            original: self.editor.settings().clone(),
            range: (a, b),
        });
        unsafe {
            SetCapture(hwnd);
        }
    }
    pub(super) fn media_snap_points(&self, exclude: &str) -> Vec<i64> {
        let mut points = vec![0, self.session.project_playhead().0];
        let c = self.control_layout(self.host_hwnd.map_or(RECT::default(), client_rect));
        if self.editor.settings().source_video_enabled {
            for span in self.editor.video_edit().spans() {
                if span.clip.id != exclude {
                    points.extend([span.project_in.0, span.project_out.0]);
                }
            }
        }
        for (t, _) in self.track_rows(&c) {
            for (_, _, id, a, b, _) in self.row_ranges(t) {
                if id != exclude {
                    points.extend([a.0, b.0]);
                }
            }
        }
        points.sort_unstable();
        points.dedup();
        points
    }
    fn media_drag_range(&self, d: &MediaDrag, c: &ControlLayout) -> Option<(TimeTick, TimeTick)> {
        let delta = self
            .timeline
            .pixel_delta_to_tick(d.current_x - d.start_x, c.ruler.right - c.ruler.left)
            .0
            + self.timeline.visible_start().0
            - d.view_start.0;
        let mut bounds = (0, panzo_core::media_edit::MAX_MEDIA_TICK);
        if d.mode != TimelineDragMode::Move {
            let source = if d.visual {
                d.original
                    .visual_clips
                    .iter()
                    .find(|v| v.id == d.id)
                    .and_then(|v| {
                        d.original
                            .visual_assets
                            .iter()
                            .find(|a| a.id == v.asset_id)
                            .map(|a| (v.source_in.0, v.source_out.0, a.duration.0, v.speed_percent))
                    })
            } else {
                d.original
                    .audio_clips
                    .iter()
                    .find(|v| v.id == d.id)
                    .map(|v| {
                        (
                            v.source_in.0,
                            v.source_out.0,
                            crate::platform::audio::tick_at(v.asset_frames).0,
                            v.speed_percent,
                        )
                    })
            };
            if let Some((a, b, total, speed)) = source {
                bounds.0 = (d.range.0.0 - a * 100 / i64::from(speed)).max(0);
                bounds.1 = (d.range.1.0 + (total - b) * 100 / i64::from(speed)).min(bounds.1);
            }
        }
        let points = self.media_snap_points(&d.id);
        let tolerance = if unsafe { GetKeyState(0x12) } < 0 {
            0
        } else {
            self.timeline
                .pixel_delta_to_tick(self.theme.scale(8), c.ruler.right - c.ruler.left)
                .0
                .unsigned_abs()
        };
        let (a, b) = crate::platform::edit_gestures::adjusted_range(
            (d.range.0.0, d.range.1.0),
            delta,
            bounds,
            d.mode,
            &points,
            tolerance,
        );
        let target=d.target_track.as_deref()?;
        let track=self.editor.settings().tracks.iter().find(|t|t.id==target)?;
        let mut occupied=self.row_ranges(track).into_iter().filter(|(_,_,id,_,_,_)|id!=&d.id)
            .map(|(_,_,_,a,b,_)|(a.0,b.0)).collect::<Vec<_>>();
        if target=="video" && self.editor.settings().source_video_enabled {
            occupied.extend(self.editor.video_edit().spans().into_iter().filter(|s|s.clip.id!=d.id).map(|s|(s.project_in.0,s.project_out.0)));
        }
        let (a,b)=panzo_core::media_edit::resolve_clip_placement((a,b),(d.range.0.0,d.range.1.0),d.mode==TimelineDragMode::Move,d.mode==TimelineDragMode::TrimStart,&occupied)?;
        Some((TimeTick(a), TimeTick(b)))
    }
    fn media_drag_target(
        &self,
        c: &ControlLayout,
        x: i32,
        y: i32,
        d: &MediaDrag,
    ) -> Option<String> {
        if d.mode != TimelineDragMode::Move {
            return Some(d.origin_track.clone());
        }
        if !c.track_area.contains(x, y) {
            return None;
        }
        self.track_rows(c)
            .into_iter()
            .find(|(t, r)| {
                r.bottom > r.top
                    && r.contains(x, y)
                    && (t.kind == d.kind || (t.kind.audio() && d.kind.audio()))
            })
            .map(|(t, _)| t.id.clone())
    }
    pub(super) fn media_move(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(mut d) = self.media_drag.take() else {
            return false;
        };
        let c = self.control_layout(client_rect(hwnd));
        d.current_x = x;
        d.current_y = y;
        d.target_track = self.media_drag_target(&c, x, y, &d);
        self.snap_guide = if unsafe { GetKeyState(0x12) } < 0 {
            None
        } else {
            self.media_drag_range(&d, &c).and_then(|(a, b)| {
                let points = self.media_snap_points(&d.id);
                [a, b].into_iter().find(|t| points.contains(&t.0))
            })
        };
        self.media_drag = Some(d);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&c.timeline_panel), false);
        }
        true
    }
    pub(super) fn media_up(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if self.media_drag.is_some() {
            self.media_move(hwnd, x, y);
        }
        let Some(mut d) = self.media_drag.take() else {
            return false;
        };
        d.current_x = x;
        unsafe {
            let _ = ReleaseCapture();
        }
        let c = self.control_layout(client_rect(hwnd));
        let moved = (x - d.start_x).abs() > self.theme.scale(2)
            || d.target_track
                .as_ref()
                .is_some_and(|t| t != &d.origin_track);
        self.snap_guide = None;
        if d.target_track.is_none() {
            self.refresh(hwnd, true);
            return true;
        }
        if moved {
            if let Some((a, b)) = self.media_drag_range(&d, &c) {
                let result = (|| {
                    if self.editor.settings() != &d.original {
                        return Err("工程已变化，请重新拖动".to_owned());
                    }
                    let mut next = d.original.clone();
                    let target = d.target_track.clone().unwrap();
                    if d.legacy {
                        ensure_source_asset(self.editor.project_root(), &mut next)?;
                        let video = self.editor.video_edit();
                        for span in video.spans() {
                            let id =
                                next.insert_visual_asset("__source__", "video", span.project_in)?;
                            let clip = next.visual_clips.iter_mut().find(|v| v.id == id).unwrap();
                            clip.id = span.clip.id.clone();
                            clip.source_in = span.clip.source_in_tick;
                            clip.source_out = span.clip.source_out_tick;
                            clip.speed_percent = span.clip.speed_percent;
                            clip.crop = span.clip.crop;
                            clip.scale = f64::from(span.clip.scale_per_mille) / 1000.0;
                            // Track mute is independent of persistent clip volume.
                            clip.gain = 1.0;
                        }
                        if next.audio.source_muted {if let Some(t)=next.tracks.iter_mut().find(|t|t.id=="video"){t.muted=true;}}
                        next.source_video_enabled = false;
                        next.audio.source_muted = true;
                    }
                    if d.follow {
                        next.follow_positions.insert(d.id.clone(), (a, b));
                        next.clip_tracks.insert(d.id.clone(), target);
                        self.editor
                            .replace_media_settings(next)
                            .map_err(|e| e.to_string())?;
                        return self.apply_editor_state().map_err(|e| e.to_string());
                    }
                    if d.visual {
                        let clip = next
                            .visual_clips
                            .iter_mut()
                            .find(|v| v.id == d.id)
                            .ok_or("片段已删除")?;
                        clip.track_id = target.clone();
                        let duration = next
                            .visual_assets
                            .iter()
                            .find(|v| v.id == clip.asset_id)
                            .ok_or("素材已删除")?
                            .duration;
                        match d.mode {
                            TimelineDragMode::Move => clip.start = a,
                            TimelineDragMode::TrimStart => {
                                let change = ((a.0 - clip.start.0) * i64::from(clip.speed_percent)
                                    / 100)
                                    .max(-clip.source_in.0);
                                clip.source_in.0 += change;
                                clip.start.0 += change * 100 / i64::from(clip.speed_percent);
                            }
                            TimelineDragMode::TrimEnd => {
                                clip.source_out = TimeTick(
                                    (clip.source_in.0
                                        + (b.0 - clip.start.0) * i64::from(clip.speed_percent)
                                            / 100)
                                        .min(duration.0),
                                )
                            }
                        }
                        clip.fade_in = clip.fade_in.min(clip.duration());
                        clip.fade_out = clip.fade_out.min(clip.duration());
                    } else if d.subtitle {
                        let cue = next
                            .subtitles
                            .iter_mut()
                            .find(|v| v.id == d.id)
                            .ok_or("字幕已删除")?;
                        cue.start = a;
                        cue.end = b;
                        cue.anchor = TimeAnchor::Project;
                        next.clip_tracks.insert(d.id.clone(), target.clone());
                    } else {
                        let clip = next
                            .audio_clips
                            .iter_mut()
                            .find(|v| v.id == d.id)
                            .ok_or("音频已删除")?;
                        match d.mode {
                            TimelineDragMode::Move => clip.start = a,
                            TimelineDragMode::TrimStart => {
                                let change = ((a.0 - clip.start.0) * i64::from(clip.speed_percent)
                                    / 100)
                                    .max(-clip.source_in.0);
                                clip.start.0 += change * 100 / i64::from(clip.speed_percent);
                                clip.source_in.0 += change;
                            }
                            TimelineDragMode::TrimEnd => {
                                clip.source_out = TimeTick(
                                    (clip.source_in.0
                                        + (b.0 - clip.start.0) * i64::from(clip.speed_percent)
                                            / 100)
                                        .min(crate::platform::audio::tick_at(clip.asset_frames).0),
                                )
                            }
                        }
                        clip.role = if next
                            .tracks
                            .iter()
                            .any(|t| t.id == target && t.kind == TrackKind::Music)
                        {
                            panzo_core::media_edit::AudioRole::Music
                        } else {
                            panzo_core::media_edit::AudioRole::Narration
                        };
                        clip.fade_in = clip.fade_in.min(clip.duration());
                        clip.fade_out = clip.fade_out.min(clip.duration());
                        next.clip_tracks.insert(d.id.clone(), target);
                    }
                    self.editor
                        .replace_media_settings(next)
                        .map_err(|e| e.to_string())?;
                    self.apply_editor_state().map_err(|e| e.to_string())
                })();
                if let Err(e) = result {
                    self.note_problem("片段未移动", e);
                }
            }
        }
        if d.follow {
            self.media_selected = None;
            self.selected_video = None;
            self.selected_segment = Some(d.id);
            self.clear_inspector_panel();
            self.inspector_video_tab = false;
        } else if !d.legacy || moved {
            self.media_selected = Some((d.subtitle, d.id.clone()));
            self.selected_video = None;
            if d.visual { self.select_video_properties(); }
            if !moved {
                if !d.visual {
                    crate::platform::media_panel::select_item(hwnd, d.subtitle, &d.id);
                }
            }
        }
        self.refresh(hwnd, true);
        true
    }
    pub(super) fn selected_media_range(&self) -> Option<(TimeTick, TimeTick)> {
        let (sub, id) = self.media_selected.as_ref()?;
        if *sub {
            self.editor
                .settings()
                .cue_ranges(
                    self.editor
                        .settings()
                        .subtitles
                        .iter()
                        .find(|c| &c.id == id)?,
                    self.editor.duration(),
                )
                .first()
                .copied()
        } else {
            self.editor
                .settings()
                .audio_clips
                .iter()
                .find(|c| &c.id == id)
                .map(|c| (c.start, c.end()))
                .or_else(|| {
                    self.editor
                        .settings()
                        .visual_clips
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| (c.start, c.end()))
                })
        }
    }
    pub(super) fn delete_media(&mut self) {
        let Some((sub, id)) = self.media_selected.clone() else {
            return;
        };
        let mut next = self.editor.settings().clone();
        if sub {
            next.subtitles.retain(|c| c.id != id);
            next.subtitle_styles.remove(&id);
        } else {
            next.audio_clips.retain(|c| c.id != id);
            next.visual_clips.retain(|c| c.id != id);
        }
        next.clip_tracks.remove(&id);
        let r = self
            .editor
            .replace_media_settings(next)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(r, "片段已删除 · 可撤销");
    }
    pub(super) fn split_media(&mut self) {
        let Some((sub, id)) = self.media_selected.clone() else {
            return;
        };
        let tick = self.session.project_playhead();
        let result = (|| {
            let mut next = self.editor.settings().clone();
            let right_id = uuid::Uuid::new_v4().to_string();
            if sub {
                let i = next
                    .subtitles
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("字幕已删除")?;
                let mut left = next.subtitles[i].clone();
                let mut right = left.clone();
                let at = if left.anchor == TimeAnchor::Source {
                    next.source_time_at(self.editor.duration(), tick)
                        .ok_or("当前位置没有源视频")?
                } else {
                    tick
                };
                if at <= left.start || at >= left.end {
                    return Err("播放头须位于片段内部".into());
                }
                left.end = at;
                right.start = at;
                right.id = right_id.clone();
                if let Some(style)=next.subtitle_styles.get(&id).cloned(){next.subtitle_styles.insert(right_id.clone(),style);}
                next.subtitles.splice(i..=i, [left, right]);
            } else if let Some(i) = next.visual_clips.iter().position(|c| c.id == id) {
                let mut left = next.visual_clips[i].clone();
                if !left.active(tick) || tick == left.start {
                    return Err("播放头须位于片段内部".into());
                }
                let mut right = left.clone();
                left.source_out = left.source_tick(tick);
                right.source_in = left.source_out;
                right.start = tick;
                right.id = right_id.clone();
                left.fade_in = left.fade_in.min(left.duration());
                left.fade_out = left.fade_out.min(left.duration());
                right.fade_in = right.fade_in.min(right.duration());
                right.fade_out = right.fade_out.min(right.duration());
                next.visual_clips.splice(i..=i, [left, right]);
            } else {
                let i = next
                    .audio_clips
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("音频已删除")?;
                let (a, mut b) = next.audio_clips[i].split(tick)?;
                b.id = right_id.clone();
                next.audio_clips.splice(i..=i, [a, b]);
            }
            if let Some(asset) = next.subtitle_links.get(&id).cloned() {
                next.subtitle_links.insert(right_id.clone(), asset);
            }
            if let Some(track) = next.clip_tracks.get(&id).cloned() {
                next.clip_tracks.insert(right_id, track);
            }
            self.editor
                .replace_media_settings(next)
                .map_err(|e| e.to_string())?;
            self.apply_editor_state().map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            self.note_problem("片段未分割", e);
        } else {
            self.set_status(StatusSeverity::Success, "片段已分割 · 可撤销".into());
        }
    }
    pub(super) fn media_context_menu(&mut self, hwnd: HWND, x: i32, y: i32, keyboard: bool) {
        use crate::platform::app_menu::{self, Row};
        if self.media_drag.is_some()
            || self.video_drag.is_some()
            || !self.finish_inline_input(hwnd, true)
        {
            return;
        }
        let c = self.control_layout(client_rect(hwnd));
        if !keyboard {
            if let Some((sub, _, id, _, _, _)) = self.media_hit(&c, x, y) {
                if self.editor.camera().segments.iter().any(|v| v.id == id) {
                    self.media_selected = None;
                    self.selected_video = None;
                    self.selected_segment = Some(id);
                    let rows = vec![Row::command(1, "删除跟随片段", IconKind::Delete, true)];
                    if let Ok(Some(1)) = app_menu::show_rows(
                        hwnd,
                        RECT {
                            left: x,
                            right: x + 2,
                            top: y,
                            bottom: y + 1,
                        },
                        self.theme,
                        rows,
                        false,
                    ) {
                        self.delete_selected();
                        self.refresh(hwnd, true);
                    }
                    return;
                }
                self.media_selected = Some((sub, id));
                self.selected_video = None;
                self.selected_segment = None;
            } else {
                if y >= c.track_area.top && y < c.track_area.bottom {
                    self.track_menu(hwnd, x, y);
                }
                return;
            }
        }
        let Some((sub, id)) = self.media_selected.clone() else {
            return;
        };
        let rows = vec![
            Row::command(
                1,
                "在播放头分割",
                IconKind::Split,
                self.control_enabled(ControlId::Split),
            ),
            Row::command(2, "删除片段", IconKind::Delete, true),
            Row::command(3, "片段属性", IconKind::Settings, true),
            Row::command(4, "位置与缩放", IconKind::Settings, self.editor.settings().visual_clips.iter().any(|c|c.id==id)),
        ];
        let anchor = RECT {
            left: x,
            right: x + 2,
            top: y,
            bottom: y + 1,
        };
        match app_menu::show_rows(hwnd, anchor, self.theme, rows, false) {
            Ok(Some(1)) => self.split_media(),
            Ok(Some(2)) => self.delete_media(),
            Ok(Some(4)) => { if let Err(e)=crate::platform::visual_panel::open(hwnd,&id,false){self.note_problem("视频属性未打开",e);} },
            Ok(Some(3)) => {
                if self
                    .editor
                    .settings()
                    .visual_clips
                    .iter()
                    .any(|c| c.id == id)
                {
                    self.select_video_properties();
                } else {
                    crate::platform::media_panel::select_item(hwnd, sub, &id);
                }
            }
            Err(e) => self.note_problem("菜单未打开", e.to_string()),
            _ => {}
        }
        self.refresh(hwnd, true);
    }
    pub(super) fn media_audio_properties(&self, hwnd: HWND, id: &str) -> Result<(), String> {
        crate::platform::media_panel::select_item(hwnd, false, id);
        Ok(())
    }
    pub(super) fn media_cursor(&self, client: RECT, x: i32, y: i32) -> Option<PCWSTR> {
        if let Some(d) = &self.media_drag {
            return Some(if d.target_track.is_none() {
                windows::Win32::UI::WindowsAndMessaging::IDC_NO
            } else if d.mode == TimelineDragMode::Move {
                IDC_SIZEALL
            } else {
                IDC_SIZEWE
            });
        }
        let c = self.control_layout(client);
        self.media_hit(&c, x, y).map(|(_, _, _, _, _, r)| {
            if timeline_edge_mode(x, r.left, r.right, self.theme.scale(6)) == TimelineDragMode::Move
            {
                IDC_HAND
            } else {
                IDC_SIZEWE
            }
        })
    }
    pub(super) fn track_menu(&mut self, hwnd: HWND, x: i32, y: i32) {
        use crate::platform::app_menu::{self, Row};
        let c = self.control_layout(client_rect(hwnd));
        let selected = self
            .track_rows(&c)
            .into_iter()
            .find(|(_, r)| r.bottom > r.top && y >= r.top && y < r.bottom)
            .map(|(t, _)| t.clone());
        let kinds = [
            TrackKind::Video,
            TrackKind::Follow,
            TrackKind::Narration,
            TrackKind::Subtitle,
            TrackKind::Music,
        ];
        let mut rows = kinds
            .iter()
            .enumerate()
            .map(|(i, _k)| {
                Row::command(
                    i as i32 + 1,
                    [
                        "新增视频轨道",
                        "新增跟随轨道",
                        "新增旁白轨道",
                        "新增字幕轨道",
                        "新增音乐轨道",
                    ][i],
                    IconKind::Add,
                    true,
                )
            })
            .collect::<Vec<_>>();
        if selected
            .as_ref()
            .is_some_and(|t| t.kind == TrackKind::Follow)
        {
            rows.push(Row::command(
                10,
                "在播放头添加跟随片段",
                IconKind::Add,
                true,
            ));
        }
        if selected.is_some() {
            rows.push(Row::command(9, "删除此轨道及片段", IconKind::Delete, true));
        }
        let anchor = RECT {
            left: x,
            right: x + 2,
            top: y,
            bottom: y + 1,
        };
        let Ok(Some(choice)) = app_menu::show_rows(hwnd, anchor, self.theme, rows, false) else {
            return;
        };
        if choice == 10 {
            let t = selected.unwrap();
            self.editor.begin_edit_group();
            let before = self
                .editor
                .camera()
                .segments
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>();
            self.add_manual_segment();
            if let Some(id) = self
                .selected_segment
                .clone()
                .filter(|id| !before.contains(id))
            {
                let mut s = self.editor.settings().clone();
                s.clip_tracks.insert(id, t.id);
                let result = self
                    .editor
                    .replace_media_settings(s)
                    .map_err(PreviewWindowError::from)
                    .and_then(|()| self.apply_editor_state());
                self.record_editor_result(result, "跟随片段已添加");
            }
            self.editor.finish_edit_group();
            self.refresh(hwnd, true);
            return;
        }
        let mut next = self.editor.settings().clone();
        let mut camera_ids = Vec::new();
        let result = if choice == 9 {
            let t = selected.unwrap();
            if unsafe {
                MessageBoxW(
                    Some(hwnd),
                    w!("删除此轨道及其中的片段？项目素材将保留，此操作可以撤销。"),
                    w!("删除轨道"),
                    MB_OKCANCEL | MB_ICONQUESTION,
                )
            } != IDOK
            {
                return;
            }
            camera_ids = self
                .editor
                .camera()
                .segments
                .iter()
                .filter(|c| next.clip_tracks.get(&c.id).map_or("follow", String::as_str) == t.id)
                .map(|c| c.id.clone())
                .collect();
            next.remove_track(&t.id)
        } else {
            next.add_track(kinds[(choice - 1) as usize]).map(|_| ())
        };
        self.editor.begin_edit_group();
        let result = result
            .map_err(|e| e.to_string())
            .and_then(|()| {
                for id in camera_ids {
                    self.editor.delete_segment(&id).map_err(|e| e.to_string())?;
                }
                self.editor
                    .replace_media_settings(next)
                    .map_err(|e| e.to_string())
            })
            .and_then(|()| self.apply_editor_state().map_err(|e| e.to_string()));
        self.editor.finish_edit_group();
        if let Err(e) = result {
            self.note_problem("轨道未更新", e);
        }
        self.refresh(hwnd, true);
    }
}
