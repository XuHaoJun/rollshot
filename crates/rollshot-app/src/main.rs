#[cfg(feature = "action-guide")]
#[allow(dead_code)]
mod action_guide_home;
#[cfg(all(target_os = "linux", feature = "action-guide"))]
mod action_guide_linux_product;
#[cfg(feature = "action-guide")]
mod action_input;
pub mod daemon;
mod diagnostics;
mod image_clipboard;
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
mod platform_actions;
mod post_capture;
pub(crate) mod product_ocr;
mod quick_ocr;
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
            run_iced_capture(options, post_capture::CapturePurpose::Present)
        }
        LaunchMode::Daemon => daemon::run(),
        LaunchMode::Ocr {
            options,
            graphical_feedback,
        } => run_iced_capture(
            options,
            post_capture::CapturePurpose::Ocr { graphical_feedback },
        ),
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuideProbe => run_action_guide_probe(),
        #[cfg(feature = "action-guide")]
        LaunchMode::ActionGuide(launch) => run_action_guide_launch(launch),
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
    macos_product::run(
        rollshot_iced_overlay::OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            request,
            target_output_name: None,
        },
        post_capture::CapturePurpose::Present,
    )
}

#[cfg(all(feature = "action-guide", target_os = "linux"))]
fn run_action_guide_launch(launch: launch::ActionGuideLaunch) -> Result<(), String> {
    use crate::action_guide_home::ActionGuideIntent;
    match launch {
        launch::ActionGuideLaunch::Home => action_guide_linux_product::run(ActionGuideIntent::Home),
        launch::ActionGuideLaunch::Record { fullscreen } => run_action_guide_record(fullscreen),
        launch::ActionGuideLaunch::Open { path } => {
            action_guide_linux_product::run(ActionGuideIntent::Open { path })
        }
    }
}

#[cfg(all(feature = "action-guide", target_os = "macos"))]
fn run_action_guide_launch(launch: launch::ActionGuideLaunch) -> Result<(), String> {
    use crate::action_guide_home::ActionGuideIntent;
    match launch {
        launch::ActionGuideLaunch::Home => macos_product::run_action_guide(ActionGuideIntent::Home),
        launch::ActionGuideLaunch::Record { fullscreen } => {
            macos_product::run_action_guide(ActionGuideIntent::Record { fullscreen })
        }
        launch::ActionGuideLaunch::Open { path } => {
            macos_product::run_action_guide(ActionGuideIntent::Open { path })
        }
    }
}

fn run_iced_capture(
    options: rollshot_capture::InteractiveLaunchOptions,
    purpose: post_capture::CapturePurpose,
) -> Result<(), String> {
    if matches!(purpose, post_capture::CapturePurpose::Ocr { .. }) && cfg!(not(feature = "ocr")) {
        return Err(crate::product_ocr::ProductOcrError::Disabled
            .message()
            .into());
    }

    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
        request: options.initial_request,
        target_output_name: None,
    };

    #[cfg(target_os = "linux")]
    {
        run_product_capture(config, purpose)
    }

    #[cfg(target_os = "macos")]
    {
        run_product_capture(config, purpose)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (config, purpose);
        Err("unsupported platform".to_string())
    }
}

#[cfg(target_os = "linux")]
fn run_product_capture(
    config: rollshot_iced_overlay::OverlayConfig,
    purpose: post_capture::CapturePurpose,
) -> Result<(), String> {
    match post_capture::select_completion(
        purpose,
        rollshot_iced_overlay::run_overlay(config).map_err(|e| e.to_string())?,
    ) {
        post_capture::PurposeCompletion::Cancelled => Ok(()),
        post_capture::PurposeCompletion::Present(result) => {
            post_capture::handle_linux_capture(result)
        }
        post_capture::PurposeCompletion::Ocr {
            image,
            graphical_feedback,
        } => quick_ocr::complete_cli(image, graphical_feedback),
    }
}

/// macOS: route capture into the single-process product daemon, which owns the
/// whole capture → thumbnail / Result Workspace flow in one event loop.
#[cfg(target_os = "macos")]
fn run_product_capture(
    config: rollshot_iced_overlay::OverlayConfig,
    purpose: post_capture::CapturePurpose,
) -> Result<(), String> {
    macos_product::run(config, purpose)
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

    #[test]
    #[cfg(not(feature = "ocr"))]
    fn ocr_disabled_build_fails_before_capture() {
        use clap::Parser;
        let cli =
            super::launch::LaunchCli::try_parse_from(["rollshot-app", "ocr"]).expect("parse ocr");
        let err = super::run(cli.command, false).unwrap_err();
        assert_eq!(err, "OCR is not available in this build");
    }
}
