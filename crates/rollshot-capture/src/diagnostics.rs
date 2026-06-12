#[allow(dead_code)]
pub(crate) const TARGET_CAPTURE: &str = "rollshot::capture";
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) const TARGET_LINUX_PORTAL: &str = "rollshot::capture::linux::portal";
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) const TARGET_LINUX_PIPEWIRE: &str = "rollshot::capture::linux::pipewire";
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) const TARGET_LINUX_KWIN: &str = "rollshot::capture::linux::kwin";
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) const TARGET_MACOS_SCK: &str = "rollshot::capture::macos::sck";
