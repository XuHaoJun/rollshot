use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktopProfile {
    Kde,
    Gnome,
    Wlroots,
    Hyprland,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPortalQuirk {
    KdeMayReturnMultipleStreams,
    PortalRegionPickerLikelyAvailable,
    RegionPickerMayReturnVideoCrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTypes {
    pub monitor: bool,
    pub window: bool,
    pub virtual_source: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorModes {
    pub hidden: bool,
    pub embedded: bool,
    pub metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxPortalCapabilities {
    pub desktop: String,
    pub session_type: String,
    pub portal_version: Option<u32>,
    pub source_types: SourceTypes,
    pub cursor_modes: CursorModes,
    pub profile: LinuxDesktopProfile,
    pub quirks: Vec<LinuxPortalQuirk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalCursorMode {
    Hidden,
    Embedded,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSelectSourcesOptions {
    pub monitor: bool,
    pub window: bool,
    pub multiple: bool,
    pub cursor_mode: PortalCursorMode,
    pub persist: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalStreamInfo {
    pub node_id: u32,
}

#[derive(Debug, Clone)]
pub struct PortalStartResult {
    pub node_id: u32,
    pub capabilities: LinuxPortalCapabilities,
}

pub struct PortalClient;

impl PortalClient {
    pub fn new() -> Self {
        Self
    }

    pub fn probe(&self) -> CaptureProbe {
        self.probe_inner()
    }

    pub fn start(&self, _options: CaptureOptions) -> Result<PortalStartResult, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal",
        })
    }
}

pub fn classify_desktop(desktop: &str) -> LinuxDesktopProfile {
    let lower = desktop.to_ascii_lowercase();
    if lower.contains("kde") || lower.contains("plasma") {
        LinuxDesktopProfile::Kde
    } else if lower.contains("gnome") {
        LinuxDesktopProfile::Gnome
    } else if lower.contains("hyprland") {
        LinuxDesktopProfile::Hyprland
    } else if lower.contains("sway") || lower.contains("wlroots") {
        LinuxDesktopProfile::Wlroots
    } else {
        LinuxDesktopProfile::Unknown
    }
}

pub fn quirks_for_profile(profile: LinuxDesktopProfile) -> Vec<LinuxPortalQuirk> {
    match profile {
        LinuxDesktopProfile::Kde => vec![
            LinuxPortalQuirk::KdeMayReturnMultipleStreams,
            LinuxPortalQuirk::PortalRegionPickerLikelyAvailable,
            LinuxPortalQuirk::RegionPickerMayReturnVideoCrop,
        ],
        _ => Vec::new(),
    }
}

pub fn choose_cursor_mode(cursors: CursorModes, show_cursor: bool) -> PortalCursorMode {
    if cursors.metadata {
        PortalCursorMode::Metadata
    } else if show_cursor && cursors.embedded {
        PortalCursorMode::Embedded
    } else {
        PortalCursorMode::Hidden
    }
}

pub fn select_sources_options(
    cursors: CursorModes,
    show_cursor: bool,
) -> PortalSelectSourcesOptions {
    PortalSelectSourcesOptions {
        monitor: true,
        window: true,
        multiple: false,
        cursor_mode: choose_cursor_mode(cursors, show_cursor),
        persist: false,
    }
}

pub fn choose_stream(streams: &[PortalStreamInfo]) -> Result<PortalStreamInfo, CaptureError> {
    streams
        .last()
        .copied()
        .ok_or_else(|| CaptureError::Backend(anyhow::anyhow!("portal returned no streams")))
}

fn probe_from_env(session_type: Option<String>, desktop: Option<String>) -> CaptureProbe {
    let session_type = session_type.unwrap_or_default();
    let desktop = desktop.unwrap_or_default();
    let is_wayland = session_type == "wayland";
    CaptureProbe {
        backend: "linux-portal",
        available: false,
        message: if is_wayland {
            "linux-portal probe needs ScreenCast portal diagnostics".to_string()
        } else {
            "linux-portal requires a Wayland session".to_string()
        },
        details: vec![
            ("os".to_string(), "linux".to_string()),
            ("XDG_SESSION_TYPE".to_string(), session_type),
            ("XDG_CURRENT_DESKTOP".to_string(), desktop),
        ],
    }
}

pub(super) trait ProbeSource {
    fn screencast_version(&self) -> Result<u32, String>;
    fn available_source_types(&self) -> Result<SourceTypes, String>;
    fn available_cursor_modes(&self) -> Result<CursorModes, String>;
    fn pipewire_version(&self) -> Result<String, String>;
}

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

fn format_source_types(s: SourceTypes) -> String {
    let mut parts = Vec::new();
    if s.monitor {
        parts.push("monitor");
    }
    if s.window {
        parts.push("window");
    }
    if s.virtual_source {
        parts.push("virtual");
    }
    parts.join("|")
}

fn format_cursor_modes(c: CursorModes) -> String {
    let mut parts = Vec::new();
    if c.hidden {
        parts.push("hidden");
    }
    if c.embedded {
        parts.push("embedded");
    }
    if c.metadata {
        parts.push("metadata");
    }
    parts.join("|")
}

fn format_quirks(quirks: &[LinuxPortalQuirk]) -> String {
    quirks
        .iter()
        .map(|q| format!("{q:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn call_with_timeout<T>(
    name: &str,
    timeout: std::time::Duration,
    details: &mut Vec<(String, String)>,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Option<T>
where
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(val)) => Some(val),
        Ok(Err(err)) => {
            details.push(("probe_error".to_string(), format!("{name}: {err}")));
            None
        }
        Err(_timeout) => {
            details.push((
                "probe_error".to_string(),
                format!("{name}: timed out after {}ms", timeout.as_millis()),
            ));
            None
        }
    }
}

fn build_probe_from_source<S: ProbeSource + Send + Sync + 'static>(
    session_type: String,
    desktop: String,
    source: S,
    timeout: std::time::Duration,
) -> CaptureProbe {
    let profile = classify_desktop(&desktop);
    let quirks = quirks_for_profile(profile);
    let mut details = vec![
        ("os".to_string(), "linux".to_string()),
        ("XDG_SESSION_TYPE".to_string(), session_type.clone()),
        ("XDG_CURRENT_DESKTOP".to_string(), desktop.clone()),
        (
            "desktop_profile".to_string(),
            format!("{profile:?}").to_ascii_lowercase(),
        ),
    ];

    let arc = std::sync::Arc::new(source);

    let arc_clone = std::sync::Arc::clone(&arc);
    let version = call_with_timeout("screencast_version", timeout, &mut details, move || {
        arc_clone.screencast_version()
    });
    let arc_clone = std::sync::Arc::clone(&arc);
    let source_types = call_with_timeout("available_source_types", timeout, &mut details, move || {
        arc_clone.available_source_types()
    });
    let arc_clone = std::sync::Arc::clone(&arc);
    let cursor_modes = call_with_timeout("available_cursor_modes", timeout, &mut details, move || {
        arc_clone.available_cursor_modes()
    });
    let arc_clone = std::sync::Arc::clone(&arc);
    let pipewire = call_with_timeout("pipewire_version", timeout, &mut details, move || {
        arc_clone.pipewire_version()
    });

    let has_source = source_types.map(|s| s.monitor || s.window).unwrap_or(false);
    let has_pipewire = pipewire.is_some();

    details.push((
        "screencast_version".to_string(),
        version
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unavailable".to_string()),
    ));
    details.push((
        "available_source_types".to_string(),
        source_types
            .map(format_source_types)
            .unwrap_or_else(|| "unavailable".to_string()),
    ));
    details.push((
        "available_cursor_modes".to_string(),
        cursor_modes
            .map(format_cursor_modes)
            .unwrap_or_else(|| "unavailable".to_string()),
    ));
    details.push((
        "pipewire_library_version".to_string(),
        pipewire.unwrap_or_else(|| "unavailable".to_string()),
    ));
    details.push(("quirks".to_string(), format_quirks(&quirks)));

    let available = session_type == "wayland"
        && has_source
        && has_pipewire
        && !details.iter().any(|(k, _)| k == "probe_error");

    CaptureProbe {
        backend: "linux-portal",
        available,
        message: if available {
            "linux-portal ScreenCast and PipeWire diagnostics look ready".to_string()
        } else if session_type != "wayland" {
            "linux-portal requires a Wayland session".to_string()
        } else {
            "linux-portal ScreenCast or PipeWire diagnostics are incomplete".to_string()
        },
        details,
    }
}

#[cfg(not(test))]
fn get_pipewire_version() -> Result<String, String> {
    std::process::Command::new("pkg-config")
        .args(["--modversion", "libpipewire-0.3"])
        .output()
        .map_err(|e| format!("pkg-config failed: {e}"))
        .and_then(|output| {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                Err("pkg-config could not find libpipewire-0.3".to_string())
            }
        })
}

#[cfg(test)]
fn get_pipewire_version() -> Result<String, String> {
    Ok("1.0.0".to_string())
}

#[cfg(not(test))]
struct AshpdProbeSource;

#[cfg(not(test))]
impl AshpdProbeSource {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
}

#[cfg(not(test))]
impl ProbeSource for AshpdProbeSource {
    fn screencast_version(&self) -> Result<u32, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let proxy = ashpd::desktop::screencast::Screencast::new()
                .await
                .map_err(|e| e.to_string())?;
            let _ = proxy.available_source_types().await.map_err(|e| e.to_string())?;
            Ok(4)
        })
    }

    fn available_source_types(&self) -> Result<SourceTypes, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let proxy = ashpd::desktop::screencast::Screencast::new()
                .await
                .map_err(|e| e.to_string())?;
            let sources = proxy.available_source_types().await.map_err(|e| e.to_string())?;
            Ok(SourceTypes {
                monitor: sources.contains(ashpd::desktop::screencast::SourceType::Monitor),
                window: sources.contains(ashpd::desktop::screencast::SourceType::Window),
                virtual_source: sources
                    .contains(ashpd::desktop::screencast::SourceType::Virtual),
            })
        })
    }

    fn available_cursor_modes(&self) -> Result<CursorModes, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let proxy = ashpd::desktop::screencast::Screencast::new()
                .await
                .map_err(|e| e.to_string())?;
            let cursors = proxy.available_cursor_modes().await.map_err(|e| e.to_string())?;
            Ok(CursorModes {
                hidden: cursors.contains(ashpd::desktop::screencast::CursorMode::Hidden),
                embedded: cursors.contains(ashpd::desktop::screencast::CursorMode::Embedded),
                metadata: cursors.contains(ashpd::desktop::screencast::CursorMode::Metadata),
            })
        })
    }

    fn pipewire_version(&self) -> Result<String, String> {
        get_pipewire_version()
    }
}

