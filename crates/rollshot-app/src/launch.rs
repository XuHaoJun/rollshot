use clap::{Parser, Subcommand, ValueEnum};
use rollshot_capture::{CaptureRequest, CaptureScope, InteractiveLaunchOptions, Workflow};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    Open {
        path: PathBuf,
    },
    Ocr {
        options: InteractiveLaunchOptions,
        graphical_feedback: bool,
    },
    Daemon,
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
    #[cfg(feature = "action-guide")]
    ActionGuide(ActionGuideLaunch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(feature = "action-guide")]
pub enum ActionGuideLaunch {
    Home,
    Record { fullscreen: bool },
    Open { path: Option<PathBuf> },
}

/// Top-level launch parser for the interactive capture app. Running with no
/// subcommand is equivalent to `capture` with all defaults.
#[derive(Debug, Parser)]
#[command(
    name = "rollshot-app",
    version,
    about = "rollshot interactive capture app"
)]
pub struct LaunchCli {
    /// Write the diagnostic session to a JSONL file alongside console output.
    #[arg(long, global = true)]
    pub log_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<LaunchCommand>,
}

#[derive(Debug, Subcommand)]
pub enum LaunchCommand {
    /// Capture a screenshot or scrolling capture (default when no subcommand).
    Capture(CaptureArgs),

    /// Open an existing PNG or JPEG for annotation and optional OCR.
    Open(OpenArgs),

    /// Recognize text in a selected region and copy it to the clipboard.
    Ocr(OcrArgs),

    /// Run Rollshot in the system tray and listen for the capture shortcut.
    Daemon,

    /// Record a desktop workflow into an Action Guide.
    #[cfg(feature = "action-guide")]
    ActionGuide(ActionGuideArgs),

    /// Probe Action Guide input capability and exit.
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
}

#[derive(Debug, clap::Args)]
#[cfg(feature = "action-guide")]
pub struct ActionGuideArgs {
    /// Record a new Action Guide workflow.
    #[arg(long, conflicts_with = "open")]
    pub record: bool,

    /// Record the whole display instead of selecting a region.
    #[arg(long, requires = "record")]
    pub fullscreen: bool,

    /// Open an existing Action Guide project.
    #[arg(long, conflicts_with = "record", num_args = 0..=1, value_name = "PATH")]
    pub open: Option<Option<PathBuf>>,
}

#[derive(Debug, clap::Args)]
pub struct OcrArgs {
    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = rollshot_capture::KNOWN_BACKEND_NAMES,
    )]
    pub backend: String,

    /// Include the cursor in captured frames.
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,

    /// Show graphical feedback during capture (daemon children only).
    #[arg(long, hide = true, default_value_t = false)]
    pub graphical_feedback: bool,
}

#[derive(Debug, clap::Args)]
pub struct CaptureArgs {
    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = rollshot_capture::KNOWN_BACKEND_NAMES,
    )]
    pub backend: String,

    /// Capture frame rate (used by real backends).
    #[arg(long, default_value_t = 5)]
    pub fps: u32,

    /// Include the cursor in captured frames.
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,

    /// What to do with the captured frames.
    #[arg(long, value_enum, default_value_t = WorkflowArg::Scrolling)]
    pub workflow: WorkflowArg,

    /// What area to capture.
    #[arg(long, value_enum, default_value_t = ScopeArg::Region)]
    pub scope: ScopeArg,
}

#[derive(Debug, clap::Args)]
pub struct OpenArgs {
    /// Existing static PNG or JPEG to open in the Result Workspace.
    #[arg(value_name = "IMAGE")]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum WorkflowArg {
    Screenshot,
    Scrolling,
}

impl From<WorkflowArg> for Workflow {
    fn from(value: WorkflowArg) -> Self {
        match value {
            WorkflowArg::Screenshot => Workflow::Screenshot,
            WorkflowArg::Scrolling => Workflow::Scrolling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ScopeArg {
    Region,
    Fullscreen,
}

impl From<ScopeArg> for CaptureScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Region => CaptureScope::Region,
            ScopeArg::Fullscreen => CaptureScope::Fullscreen,
        }
    }
}

