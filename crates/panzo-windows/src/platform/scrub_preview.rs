use crate::platform::decode_worker::{DecodePlan, DecodeResult, DecodeWorker};
use crate::platform::preview::ScrubDecodePlan;
use std::path::Path;
use thiserror::Error;

pub struct ScrubPreviewWorker(DecodeWorker);

impl ScrubPreviewWorker {
    pub fn start(media_path: &Path) -> Result<Self, ScrubPreviewWorkerError> {
        DecodeWorker::scrub(media_path)
            .map(Self)
            .map_err(ScrubPreviewWorkerError)
    }

    pub fn submit(&mut self, plan: ScrubDecodePlan) -> Result<u64, ScrubPreviewWorkerError> {
        self.0
            .submit(DecodePlan {
                project_tick: plan.project_tick,
                target_pts: plan.target_pts,
                fallback_pts: plan.fallback_pts.into_iter().collect(),
                exact: false,
                prefetch_until: None,
            })
            .map_err(ScrubPreviewWorkerError)
    }

    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn pause_preparation(&self, paused: bool) {
        self.0.pause_proxy_preparation(paused);
    }

    pub fn proxy_status(&self) -> Option<String> {
        self.0.proxy_status()
    }

    pub fn ready_proxy(
        &self,
    ) -> Option<std::sync::Arc<crate::platform::preview_proxy::PreviewProxy>> {
        self.0.ready_proxy()
    }

    pub fn set_frame_ready_callback(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.0.set_completion_callback(callback);
    }

    pub fn take_latest(&self) -> Option<DecodeResult> {
        self.0.take_latest()
    }
}

#[derive(Debug, Error)]
#[error("scrub preview media service: {0}")]
pub struct ScrubPreviewWorkerError(String);
