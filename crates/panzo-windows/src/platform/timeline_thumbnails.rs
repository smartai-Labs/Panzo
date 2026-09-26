//! Visible-range thumbnail service. Never creates a proxy or blocks the window thread.
use crate::platform::{
    decode_worker::{DecodePlan, DecodeWorker},
    media_decoder::DecodedBgraFrame,
    preview_proxy::PreviewProxy,
};
use panzo_core::TimeTick;
use std::{collections::VecDeque, sync::Arc};

pub const MAX_VISIBLE_THUMBNAILS: usize = 128;

#[derive(Default)]
pub struct TimelineThumbnails {
    worker: Option<DecodeWorker>,
    proxy: Option<Arc<PreviewProxy>>,
    cache: VecDeque<(TimeTick, Arc<DecodedBgraFrame>)>,
    pending: Option<(u64, TimeTick)>,
}
impl TimelineThumbnails {
    // Fixed 160-pixel thumbnails; enough entries to retain the compact track's visible tiles.
    const CAPACITY: usize = MAX_VISIBLE_THUMBNAILS;
    pub fn poll(
        &mut self,
        ready: Option<Arc<PreviewProxy>>,
        visible: &[TimeTick],
        idle: bool,
    ) -> bool {
        if self.worker.is_none()
            && let Some(proxy) = ready
            && let Ok(worker) = DecodeWorker::thumbnails(&proxy.path)
        {
            self.worker = Some(worker);
            self.proxy = Some(proxy);
        }
        let (Some(worker), Some(proxy)) = (&self.worker, &self.proxy) else {
            return false;
        };
        let mut changed = false;
        if let Some(result) = worker.take_latest()
            && let Some((generation, key)) = self.pending.take()
            && generation == result.generation
            && let Ok(frame) = result.frame
        {
            self.cache.push_back((key, frame));
            while self.cache.len() > Self::CAPACITY {
                self.cache.pop_front();
            }
            changed = true;
        }
        // Media playback and gestures always win over decoration.
        if idle
            && self.pending.is_none()
            && let Some(key) = visible
                .iter()
                .map(|tick| proxy.proxy_time(*tick))
                .find(|key| !self.cache.iter().any(|(cached, _)| cached == key))
            && let Ok(generation) = worker.submit(DecodePlan {
                project_tick: key,
                target_pts: key,
                fallback_pts: vec![TimeTick::ZERO],
                exact: false,
                prefetch_until: None,
            })
        {
            self.pending = Some((generation, key));
        }
        changed
    }

    pub fn get(&self, source: TimeTick) -> Option<&DecodedBgraFrame> {
        let key = self.proxy.as_ref()?.proxy_time(source);
        self.cache
            .iter()
            .find(|(cached, _)| *cached == key)
            .map(|(_, frame)| frame.as_ref())
    }
}
