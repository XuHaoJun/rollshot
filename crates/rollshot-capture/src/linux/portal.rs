use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, RegionMode};

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
        probe_from_env(
            std::env::var("XDG_SESSION_TYPE").ok(),
            std::env::var("XDG_CURRENT_DESKTOP").ok(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
