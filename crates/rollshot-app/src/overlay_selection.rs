use rollshot_capture::OverlayMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRunner {
    Iced,
    Tauri,
}

pub fn resolve_overlay_runner(os: &str, mode: OverlayMode) -> OverlayRunner {
    match (os, mode) {
        (_, OverlayMode::Iced) => OverlayRunner::Iced,
        (_, OverlayMode::Tauri) => OverlayRunner::Tauri,
        ("linux", OverlayMode::Auto) => OverlayRunner::Iced,
        ("macos", OverlayMode::Auto) => OverlayRunner::Tauri,
        (_, OverlayMode::Auto) => OverlayRunner::Tauri,
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_overlay_runner, OverlayRunner};
    use rollshot_capture::OverlayMode;

    #[test]
    fn linux_auto_uses_iced_overlay() {
        assert_eq!(
            resolve_overlay_runner("linux", OverlayMode::Auto),
            OverlayRunner::Iced
        );
    }

    #[test]
    fn macos_auto_keeps_tauri_fallback() {
        assert_eq!(
            resolve_overlay_runner("macos", OverlayMode::Auto),
            OverlayRunner::Tauri
        );
    }

    #[test]
    fn macos_iced_is_explicit_opt_in() {
        assert_eq!(
            resolve_overlay_runner("macos", OverlayMode::Iced),
            OverlayRunner::Iced
        );
    }
}
