use crate::{CaptureResult, OverlayConfig, OverlayError};

pub(crate) fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    let _ = config;
    Err(OverlayError::Overlay(
        "macOS iced overlay runner is scaffolded but not wired to capture yet".to_string(),
    ))
}
