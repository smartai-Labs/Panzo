//! Editable image objects, non-destructive framing and bounded undo history.
use super::{POINT, RECT, Raster};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Tool {
    Select,
    #[default]
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Pen,
    Highlight,
    Text,
    Number,
    Mosaic,
    Blur,
    Cover,
    Crop,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Mark {
    #[serde(default)]
    pub text_box: Option<[u32; 2]>,
    #[serde(default)]
    pub font: panzo_core::text_style::FontStyle,
    pub tool: Tool,
    #[serde(with = "points")]
    pub points: Vec<POINT>,
    pub color: [u8; 4],
    pub text: String,
    #[serde(default = "stroke_default")]
    pub stroke: u32,
    #[serde(default)]
    pub filled: bool,
    #[serde(default = "font_default")]
    pub font_size: u32,
}
const fn stroke_default() -> u32 {
    3
}
const fn font_default() -> u32 {
    24
}
impl Default for Mark {
    fn default() -> Self {
        Self {
            tool: Tool::Rectangle,
            points: Vec::new(),
            color: [55, 65, 225, 255],
            text: String::new(),
            stroke: 3,
            filled: false,
            font_size: 24,
            font: Default::default(),
            text_box: None,
        }
    }
}
mod points {
    use super::{Deserialize, POINT, Serialize};
    pub fn serialize<S: serde::Serializer>(
        value: &[POINT],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .iter()
            .map(|p| [p.x, p.y])
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<POINT>, D::Error> {
        Vec::<[i32; 2]>::deserialize(deserializer)
            .map(|v| v.into_iter().map(|[x, y]| POINT { x, y }).collect())
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct ImageState {
    pub marks: Vec<Mark>,
    pub crop: Option<[u32; 4]>,
    pub rotation: u8,
    pub flip_x: bool,
    pub flip_y: bool,
}
#[derive(Clone)]
pub(super) struct Document {
    pub original: Raster,
    pub state: ImageState,
    undo: Vec<ImageState>,
    redo: Vec<ImageState>,
    pub revision: u64,
}
impl Document {
    pub fn new(original: Raster) -> Self {
        Self {
            original,
            state: ImageState::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }
    pub fn from_state(original: Raster, state: ImageState) -> Result<Self, String> {
        let mut result = Self::new(original);
        result.state = state;
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<(), String> {
        let invalid = self.state.rotation > 3
            || self.state.marks.len() > 1000
            || self
                .state
                .marks
                .iter()
                .map(|m| m.points.len())
                .sum::<usize>()
                > 100_000;
        if invalid {
            return Err("图片编辑数据超出范围".into());
        }
        for mark in &self.state.marks {
            if !(1..=24).contains(&mark.stroke)
                || !(1..=512).contains(&mark.font_size)
                || mark
                    .text_box
                    .is_some_and(|s| s[0] == 0 || s[1] == 0 || s[0] > 100_000 || s[1] > 100_000)
                || !mark.font.valid()
                || mark.text.len() > 16_384
                || mark.points.iter().any(|p| {
                    p.x < 0
                        || p.y < 0
                        || p.x > self.original.width as i32
                        || p.y > self.original.height as i32
                })
            {
                return Err("图片标注数据无效".into());
            }
        }
        if let Some([x, y, width, height]) = self.state.crop
            && (width == 0
                || height == 0
                || u64::from(x) + u64::from(width) > u64::from(self.original.width)
                || u64::from(y) + u64::from(height) > u64::from(self.original.height))
        {
            return Err("图片裁剪数据无效".into());
        }
        Ok(())
    }
    pub fn checkpoint(&mut self) {
        self.undo.push(self.state.clone());
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
    }
    pub fn commit(&mut self, mark: Mark) {
        if self.state.marks.len() >= 1000 {
            return;
        }
        self.checkpoint();
        self.state.marks.push(mark);
    }
    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.state, previous));
            self.revision += 1;
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.state, next));
            self.revision += 1;
        }
    }
    pub fn crop_rect(&self) -> [u32; 4] {
        self.state
            .crop
            .unwrap_or([0, 0, self.original.width, self.original.height])
    }
    pub fn size(&self) -> (u32, u32) {
        let [_, _, width, height] = self.crop_rect();
        if self.state.rotation.is_multiple_of(2) {
            (width, height)
        } else {
            (height, width)
        }
    }
    pub fn source_point(&self, point: POINT) -> POINT {
        let (width, height) = self.size();
        let mut px = point.x.clamp(0, width as i32 - 1);
        let mut py = point.y.clamp(0, height as i32 - 1);
        if self.state.flip_x {
            px = width as i32 - 1 - px;
        }
        if self.state.flip_y {
            py = height as i32 - 1 - py;
        }
        let [left, top, cw, ch] = self.crop_rect();
        let (px, py) = match self.state.rotation {
            1 => (py, ch as i32 - 1 - px),
            2 => (cw as i32 - 1 - px, ch as i32 - 1 - py),
            3 => (cw as i32 - 1 - py, px),
            _ => (px, py),
        };
        POINT {
            x: px + left as i32,
            y: py + top as i32,
        }
    }
    /// Convert an exclusive output edge to source coordinates, preserving the last row/column.
    pub fn source_edge(&self, point: POINT) -> POINT {
        let (width, height) = self.size();
        let mut px = point.x.clamp(0, width as i32);
        let mut py = point.y.clamp(0, height as i32);
        if self.state.flip_x {
            px = width as i32 - px;
        }
        if self.state.flip_y {
            py = height as i32 - py;
        }
        let [left, top, cw, ch] = self.crop_rect();
        let (px, py) = match self.state.rotation {
            1 => (py, ch as i32 - px),
            2 => (cw as i32 - px, ch as i32 - py),
            3 => (cw as i32 - py, px),
            _ => (px, py),
        };
        POINT {
            x: px + left as i32,
            y: py + top as i32,
        }
    }
    /// Forward mapping for editor overlays (inverse of source_point).
    pub fn display_point(&self, point: POINT) -> POINT {
        let [l, t, w, h] = self.crop_rect();
        let x = point.x - l as i32;
        let y = point.y - t as i32;
        let (mut x, mut y) = match self.state.rotation {
            1 => (h as i32 - 1 - y, x),
            2 => (w as i32 - 1 - x, h as i32 - 1 - y),
            3 => (y, w as i32 - 1 - x),
            _ => (x, y),
        };
        let (w, h) = self.size();
        if self.state.flip_x {
            x = w as i32 - 1 - x;
        }
        if self.state.flip_y {
            y = h as i32 - 1 - y;
        }
        POINT { x, y }
    }
    /// Forward edge mapping for rectangular overlay bitmaps (no pixel-center -1 offset).
    pub fn display_edge(&self, p: POINT) -> POINT {
        let [l, t, w, h] = self.crop_rect();
        let x = p.x - l as i32;
        let y = p.y - t as i32;
        let (mut x, mut y) = match self.state.rotation {
            1 => (h as i32 - y, x),
            2 => (w as i32 - x, h as i32 - y),
            3 => (y, w as i32 - x),
            _ => (x, y),
        };
        let (w, h) = self.size();
        if self.state.flip_x {
            x = w as i32 - x;
        }
        if self.state.flip_y {
            y = h as i32 - y;
        }
        POINT { x, y }
    }
    pub fn crop_output(&mut self, edges: [u32; 4]) {
        let [l, t, r, b] = edges;
        let a = self.source_edge(POINT {
            x: l as i32,
            y: t as i32,
        });
        let b = self.source_edge(POINT {
            x: r as i32,
            y: b as i32,
        });
        self.frame(a, b);
    }
    pub fn replace_mark(&mut self, index: usize, mark: Mark) -> bool {
        let Some(previous) = self.state.marks.get(index) else {
            return false;
        };
        if previous.same(&mark) {
            return false;
        }
        self.checkpoint();
        self.state.marks[index] = mark;
        true
    }
    pub fn render_replacement(&self, mark: &Mark, index: Option<usize>) -> Result<Raster, String> {
        self.render_inner(Some(mark), None, index)
    }
    pub fn render_without(&self, excluded: Option<usize>) -> Result<Raster, String> {
        self.render_inner(None, None, excluded)
    }
    pub fn frame(&mut self, a: POINT, b: POINT) {
        let [left, top, width, height] = self.crop_rect();
        let x = a.x.min(b.x).max(left as i32) as u32;
        let y = a.y.min(b.y).max(top as i32) as u32;
        let right = (a.x.max(b.x) as u32).min(left + width);
        let bottom = (a.y.max(b.y) as u32).min(top + height);
        if right > x && bottom > y && [x, y, right - x, bottom - y] != self.crop_rect() {
            self.checkpoint();
            self.state.crop = Some([x, y, right - x, bottom - y]);
        }
    }
    pub fn transform(&mut self, command: usize) {
        self.checkpoint();
        match command {
            40 => self.state.rotation = (self.state.rotation + 1) % 4,
            41 => self.state.flip_x = !self.state.flip_x,
            42 => self.state.flip_y = !self.state.flip_y,
            43 => self.state.crop = None,
            _ => {}
        }
    }
    pub fn hit(&self, point: POINT) -> Option<usize> {
        self.state.marks.iter().rposition(|mark| {
            let r = mark.bounds();
            point.x >= r.left - 5
                && point.x <= r.right + 5
                && point.y >= r.top - 5
                && point.y <= r.bottom + 5
        })
    }
    pub fn delete(&mut self, index: usize) {
        if index < self.state.marks.len() {
            self.checkpoint();
            self.state.marks.remove(index);
        }
    }
    #[cfg(test)]
    pub fn move_mark(&mut self, index: usize, dx: i32, dy: i32) {
        let Some(mark) = self.state.marks.get_mut(index) else {
            return;
        };
        let rect = mark.bounds();
        let dx = dx.clamp(
            -rect.left,
            (self.original.width as i32 - rect.right).max(-rect.left),
        );
        let dy = dy.clamp(
            -rect.top,
            (self.original.height as i32 - rect.bottom).max(-rect.top),
        );
        for point in &mut mark.points {
            point.x += dx;
            point.y += dy;
        }
        self.revision += 1;
    }
    pub fn render(&self, pending: Option<&Mark>) -> Result<Raster, String> {
        self.render_editor(pending, None)
    }
    pub fn render_editor(
        &self,
        pending: Option<&Mark>,
        selected: Option<usize>,
    ) -> Result<Raster, String> {
        self.render_inner(pending, selected, None)
    }
    fn render_inner(
        &self,
        pending: Option<&Mark>,
        selected: Option<usize>,
        excluded: Option<usize>,
    ) -> Result<Raster, String> {
        let mut raster = self.original.clone();
        for (index, mark) in self.state.marks.iter().enumerate() {
            if Some(index) != excluded {
                render_mark(&mut raster, mark)?;
            } else if let Some(replacement) = pending {
                render_mark(&mut raster, replacement)?;
            }
        }
        if let Some(mark) = pending.filter(|_| excluded.is_none()) {
            render_mark(&mut raster, mark)?;
        }
        if let Some(mark) = selected.and_then(|index| self.state.marks.get(index)) {
            let rect = mark.bounds();
            let color = [230, 150, 20, 255];
            for (a, b) in [
                ((rect.left, rect.top), (rect.right, rect.top)),
                ((rect.right, rect.top), (rect.right, rect.bottom)),
                ((rect.right, rect.bottom), (rect.left, rect.bottom)),
                ((rect.left, rect.bottom), (rect.left, rect.top)),
            ] {
                line(&mut raster, a, b, color, 1);
            }
        }
        let [x, y, width, height] = self.crop_rect();
        if self.state.crop.is_some() {
            raster = raster.crop(panzo_core::PhysicalRect {
                x: x as i32,
                y: y as i32,
                width,
                height,
            })?;
        }
        let mut image = image::RgbaImage::from_raw(raster.width, raster.height, raster.pixels)
            .ok_or("图片数据无效")?;
        image = match self.state.rotation {
            1 => image::imageops::rotate90(&image),
            2 => image::imageops::rotate180(&image),
            3 => image::imageops::rotate270(&image),
            _ => image,
        };
        if self.state.flip_x {
            image::imageops::flip_horizontal_in_place(&mut image);
        }
        if self.state.flip_y {
            image::imageops::flip_vertical_in_place(&mut image);
        }
        Ok(Raster {
            width: image.width(),
            height: image.height(),
            pixels: image.into_raw(),
        })
    }
}
impl Mark {
    pub fn same(&self, other: &Self) -> bool {
        self.tool == other.tool
            && self.color == other.color
            && self.text == other.text
            && self.filled == other.filled
            && self.stroke == other.stroke
            && self.font_size == other.font_size
            && self.font == other.font
            && self.text_box == other.text_box
            && self.points.len() == other.points.len()
            && self
                .points
                .iter()
                .zip(&other.points)
                .all(|(a, b)| a.x == b.x && a.y == b.y)
    }
    pub fn translated(&self, dx: i32, dy: i32, size: (u32, u32)) -> Self {
        let mut mark = self.clone();
        let r = mark.bounds();
        let dx = dx.clamp(-r.left, (size.0 as i32 - r.right).max(-r.left));
        let dy = dy.clamp(-r.top, (size.1 as i32 - r.bottom).max(-r.top));
        for p in &mut mark.points {
            p.x += dx;
            p.y += dy;
        }
        mark
    }
    pub fn bounds(&self) -> RECT {
        let Some(first) = self.points.first() else {
            return RECT::default();
        };
        let mut rect = RECT {
            left: first.x,
            top: first.y,
            right: first.x,
            bottom: first.y,
        };
        for point in &self.points {
            rect.left = rect.left.min(point.x);
            rect.top = rect.top.min(point.y);
            rect.right = rect.right.max(point.x);
            rect.bottom = rect.bottom.max(point.y);
        }
        if matches!(self.tool, Tool::Text | Tool::Number) {
            let [width, height] = self.text_box.unwrap_or_else(|| {
                let (width, height) = crate::platform::font_catalog::measure(
                    &self.text,
                    &self.font,
                    self.font_size as i32,
                );
                [width as u32, height as u32]
            });
            rect.right = rect.left + width as i32;
            rect.bottom = rect.top + height as i32;
        }
        rect
    }
}
#[allow(clippy::many_single_char_names)] // Conventional bounded raster coordinates.
pub(super) fn render_mark(image: &mut Raster, mark: &Mark) -> Result<(), String> {
    let Some(a) = mark.points.first() else {
        return Ok(());
    };
    let b = mark.points.last().unwrap_or(a);
    let rect = mark.bounds();
    let left = rect.left.max(0);
    let top = rect.top.max(0);
    let right = rect.right.min(image.width as i32);
    let bottom = rect.bottom.min(image.height as i32);
    let color = mark.color;
    let stroke = mark.stroke.clamp(1, 24);
    if mark.filled && matches!(mark.tool, Tool::Rectangle | Tool::Ellipse) {
        let cx = f64::from(left + right) / 2.;
        let cy = f64::from(top + bottom) / 2.;
        let rx = f64::from(right - left).max(1.) / 2.;
        let ry = f64::from(bottom - top).max(1.) / 2.;
        for y in top..bottom {
            for x in left..right {
                if mark.tool == Tool::Rectangle
                    || ((f64::from(x) + 0.5 - cx) / rx).powi(2)
                        + ((f64::from(y) + 0.5 - cy) / ry).powi(2)
                        <= 1.
                {
                    blend_pixel(image, x, y, color);
                }
            }
        }
        return Ok(());
    }
    match mark.tool {
        Tool::Rectangle | Tool::Crop => {
            for (a, b) in [
                ((left, top), (right, top)),
                ((right, top), (right, bottom)),
                ((right, bottom), (left, bottom)),
                ((left, bottom), (left, top)),
            ] {
                line(image, a, b, color, stroke);
            }
        }
        Tool::Ellipse => {
            let cx = f64::from(left + right) / 2.0;
            let cy = f64::from(top + bottom) / 2.0;
            let rx = f64::from(right - left) / 2.0;
            let ry = f64::from(bottom - top) / 2.0;
            let steps = ((rx + ry) * 4.0).clamp(32.0, 16000.0) as usize;
            let mut previous = (right, i32::midpoint(top, bottom));
            for step in 1..=steps {
                let angle = step as f64 / steps as f64 * std::f64::consts::TAU;
                let next = (
                    (cx + rx * angle.cos()) as i32,
                    (cy + ry * angle.sin()) as i32,
                );
                line(image, previous, next, color, stroke);
                previous = next;
            }
        }
        Tool::Line | Tool::Arrow => {
            line(image, (a.x, a.y), (b.x, b.y), color, stroke);
            if mark.tool == Tool::Arrow {
                let angle = f64::from(b.y - a.y).atan2(f64::from(b.x - a.x));
                for offset in [-0.5, 0.5] {
                    let value = angle + offset;
                    let length = 12.0 + f64::from(stroke) * 3.0;
                    line(
                        image,
                        (b.x, b.y),
                        (
                            b.x - (value.cos() * length) as i32,
                            b.y - (value.sin() * length) as i32,
                        ),
                        color,
                        stroke,
                    );
                }
            }
        }
        Tool::Pen => {
            if mark.points.len() == 1 {
                line(image, (a.x, a.y), (a.x, a.y), color, stroke);
            }
            for pair in mark.points.windows(2) {
                line(
                    image,
                    (pair[0].x, pair[0].y),
                    (pair[1].x, pair[1].y),
                    color,
                    stroke,
                );
            }
        }
        Tool::Cover | Tool::Highlight => {
            let mut shade = color;
            if mark.tool == Tool::Highlight {
                shade[3] = (u16::from(color[3]) / 3) as u8;
            }
            for y in top..bottom {
                for x in left..right {
                    blend_pixel(image, x, y, shade);
                }
            }
        }
        Tool::Mosaic => {
            let block = (stroke * 4).clamp(2, 96) as i32;
            for y in (top..bottom).step_by(block as usize) {
                for x in (left..right).step_by(block as usize) {
                    let mut sum = [0_u64; 3];
                    let mut count = 0;
                    for py in y..(y + block).min(bottom) {
                        for px in x..(x + block).min(right) {
                            let offset = (py as usize * image.width as usize + px as usize) * 4;
                            for (channel, total) in sum.iter_mut().enumerate() {
                                *total += u64::from(image.pixels[offset + channel]);
                            }
                            count += 1;
                        }
                    }
                    let shade = [
                        (sum[0] / count) as u8,
                        (sum[1] / count) as u8,
                        (sum[2] / count) as u8,
                        255,
                    ];
                    for py in y..(y + block).min(bottom) {
                        for px in x..(x + block).min(right) {
                            image.pixel(px, py, shade);
                        }
                    }
                }
            }
        }
        Tool::Blur => {
            if right > left && bottom > top {
                let crop = image.crop(panzo_core::PhysicalRect {
                    x: left,
                    y: top,
                    width: (right - left) as u32,
                    height: (bottom - top) as u32,
                })?;
                let source = image::RgbaImage::from_raw(crop.width, crop.height, crop.pixels)
                    .ok_or("模糊区域无效")?;
                let blurred = image::imageops::blur(&source, (stroke as f32 * 2.0).max(0.5));
                for y in 0..blurred.height() {
                    for x in 0..blurred.width() {
                        image.pixel(left + x as i32, top + y as i32, blurred.get_pixel(x, y).0);
                    }
                }
            }
        }
        Tool::Text | Tool::Number => render_text(image, mark)?,
        Tool::Select => {}
    }
    Ok(())
}
fn render_text(image: &mut Raster, mark: &Mark) -> Result<(), String> {
    render_text_in_rect(image, mark, mark.bounds())
}
pub(super) fn render_text_in_rect(
    image: &mut Raster,
    mark: &Mark,
    rect: RECT,
) -> Result<(), String> {
    let width = (rect.right - rect.left)
        .min(image.width as i32 - rect.left)
        .max(1) as u32;
    let height = (rect.bottom - rect.top)
        .min(image.height as i32 - rect.top)
        .max(1) as u32;
    let coverage = crate::platform::capture_tools::image_text::rasterize(
        &mark.text,
        &mark.font,
        mark.font_size as i32,
        width,
        height,
    )?;
    for y in 0..height {
        for x in 0..width {
            let opacity =
                u32::from(coverage.pixels[(y as usize * width as usize + x as usize) * 4]);
            if opacity == 0 {
                continue;
            }
            let px = rect.left + x as i32;
            let py = rect.top + y as i32;
            if px < 0 || py < 0 || px >= image.width as i32 || py >= image.height as i32 {
                continue;
            }
            let mut color = mark.color;
            color[3] = ((opacity * u32::from(mark.color[3]) + 127) / 255) as u8;
            blend_pixel(image, px, py, color);
        }
    }
    Ok(())
}
/// Source-over on straight BGRA pixels, retaining a transparent document's alpha.
pub(super) fn blend_pixel(image: &mut Raster, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= image.width as i32 || y >= image.height as i32 || color[3] == 0 {
        return;
    }
    if color[3] == 255 {
        image.pixel(x, y, color);
        return;
    }
    let i = (y as usize * image.width as usize + x as usize) * 4;
    let sa = u32::from(color[3]);
    let da = u32::from(image.pixels[i + 3]);
    let alpha_numerator = sa * 255 + da * (255 - sa);
    for c in 0..3 {
        image.pixels[i + c] = ((u32::from(color[c]) * sa * 255
            + u32::from(image.pixels[i + c]) * da * (255 - sa)
            + alpha_numerator / 2)
            / alpha_numerator) as u8;
    }
    image.pixels[i + 3] = ((alpha_numerator + 127) / 255) as u8;
}
#[allow(clippy::many_single_char_names)] // Integer Bresenham rasterization, original-image coordinates.
fn line(image: &mut Raster, a: (i32, i32), b: (i32, i32), color: [u8; 4], width: u32) {
    let (mut x, mut y) = a;
    let dx = (b.0 - a.0).abs();
    let dy = -(b.1 - a.1).abs();
    let sx = if a.0 < b.0 { 1 } else { -1 };
    let sy = if a.1 < b.1 { 1 } else { -1 };
    let mut err = dx + dy;
    let radius = width as i32 / 2;
    loop {
        for oy in -radius..=radius {
            for ox in -radius..=radius {
                blend_pixel(image, x + ox, y + oy, color);
            }
        }
        if x == b.0 && y == b.1 {
            break;
        }
        let twice = 2 * err;
        if twice >= dy {
            err += dy;
            x += sx;
        }
        if twice <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raster() -> Raster {
        let mut pixels = Vec::new();
        for y in 0..12_u8 {
            for x in 0..20_u8 {
                pixels.extend_from_slice(&[x, y, 100, 255]);
            }
        }
        Raster {
            width: 20,
            height: 12,
            pixels,
        }
    }
    #[test]
    fn cropped_rotated_flipped_pixels_match_inverse_coordinate_mapping() {
        for rotation in 0..4 {
            for flip_x in [false, true] {
                for flip_y in [false, true] {
                    let mut document = Document::new(raster());
                    document.state = ImageState {
                        crop: Some([3, 2, 12, 8]),
                        rotation,
                        flip_x,
                        flip_y,
                        ..Default::default()
                    };
                    let output = document.render(None).unwrap();
                    for y in 0..output.height {
                        for x in 0..output.width {
                            let point = document.source_point(POINT {
                                x: x as i32,
                                y: y as i32,
                            });
                            let offset = (y as usize * output.width as usize + x as usize) * 4;
                            assert_eq!(
                                &output.pixels[offset..offset + 2],
                                &[point.x as u8, point.y as u8]
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn object_edits_and_framing_roundtrip_with_undo_and_redo() {
        let mut document = Document::new(raster());
        document.commit(Mark {
            tool: Tool::Cover,
            points: vec![POINT { x: 2, y: 2 }, POINT { x: 8, y: 8 }],
            color: [0, 0, 0, 255],
            ..Default::default()
        });
        let original = document.render(None).unwrap();
        document.checkpoint();
        document.move_mark(0, 4, 0);
        assert_eq!(document.hit(POINT { x: 10, y: 5 }), Some(0));
        document.undo();
        assert_eq!(document.render(None).unwrap().pixels, original.pixels);
        document.redo();
        document.frame(POINT { x: 2, y: 2 }, POINT { x: 18, y: 10 });
        document.transform(40);
        let state: ImageState =
            serde_json::from_slice(&serde_json::to_vec(&document.state).unwrap()).unwrap();
        let restored = Document::from_state(raster(), state).unwrap();
        assert_eq!(
            restored.render(None).unwrap().pixels,
            document.render(None).unwrap().pixels
        );
        assert_eq!(restored.size(), (8, 16));
        document.delete(0);
        document.undo();
        assert_eq!(document.state.marks.len(), 1);
    }
    #[test]
    fn exclusive_crop_preserves_every_edge_after_all_transforms() {
        for rotation in 0..4 {
            for flip_x in [false, true] {
                for flip_y in [false, true] {
                    let mut d = Document::new(raster());
                    d.state.rotation = rotation;
                    d.state.flip_x = flip_x;
                    d.state.flip_y = flip_y;
                    let (w, h) = d.size();
                    let before = d.render(None).unwrap();
                    d.crop_output([0, 0, w, h]);
                    assert_eq!(d.revision, 0, "full-image crop must be a no-op");
                    assert_eq!(d.render(None).unwrap().pixels, before.pixels);
                    d.crop_output([1, 1, w, h]);
                    assert_eq!(d.size(), (w - 1, h - 1));
                    d.undo();
                    assert_eq!(d.size(), (w, h));
                    for y in 0..h {
                        for x in 0..w {
                            let p = POINT {
                                x: x as i32,
                                y: y as i32,
                            };
                            assert_eq!(d.display_point(d.source_point(p)), p);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn selection_and_cancelled_preview_preserve_redo_and_revision() {
        let mut d = Document::new(raster());
        let mark = Mark {
            points: vec![POINT { x: 1, y: 1 }, POINT { x: 5, y: 5 }],
            ..Default::default()
        };
        d.commit(mark.clone());
        d.commit(mark.clone());
        d.undo();
        let revision = d.revision;
        let index = d.hit(POINT { x: 2, y: 2 }).unwrap();
        let mut preview = d.state.marks[index].clone();
        preview.points[0].x += 2;
        drop(preview);
        assert!(!d.replace_mark(index, mark));
        assert_eq!(d.revision, revision);
        d.redo();
        assert_eq!(d.state.marks.len(), 2);
    }
    #[test]
    fn object_gesture_commits_one_undo_transaction() {
        let mut d = Document::new(raster());
        let mark = Mark {
            points: vec![POINT { x: 2, y: 2 }, POINT { x: 6, y: 6 }],
            ..Default::default()
        };
        d.commit(mark.clone());
        let mut preview = mark.clone();
        for dx in 1..8 {
            preview = mark.translated(dx, 0, (20, 12));
        }
        let revision = d.revision;
        assert!(d.replace_mark(0, preview));
        assert_eq!(d.revision, revision + 1);
        d.undo();
        assert!(d.state.marks[0].same(&mark));
        d.redo();
        assert_eq!(d.state.marks[0].points[0].x, 9);
    }
    #[test]
    fn corrupted_mark_coordinates_and_excessive_points_are_rejected() {
        let mut state = ImageState::default();
        state.marks.push(Mark {
            points: vec![POINT { x: i32::MAX, y: 1 }],
            ..Default::default()
        });
        assert!(Document::from_state(raster(), state).is_err());
    }
    #[test]
    fn edge_transform_roundtrips_outer_boundary_for_overlay_bitmaps() {
        for rotation in 0..4 {
            for flip_x in [false, true] {
                for flip_y in [false, true] {
                    let mut d = Document::new(raster());
                    d.state = ImageState {
                        crop: Some([2, 3, 14, 7]),
                        rotation,
                        flip_x,
                        flip_y,
                        ..Default::default()
                    };
                    let (w, h) = d.size();
                    for p in [
                        POINT { x: 0, y: 0 },
                        POINT { x: w as i32, y: 0 },
                        POINT { x: 0, y: h as i32 },
                        POINT {
                            x: w as i32,
                            y: h as i32,
                        },
                    ] {
                        assert_eq!(d.display_edge(d.source_edge(p)), p);
                    }
                }
            }
        }
    }
    #[test]
    fn annotation_opacity_composites_over_image_instead_of_punching_holes() {
        let mut image = Raster {
            width: 1,
            height: 1,
            pixels: vec![200, 100, 0, 255],
        };
        blend_pixel(&mut image, 0, 0, [0, 100, 200, 128]);
        assert_eq!(image.pixels, vec![100, 100, 100, 255]);
        image.pixels = vec![0, 0, 0, 0];
        blend_pixel(&mut image, 0, 0, [20, 40, 60, 128]);
        assert_eq!(image.pixels, vec![20, 40, 60, 128]);
    }
    #[test]
    fn every_annotation_tool_changes_pixels_and_preserves_dimensions() {
        for tool in [
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Line,
            Tool::Arrow,
            Tool::Pen,
            Tool::Highlight,
            Tool::Text,
            Tool::Number,
            Tool::Mosaic,
            Tool::Blur,
            Tool::Cover,
        ] {
            let source = Raster {
                width: 180,
                height: 100,
                pixels: (0..180 * 100)
                    .flat_map(|index| [(index % 255) as u8, 150, 200, 255])
                    .collect(),
            };
            let original = source.pixels.clone();
            let mut document = Document::new(source);
            document.commit(Mark {
                tool,
                points: vec![POINT { x: 15, y: 15 }, POINT { x: 150, y: 80 }],
                text: if tool == Tool::Number {
                    "1".into()
                } else {
                    "中文注释\n第二行".into()
                },
                ..Default::default()
            });
            let image = document.render(None).unwrap();
            assert_eq!((image.width, image.height), (180, 100));
            assert_ne!(image.pixels, original);
        }
    }
}