#[cfg(not(test))]
impl PortalClient {
    fn probe_inner(&self) -> CaptureProbe {
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

        match AshpdProbeSource::new() {
            Ok(source) => build_probe_from_source(session_type, desktop, source, PROBE_TIMEOUT),
            Err(err) => {
                let mut details = vec![
                    ("os".to_string(), "linux".to_string()),
                    ("XDG_SESSION_TYPE".to_string(), session_type),
                    ("XDG_CURRENT_DESKTOP".to_string(), desktop),
                ];
                let pipewire_version = get_pipewire_version().unwrap_or_else(|_| "unavailable".to_string());
                details.push((
                    "pipewire_library_version".to_string(),
                    pipewire_version,
                ));
                details.push(("probe_error".to_string(), err));
                CaptureProbe {
                    backend: "linux-portal",
                    available: false,
                    message: "linux-portal ScreenCast or PipeWire diagnostics are incomplete"
                        .to_string(),
                    details,
                }
            }
        }
    }
}

#[cfg(test)]
impl PortalClient {
    fn probe_inner(&self) -> CaptureProbe {
        probe_from_env(
            std::env::var("XDG_SESSION_TYPE").ok(),
            std::env::var("XDG_CURRENT_DESKTOP").ok(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWaylandProbeSource {
        screencast_version: u32,
        source_types: SourceTypes,
        cursor_modes: CursorModes,
        pipewire_version: String,
    }

    impl ProbeSource for FakeWaylandProbeSource {
        fn screencast_version(&self) -> Result<u32, String> {
            Ok(self.screencast_version)
        }
        fn available_source_types(&self) -> Result<SourceTypes, String> {
            Ok(self.source_types)
        }
        fn available_cursor_modes(&self) -> Result<CursorModes, String> {
            Ok(self.cursor_modes)
        }
        fn pipewire_version(&self) -> Result<String, String> {
            Ok(self.pipewire_version.clone())
        }
    }

    struct FakeX11ProbeSource;

    impl ProbeSource for FakeX11ProbeSource {
        fn screencast_version(&self) -> Result<u32, String> {
            Err("no portal on X11".to_string())
        }
        fn available_source_types(&self) -> Result<SourceTypes, String> {
            Err("no portal on X11".to_string())
        }
        fn available_cursor_modes(&self) -> Result<CursorModes, String> {
            Err("no portal on X11".to_string())
        }
        fn pipewire_version(&self) -> Result<String, String> {
            Ok("1.0.0".to_string())
        }
    }

    struct FakeSleepingProbeSource {
        sleep_duration: std::time::Duration,
    }

    impl ProbeSource for FakeSleepingProbeSource {
        fn screencast_version(&self) -> Result<u32, String> {
            std::thread::sleep(self.sleep_duration);
            Ok(4)
        }
        fn available_source_types(&self) -> Result<SourceTypes, String> {
            Ok(SourceTypes {
                monitor: true,
                window: false,
                virtual_source: false,
            })
        }
        fn available_cursor_modes(&self) -> Result<CursorModes, String> {
            Ok(CursorModes {
                hidden: true,
                embedded: false,
                metadata: false,
            })
        }
        fn pipewire_version(&self) -> Result<String, String> {
            Ok("1.0.0".to_string())
        }
    }

    struct FakeNoMonitorProbeSource;

    impl ProbeSource for FakeNoMonitorProbeSource {
        fn screencast_version(&self) -> Result<u32, String> {
            Ok(4)
        }
        fn available_source_types(&self) -> Result<SourceTypes, String> {
            Ok(SourceTypes {
                monitor: false,
                window: false,
                virtual_source: true,
            })
        }
        fn available_cursor_modes(&self) -> Result<CursorModes, String> {
            Ok(CursorModes {
                hidden: true,
                embedded: false,
                metadata: false,
            })
        }
        fn pipewire_version(&self) -> Result<String, String> {
            Ok("1.0.0".to_string())
        }
    }

    #[test]
    fn classify_desktop_kde() {
        assert_eq!(classify_desktop("KDE"), LinuxDesktopProfile::Kde);
        assert_eq!(classify_desktop("kde"), LinuxDesktopProfile::Kde);
        assert_eq!(classify_desktop("KDE Plasma"), LinuxDesktopProfile::Kde);
        assert_eq!(classify_desktop("plasma"), LinuxDesktopProfile::Kde);
    }

    #[test]
    fn classify_desktop_gnome() {
        assert_eq!(classify_desktop("GNOME"), LinuxDesktopProfile::Gnome);
        assert_eq!(classify_desktop("gnome"), LinuxDesktopProfile::Gnome);
        assert_eq!(classify_desktop("ubuntu:GNOME"), LinuxDesktopProfile::Gnome);
    }

    #[test]
    fn classify_desktop_hyprland() {
        assert_eq!(classify_desktop("Hyprland"), LinuxDesktopProfile::Hyprland);
        assert_eq!(classify_desktop("hyprland"), LinuxDesktopProfile::Hyprland);
    }

    #[test]
    fn classify_desktop_wlroots() {
        assert_eq!(classify_desktop("sway"), LinuxDesktopProfile::Wlroots);
        assert_eq!(classify_desktop("Sway"), LinuxDesktopProfile::Wlroots);
        assert_eq!(classify_desktop("wlroots"), LinuxDesktopProfile::Wlroots);
    }

    #[test]
    fn classify_desktop_unknown() {
        assert_eq!(classify_desktop(""), LinuxDesktopProfile::Unknown);
        assert_eq!(classify_desktop("xfce"), LinuxDesktopProfile::Unknown);
        assert_eq!(classify_desktop("i3"), LinuxDesktopProfile::Unknown);
    }

    #[test]
    fn quirks_for_kde() {
        let quirks = quirks_for_profile(LinuxDesktopProfile::Kde);
        assert_eq!(quirks.len(), 3);
        assert!(quirks.contains(&LinuxPortalQuirk::KdeMayReturnMultipleStreams));
        assert!(quirks.contains(&LinuxPortalQuirk::PortalRegionPickerLikelyAvailable));
        assert!(quirks.contains(&LinuxPortalQuirk::RegionPickerMayReturnVideoCrop));
    }

    #[test]
    fn quirks_for_non_kde_are_empty() {
        assert!(quirks_for_profile(LinuxDesktopProfile::Gnome).is_empty());
        assert!(quirks_for_profile(LinuxDesktopProfile::Wlroots).is_empty());
        assert!(quirks_for_profile(LinuxDesktopProfile::Hyprland).is_empty());
        assert!(quirks_for_profile(LinuxDesktopProfile::Unknown).is_empty());
    }

    #[test]
    fn choose_cursor_mode_metadata_preferred() {
        let cursors = CursorModes {
            hidden: true,
            embedded: true,
            metadata: true,
        };
        assert_eq!(
            choose_cursor_mode(cursors, true),
            PortalCursorMode::Metadata
        );
        assert_eq!(
            choose_cursor_mode(cursors, false),
            PortalCursorMode::Metadata
        );
    }

    #[test]
    fn choose_cursor_mode_embedded_when_show_cursor() {
        let cursors = CursorModes {
            hidden: true,
            embedded: true,
            metadata: false,
        };
        assert_eq!(
            choose_cursor_mode(cursors, true),
            PortalCursorMode::Embedded
        );
    }

    #[test]
    fn choose_cursor_mode_hidden_when_no_cursor() {
        let cursors = CursorModes {
            hidden: true,
            embedded: true,
            metadata: false,
        };
        assert_eq!(
            choose_cursor_mode(cursors, false),
            PortalCursorMode::Hidden
        );
    }

    #[test]
    fn select_sources_options_defaults() {
        let cursors = CursorModes {
            hidden: true,
            embedded: false,
            metadata: false,
        };
        let opts = select_sources_options(cursors, false);
        assert!(opts.monitor);
        assert!(opts.window);
        assert!(!opts.multiple);
        assert!(!opts.persist);
        assert_eq!(opts.cursor_mode, PortalCursorMode::Hidden);
    }

    #[test]
    fn choose_stream_empty_returns_error() {
        let result = choose_stream(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn choose_stream_single_returns_last() {
        let streams = vec![PortalStreamInfo { node_id: 42 }];
        let result = choose_stream(&streams).unwrap();
        assert_eq!(result.node_id, 42);
    }

    #[test]
    fn choose_stream_multiple_returns_last() {
        let streams = vec![
            PortalStreamInfo { node_id: 1 },
            PortalStreamInfo { node_id: 2 },
            PortalStreamInfo { node_id: 3 },
        ];
        let result = choose_stream(&streams).unwrap();
        assert_eq!(result.node_id, 3);
    }

    #[test]
    fn probe_non_wayland_message() {
        let probe = probe_from_env(Some("x11".to_string()), Some("GNOME".to_string()));
        assert_eq!(probe.backend, "linux-portal");
        assert!(!probe.available);
        assert!(probe.message.contains("Wayland session"));
    }

    #[test]
    fn probe_wayland_message() {
        let probe = probe_from_env(Some("wayland".to_string()), Some("KDE".to_string()));
        assert_eq!(probe.backend, "linux-portal");
        assert!(!probe.available);
        assert!(probe.message.contains("ScreenCast portal"));
    }

    #[test]
    fn probe_includes_env_details() {
        let probe = probe_from_env(Some("wayland".to_string()), Some("GNOME".to_string()));
        let details: std::collections::HashMap<_, _> = probe.details.into_iter().collect();
        assert_eq!(details.get("os").unwrap(), "linux");
        assert_eq!(details.get("XDG_SESSION_TYPE").unwrap(), "wayland");
        assert_eq!(details.get("XDG_CURRENT_DESKTOP").unwrap(), "GNOME");
    }

    #[test]
    fn probe_wayland_monitor_pipewire_available() {
        let source = FakeWaylandProbeSource {
            screencast_version: 4,
            source_types: SourceTypes {
                monitor: true,
                window: false,
                virtual_source: false,
            },
            cursor_modes: CursorModes {
                hidden: true,
                embedded: true,
                metadata: false,
            },
            pipewire_version: "1.0.0".to_string(),
        };
        let probe = build_probe_from_source(
            "wayland".to_string(),
            "GNOME".to_string(),
            source,
            std::time::Duration::from_millis(500),
        );
        assert!(probe.available);
        assert!(probe.message.contains("look ready"));
    }

    #[test]
    fn probe_x11_returns_unavailable() {
        let source = FakeX11ProbeSource;
        let probe = build_probe_from_source(
            "x11".to_string(),
            "GNOME".to_string(),
            source,
            std::time::Duration::from_millis(500),
        );
        assert!(!probe.available);
        assert!(probe.message.contains("Wayland session"));
        let details: std::collections::HashMap<_, _> = probe.details.into_iter().collect();
        assert_eq!(
            details.get("available_source_types").unwrap(),
            "unavailable"
        );
    }

    #[test]
    fn probe_timeout_appends_probe_error() {
        let source = FakeSleepingProbeSource {
            sleep_duration: std::time::Duration::from_millis(500),
        };
        let start = std::time::Instant::now();
        let probe = build_probe_from_source(
            "wayland".to_string(),
            "GNOME".to_string(),
            source,
            std::time::Duration::from_millis(100),
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 300,
            "probe took {}ms, expected < 300ms",
            elapsed.as_millis()
        );
        assert!(!probe.available);
        let details: std::collections::HashMap<_, _> = probe.details.into_iter().collect();
        let error = details.get("probe_error").unwrap();
        assert!(error.contains("screencast_version"), "error: {error}");
        assert!(error.contains("timed out"), "error: {error}");
    }

    #[test]
    fn probe_no_monitor_source_returns_unavailable() {
        let source = FakeNoMonitorProbeSource;
        let probe = build_probe_from_source(
            "wayland".to_string(),
            "GNOME".to_string(),
            source,
            std::time::Duration::from_millis(500),
        );
        assert!(!probe.available);
        assert!(probe.message.contains("incomplete"));
    }

    #[test]
    fn probe_kde_desktop_includes_quirks() {
        let source = FakeWaylandProbeSource {
            screencast_version: 4,
            source_types: SourceTypes {
                monitor: true,
                window: false,
                virtual_source: false,
            },
            cursor_modes: CursorModes {
                hidden: true,
                embedded: false,
                metadata: false,
            },
            pipewire_version: "1.0.0".to_string(),
        };
        let probe = build_probe_from_source(
            "wayland".to_string(),
            "KDE".to_string(),
            source,
            std::time::Duration::from_millis(500),
        );
        let details: std::collections::HashMap<_, _> = probe.details.into_iter().collect();
        let quirks = details.get("quirks").unwrap();
        assert!(quirks.contains("KdeMayReturnMultipleStreams"), "quirks: {quirks}");
        assert!(quirks.contains("PortalRegionPickerLikelyAvailable"), "quirks: {quirks}");
        assert!(quirks.contains("RegionPickerMayReturnVideoCrop"), "quirks: {quirks}");
    }
}
