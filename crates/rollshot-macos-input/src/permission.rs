//! Input Monitoring permission API (filled in Task 6).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMonitoringStatus {
    Granted,
    Denied,
    NotDetermined,
}

pub fn input_monitoring_status() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

pub fn request_input_monitoring() -> InputMonitoringStatus {
    InputMonitoringStatus::Denied
}

pub fn open_input_monitoring_settings() {}
