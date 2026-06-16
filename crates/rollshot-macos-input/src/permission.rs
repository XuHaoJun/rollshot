//! Input Monitoring (TCC `kTCCServiceListenEvent`) permission operations. A
//! listen-only `CGEventTap` needs Input Monitoring only — never Accessibility
//! or PostEvent, which this crate deliberately does not request (spec §macOS).

#[cfg(target_os = "macos")]
const TARGET: &str = "rollshot::action::macos_input";

/// Tri-state Input Monitoring permission, mapped from CoreGraphics' boolean
/// preflight plus a request attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMonitoringStatus {
    Granted,
    Denied,
    NotDetermined,
}

#[cfg(not(target_os = "macos"))]
pub fn input_monitoring_status() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn request_input_monitoring() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn open_input_monitoring_settings() {}

#[cfg(target_os = "macos")]
pub fn input_monitoring_status() -> InputMonitoringStatus {
    // CGPreflightListenEventAccess takes no arguments, returns a Boolean, and is
    // a safe binding in objc2-core-graphics.
    let granted = objc2_core_graphics::CGPreflightListenEventAccess();
    if granted {
        InputMonitoringStatus::Granted
    } else {
        // Preflight cannot distinguish "denied" from "not yet asked"; callers
        // treat both as not-granted and may call `request_input_monitoring`.
        InputMonitoringStatus::NotDetermined
    }
}

#[cfg(target_os = "macos")]
pub fn request_input_monitoring() -> InputMonitoringStatus {
    // CGRequestListenEventAccess prompts (once) and returns whether access is
    // now granted; it is a safe binding in objc2-core-graphics.
    let granted = objc2_core_graphics::CGRequestListenEventAccess();
    if granted {
        InputMonitoringStatus::Granted
    } else {
        InputMonitoringStatus::Denied
    }
}

#[cfg(target_os = "macos")]
pub fn open_input_monitoring_settings() {
    // Open the Input Monitoring pane via the standard System Settings URL.
    // Use `open(1)` to avoid pulling in AppKit here.
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    if let Err(err) = std::process::Command::new("open").arg(url).spawn() {
        tracing::warn!(target: TARGET, error = %err, "failed to open Input Monitoring settings");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_status_is_denied_and_request_does_not_panic() {
        assert_eq!(input_monitoring_status(), InputMonitoringStatus::Denied);
        assert_eq!(request_input_monitoring(), InputMonitoringStatus::Denied);
        open_input_monitoring_settings(); // must be a no-op, not a panic
    }

    #[test]
    fn status_enum_distinguishes_three_states() {
        // Compile-time proof the three TCC states are representable and that
        // Input Monitoring is modeled separately from Accessibility (which this
        // crate never touches).
        let all = [
            InputMonitoringStatus::Granted,
            InputMonitoringStatus::Denied,
            InputMonitoringStatus::NotDetermined,
        ];
        assert_eq!(all.len(), 3);
    }
}
