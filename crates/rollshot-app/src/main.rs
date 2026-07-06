#[cfg(feature = "action-guide")]
mod action_input;
pub mod daemon;
mod diagnostics;
mod launch;
#[cfg(feature = "action-guide")]
mod managed_ffmpeg;
#[cfg(feature = "action-guide")]
mod timeline_workspace;

use clap::Parser;
use launch::{LaunchCommand, LaunchMode};
use std::process::ExitCode;

// Registered on every target so the portable thumbnail timer + interaction
// helpers compile and unit-test on Linux. Only its `view` is macOS-gated.
#[cfg(target_os = "macos")]
mod macos_product;
#[cfg(all(target_os = "macos", feature = "action-guide"))]
mod macos_recording_tray;
// Registered on every target so the pure drag placement/result helpers compile
// and unit-test on Linux; the AppKit bridge inside it is macOS-gated.
mod issue_pack;
mod macos_native_drag;
mod macos_thumbnail;
mod post_capture;
mod result_workspace;
mod storage;

fn main() -> ExitCode {
    let cli = launch::LaunchCli::parse();

    let selected = diagnostics::select_filter(std::env::var("RUST_LOG").ok().as_deref());
    let _diagnostics = match diagnostics::init(cli.log_file.as_deref(), &selected) {
        Ok(guard) => guard,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    if !selected.ignored.is_empty() {
        tracing::warn!(
            target: diagnostics::TARGET_FILTER,
            ignored = ?selected.ignored,
            "ignored invalid RUST_LOG directives"
        );
    }

    match run(cli.command, cli.log_file.is_some()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                target: diagnostics::TARGET_APP,
                error_category = diagnostics::classify_app_error(&error),
                "application failed"
            );
            ExitCode::FAILURE
        }
    }
}

fn run(command: Option<LaunchCommand>, file_logging: bool) -> Result<(), String> {
    let launch_mode = launch::resolve_launch_mode(command)?;

    match launch_mode {
        LaunchMode::Capture(options) => {
            tracing::info!(
                target: diagnostics::TARGET_APP,
                version = env!("CARGO_PKG_VERSION"),
                os = std::env::consts::OS,
                arch = std::env::consts::ARCH,
                backend = options.backend.as_str(),
                fps = options.fps,
                show_cursor = options.show_cursor,
                workflow = ?options.initial_request.workflow,
                scope = ?options.initial_request.scope,
                file_logging,
                "capture session started"
            );
            run_iced_capture(options)
        }
        LaunchMode::Daemon => daemon::run(),
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuideProbe => run_action_guide_probe(),
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuide { fullscreen } => run_action_guide_record(fullscreen),
    }
}

#[cfg(feature = "action-guide")]
fn run_action_guide_probe() -> Result<(), String> {
    use crate::action_input::{create_input_source, degraded_advisory};
    use rollshot_action::{
        ActionRecorder, CaptureRegion, DetectorConfig, InputCapability, StartedSemanticInput,
        StoreConfig,
    };

    // P0b probe: no overlay region picker yet (deferred to the app-integration
    // plan). Observe the full virtual region as a placeholder.
    let region = CaptureRegion {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let mut input = StartedSemanticInput::start(create_input_source(), region);
    let capability = input.capability();

    match capability {
        InputCapability::SemanticEvents => {
            tracing::info!(target: "rollshot::action::probe", "semantic input enabled");
            println!("Action Guide input probe: Semantic input enabled.");
        }
        InputCapability::VisualOnly { reason } => {
            tracing::warn!(target: "rollshot::action::probe", ?reason, "visual-only");
            println!("Action Guide input probe: Visual-only detection.");
            println!("{}", degraded_advisory(reason));
        }
    }

    // Poll for ~3 seconds into a throwaway recorder so semantic events are
    // observed only during this active probe, then stop.
    let mut recorder =
        ActionRecorder::new(region, StoreConfig::default(), DetectorConfig::default());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        input.poll_into(&mut recorder);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    input.stop();
    println!("Action Guide input probe finished.");
    Ok(())
}

#[cfg(all(feature = "action-guide", target_os = "linux"))]
fn run_action_guide_record(fullscreen: bool) -> Result<(), String> {
    use rollshot_capture::CaptureRequest;
    let request = if fullscreen {
        CaptureRequest::action_guide_fullscreen()
    } else {
        CaptureRequest::action_guide_region()
    };
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        request,
        target_output_name: None,
    };
    let source = crate::action_input::create_input_source();
    let outcome = if fullscreen {
        rollshot_iced_overlay::run_action_guide_fullscreen(config, source)
    } else {
        rollshot_iced_overlay::run_action_guide(config, source)
    }
    .map_err(|e| e.to_string())?;
    match outcome {
        Some((recording, capability, region)) => {
            let source_kind = crate::timeline_workspace::source_kind_for(
                capability,
                crate::storage::Platform::Linux,
            );
            crate::timeline_workspace::run(recording, region, capability, source_kind)
        }
        None => Ok(()),
    }
}

#[cfg(all(feature = "action-guide", target_os = "macos"))]
fn run_action_guide_record(fullscreen: bool) -> Result<(), String> {
    use rollshot_capture::CaptureRequest;

    let request = if fullscreen {
        CaptureRequest::action_guide_fullscreen()
    } else {
        CaptureRequest::action_guide_region()
    };
    macos_product::run(rollshot_iced_overlay::OverlayConfig {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        request,
        target_output_name: None,
    })
}

fn run_iced_capture(options: rollshot_capture::InteractiveLaunchOptions) -> Result<(), String> {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
        request: options.initial_request,
        target_output_name: None,
    };

    #[cfg(target_os = "linux")]
    {
        run_product_capture(config)
    }

    #[cfg(target_os = "macos")]
    {
        run_product_capture(config)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = config;
        Err("unsupported platform".to_string())
    }
}

#[cfg(target_os = "linux")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    match post_capture::capture_completion(
        rollshot_iced_overlay::run_overlay(config).map_err(|e| e.to_string())?,
    ) {
        post_capture::CaptureCompletion::Present(result) => {
            post_capture::handle_linux_capture(result)
        }
        post_capture::CaptureCompletion::Cancelled => Ok(()),
    }
}

/// macOS: route capture into the single-process product daemon, which owns the
/// whole capture → thumbnail / Result Workspace flow in one event loop.
#[cfg(target_os = "macos")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    macos_product::run(config)
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn run_returns_error_for_unsupported_platform() {
        // `None` resolves to default capture, which reaches the platform guard.
        let err = super::run(None, false);
        assert!(err.is_err());
    }
}
