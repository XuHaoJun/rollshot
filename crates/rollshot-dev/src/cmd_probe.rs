use std::fmt::Write;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rollshot_capture::CaptureBackend;
use rollshot_capture::{default_backend, CaptureProbe};
use serde::Serialize;

use crate::args::ProbeArgs;
use crate::cli_error::CliError;

pub fn run(args: &ProbeArgs) -> Result<String, CliError> {
    let report = build_report();
    if args.json {
        serde_json::to_string_pretty(&report)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|err| CliError::new(format!("failed to render probe json: {err}"), 1))
    } else {
        Ok(render_text(&report))
    }
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    os: &'static str,
    session_type: String,
    desktop: String,
    default_backend: &'static str,
    backends: Vec<ProbeEntry>,
}

#[derive(Debug, Serialize)]
struct ProbeEntry {
    name: &'static str,
    available: bool,
    message: String,
    details: Vec<(String, String)>,
}

impl From<CaptureProbe> for ProbeEntry {
    fn from(p: CaptureProbe) -> Self {
        ProbeEntry {
            name: p.backend,
            available: p.available,
            message: p.message,
            details: p.details,
        }
    }
}

fn build_report() -> ProbeReport {
    let os = std::env::consts::OS;
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let default = default_backend();

    #[allow(unused_mut)]
    let mut backends: Vec<ProbeEntry> = vec![ProbeEntry {
        name: "fixture",
        available: true,
        message: "directory-based test backend".to_string(),
        details: Vec::new(),
    }];

    #[cfg(target_os = "linux")]
    {
        let backend = rollshot_capture::LinuxKwinBackend::new_real();
        backends.push(backend.probe().into());
    }
    #[cfg(target_os = "linux")]
    {
        let backend = rollshot_capture::LinuxPortalBackend::new();
        backends.push(backend.probe().into());
    }
    #[cfg(target_os = "macos")]
    {
        let backend = rollshot_capture::MacosScreenCaptureKitBackend::new();
        backends.push(backend.probe().into());
    }

    ProbeReport {
        os,
        session_type,
        desktop,
        default_backend: default.as_flag(),
        backends,
    }
}

fn render_text(report: &ProbeReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "rollshot-dev probe");
    let _ = writeln!(out, "  os: {}", report.os);
    let _ = writeln!(out, "  session_type: {}", report.session_type);
    let _ = writeln!(out, "  desktop: {}", report.desktop);
    let _ = writeln!(out, "  default backend: {}", report.default_backend);
    let _ = writeln!(out, "  backends:");
    for b in &report.backends {
        let status = if b.available {
            "available"
        } else {
            "unavailable"
        };
        let _ = writeln!(out, "    - {} ({status}): {}", b.name, b.message);
        for (k, v) in &b.details {
            let _ = writeln!(out, "        {k}: {v}");
        }
    }
    out
}