/// Lower a parsed launch command into a `LaunchMode`. `None` (no subcommand)
/// resolves to the default capture options. Rejects the unwired
/// `scrolling + fullscreen` capture combination with a clear message.
pub fn resolve_launch_mode(command: Option<LaunchCommand>) -> Result<LaunchMode, String> {
    match command {
        None => Ok(LaunchMode::Capture(
            InteractiveLaunchOptions::default_capture(),
        )),
        Some(LaunchCommand::Capture(args)) => {
            let request = CaptureRequest {
                workflow: args.workflow.into(),
                scope: args.scope.into(),
            };
            if !request.is_supported() {
                return Err(
                    "unsupported capture combination: scrolling + fullscreen is not wired; \
                     use scrolling + region, screenshot + region, or screenshot + fullscreen"
                        .to_string(),
                );
            }
            Ok(LaunchMode::Capture(InteractiveLaunchOptions {
                backend: args.backend,
                fps: args.fps,
                show_cursor: args.show_cursor,
                initial_request: request,
            }))
        }
        Some(LaunchCommand::Daemon) => Ok(LaunchMode::Daemon),
        Some(LaunchCommand::Ocr(args)) => Ok(LaunchMode::Ocr {
            options: InteractiveLaunchOptions {
                backend: args.backend,
                fps: 5,
                show_cursor: args.show_cursor,
                initial_request: CaptureRequest::screenshot_region(),
            },
            graphical_feedback: args.graphical_feedback,
        }),
        Some(LaunchCommand::Open(args)) => Ok(LaunchMode::Open { path: args.path }),
        #[cfg(feature = "action-guide")]
        Some(LaunchCommand::ActionGuide(args)) => {
            if args.record {
                Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Record {
                    fullscreen: args.fullscreen,
                }))
            } else if let Some(path) = args.open {
                Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Open { path }))
            } else {
                Ok(LaunchMode::ActionGuide(ActionGuideLaunch::Home))
            }
        }
        #[cfg(feature = "action-guide")]
        Some(LaunchCommand::ActionGuideProbe) => Ok(LaunchMode::ActionGuideProbe),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_launch_mode, LaunchCli, LaunchMode};
    use clap::Parser;
    use rollshot_capture::CaptureRequest;
    use std::path::PathBuf;

    #[cfg(feature = "action-guide")]
    use super::ActionGuideLaunch;

    #[cfg(feature = "action-guide")]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Route {
        LinuxPhasedHost,
        LinuxChildOverlay,
        MacOsProductDaemon,
    }

    #[cfg(feature = "action-guide")]
    fn route_action_guide_launch(launch: &ActionGuideLaunch) -> Route {
        match launch {
            ActionGuideLaunch::Home | ActionGuideLaunch::Open { .. } => {
                if cfg!(target_os = "macos") {
                    Route::MacOsProductDaemon
                } else {
                    Route::LinuxPhasedHost
                }
            }
            ActionGuideLaunch::Record { .. } => {
                if cfg!(target_os = "macos") {
                    Route::MacOsProductDaemon
                } else {
                    Route::LinuxChildOverlay
                }
            }
        }
    }

    fn parse(args: &[&str]) -> Result<LaunchMode, String> {
        let cli = LaunchCli::try_parse_from(args).map_err(|e| e.to_string())?;
        resolve_launch_mode(cli.command)
    }

    #[test]
    fn no_subcommand_uses_defaults() {
        let mode = parse(&["rollshot-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_backend_and_fps_flags() {
        let mode = parse(&[
            "rollshot-app",
            "capture",
            "--backend",
            "macos-sck",
            "--fps",
            "30",
        ])
        .expect("parse capture flags");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "macos-sck");
                assert_eq!(options.fps, 30);
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_show_cursor_flag() {
        let mode = parse(&["rollshot-app", "capture", "--show-cursor"]).expect("parse");
        match mode {
            LaunchMode::Capture(options) => assert!(options.show_cursor),
            _ => unreachable!("test expects Capture mode"),
        }
    }

    #[test]
    fn capture_screenshot_fullscreen() {
        let mode = parse(&[
            "rollshot-app",
            "capture",
            "--workflow",
            "screenshot",
            "--scope",
            "fullscreen",
        ])
        .expect("parse");
        assert!(matches!(
            mode,
            LaunchMode::Capture(options)
                if options.initial_request == CaptureRequest::screenshot_fullscreen()
        ));
    }

    #[test]
    fn capture_screenshot_region() {
        let mode = parse(&[
            "rollshot-app",
            "capture",
            "--workflow",
            "screenshot",
            "--scope",
            "region",
        ])
        .expect("parse");
        assert!(matches!(
            mode,
            LaunchMode::Capture(options)
                if options.initial_request == CaptureRequest::screenshot_region()
        ));
    }

    #[test]
    fn scrolling_fullscreen_is_rejected() {
        let err = parse(&["rollshot-app", "capture", "--scope", "fullscreen"])
            .expect_err("scrolling + fullscreen should be rejected");
        assert!(err.contains("scrolling"), "err = {err}");
        assert!(err.contains("fullscreen"), "err = {err}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let err = parse(&["rollshot-app", "capture", "--bogus"]).expect_err("unknown flag");
        assert!(!err.is_empty());
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        let err = parse(&["rollshot-app", "bogus"]).expect_err("unknown subcommand");
        assert!(!err.is_empty());
    }

    #[test]
    fn save_dialog_temp_is_rejected() {
        let err = parse(&["rollshot-app", "--save-dialog-temp", "/tmp/rollshot.png"])
            .expect_err("save-dialog-temp should be rejected");
        assert!(!err.is_empty());
    }

    #[test]
    fn log_file_global_before_subcommand() {
        let cli = LaunchCli::try_parse_from(["rollshot-app", "--log-file", "/tmp/x.jsonl"])
            .expect("parse log-file");
        assert_eq!(
            cli.log_file.as_deref(),
            Some(std::path::Path::new("/tmp/x.jsonl"))
        );
        assert!(cli.command.is_none());
    }

    #[test]
    fn log_file_global_after_subcommand() {
        let cli = LaunchCli::try_parse_from([
            "rollshot-app",
            "capture",
            "--log-file",
            "/tmp/x.jsonl",
            "--backend",
            "auto",
        ])
        .expect("parse log-file after subcommand");
        assert_eq!(
            cli.log_file.as_deref(),
            Some(std::path::Path::new("/tmp/x.jsonl"))
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_home_default() {
        let mode = parse(&["rollshot-app", "action-guide"]).expect("parse");
        assert_eq!(mode, LaunchMode::ActionGuide(ActionGuideLaunch::Home));
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_record_no_fullscreen() {
        let mode = parse(&["rollshot-app", "action-guide", "--record"]).expect("parse");
        assert_eq!(
            mode,
            LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: false })
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_record_with_fullscreen() {
        let mode =
            parse(&["rollshot-app", "action-guide", "--record", "--fullscreen"]).expect("parse");
        assert_eq!(
            mode,
            LaunchMode::ActionGuide(ActionGuideLaunch::Record { fullscreen: true })
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_fullscreen_requires_record() {
        let err = parse(&["rollshot-app", "action-guide", "--fullscreen"]).unwrap_err();
        assert!(
            err.contains("--record"),
            "error should mention --record: {err}"
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_record_and_open_conflict() {
        assert!(parse(&["rollshot-app", "action-guide", "--record", "--open"]).is_err());
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_open_no_path() {
        let mode = parse(&["rollshot-app", "action-guide", "--open"]).expect("parse");
        assert_eq!(
            mode,
            LaunchMode::ActionGuide(ActionGuideLaunch::Open { path: None })
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_open_with_path() {
        let mode = parse(&[
            "rollshot-app",
            "action-guide",
            "--open",
            "/tmp/a.rollshot-guide",
        ])
        .expect("parse");
        assert_eq!(
            mode,
            LaunchMode::ActionGuide(ActionGuideLaunch::Open {
                path: Some(std::path::PathBuf::from("/tmp/a.rollshot-guide"))
            })
        );
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_probe_mode() {
        let mode = parse(&["rollshot-app", "action-guide-probe"]).expect("parse");
        assert!(matches!(mode, LaunchMode::ActionGuideProbe));
    }

    #[test]
    fn daemon_subcommand_selects_daemon_mode() {
        let mode = parse(&["rollshot-app", "daemon"]).expect("parse daemon");
        assert!(matches!(mode, LaunchMode::Daemon));
    }

    #[test]
    fn no_subcommand_still_selects_default_capture() {
        let mode = parse(&["rollshot-app"]).expect("parse default");
        assert!(matches!(mode, LaunchMode::Capture(_)));
    }

    #[test]
    fn ocr_uses_screenshot_region_and_capture_flags() {
        let mode = parse(&[
            "rollshot-app",
            "ocr",
            "--backend",
            "fixture",
            "--show-cursor",
        ])
        .expect("ocr parse");
        assert!(matches!(
            mode,
            LaunchMode::Ocr { options, graphical_feedback: false }
                if options.backend == "fixture"
                    && options.show_cursor
                    && options.initial_request == CaptureRequest::screenshot_region()
        ));
    }

    #[test]
    fn ocr_rejects_workflow_and_scope_flags() {
        assert!(parse(&["rollshot-app", "ocr", "--scope", "fullscreen"]).is_err());
        assert!(parse(&["rollshot-app", "ocr", "--workflow", "scrolling"]).is_err());
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_route_home() {
        let route = route_action_guide_launch(&ActionGuideLaunch::Home);
        if cfg!(target_os = "macos") {
            assert_eq!(route, Route::MacOsProductDaemon);
        } else {
            assert_eq!(route, Route::LinuxPhasedHost);
        }
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_route_open() {
        let route = route_action_guide_launch(&ActionGuideLaunch::Open { path: None });
        if cfg!(target_os = "macos") {
            assert_eq!(route, Route::MacOsProductDaemon);
        } else {
            assert_eq!(route, Route::LinuxPhasedHost);
        }
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_route_open_with_path() {
        let route = route_action_guide_launch(&ActionGuideLaunch::Open {
            path: Some(std::path::PathBuf::from("/tmp/test")),
        });
        if cfg!(target_os = "macos") {
            assert_eq!(route, Route::MacOsProductDaemon);
        } else {
            assert_eq!(route, Route::LinuxPhasedHost);
        }
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_route_record() {
        let route = route_action_guide_launch(&ActionGuideLaunch::Record { fullscreen: false });
        if cfg!(target_os = "macos") {
            assert_eq!(route, Route::MacOsProductDaemon);
        } else {
            assert_eq!(route, Route::LinuxChildOverlay);
        }
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_route_record_fullscreen() {
        let route = route_action_guide_launch(&ActionGuideLaunch::Record { fullscreen: true });
        if cfg!(target_os = "macos") {
            assert_eq!(route, Route::MacOsProductDaemon);
        } else {
            assert_eq!(route, Route::LinuxChildOverlay);
        }
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn action_guide_probe_is_separate_from_action_guide_launch() {
        let probe = LaunchMode::ActionGuideProbe;
        let home = LaunchMode::ActionGuide(ActionGuideLaunch::Home);
        assert_ne!(probe, home);
    }

    #[test]
    fn open_requires_exactly_one_image_path() {
        let mode =
            parse(&["rollshot-app", "open", "fixtures/sample.png"]).expect("open path parses");
        assert_eq!(
            mode,
            LaunchMode::Open {
                path: PathBuf::from("fixtures/sample.png"),
            }
        );

        assert!(LaunchCli::try_parse_from(["rollshot-app", "open"]).is_err());
        assert!(LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "b.png"]).is_err());
    }

    #[test]
    fn open_rejects_capture_only_flags() {
        assert!(
            LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "--backend", "auto"])
                .is_err()
        );
        assert!(
            LaunchCli::try_parse_from(["rollshot-app", "open", "a.png", "--show-cursor"]).is_err()
        );
    }
}
