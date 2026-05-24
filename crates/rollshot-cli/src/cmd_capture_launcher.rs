use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use rollshot_capture::InteractiveLaunchOptions;

use crate::args::{CaptureArgs, MAX_FRAMES_DEFAULT};
use crate::cli_error::CliError;

const APP_ENV: &str = "ROLLSHOT_APP";

pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    reject_headless_only_flags(args)?;

    let options = launch_options(args);
    let app_path = resolve_app_binary()?;
    let mut command = build_app_command(&app_path, &options)?;

    let status = command.status().map_err(|err| {
        CliError::new(format!("failed to launch {}: {err}", app_path.display()), 1)
    })?;

    if status.success() {
        Ok(String::new())
    } else {
        Err(CliError::new(
            format!(
                "{} exited with status {}",
                app_path.display(),
                status_label(status)
            ),
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        ))
    }
}

fn launch_options(args: &CaptureArgs) -> InteractiveLaunchOptions {
    InteractiveLaunchOptions {
        backend: args.backend.clone(),
        fps: args.fps,
        show_cursor: args.show_cursor,
    }
}

fn build_app_command(
    app_path: &Path,
    options: &InteractiveLaunchOptions,
) -> Result<Command, CliError> {
    #[cfg(windows)]
    {
        if app_path
            .extension()
            .map_or(false, |ext| ext == "cmd" || ext == "bat")
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/c").arg(app_path);
            cmd.args(app_args(options)?);
            return Ok(cmd);
        }
    }
    let mut cmd = Command::new(app_path);
    cmd.args(app_args(options)?);
    Ok(cmd)
}

fn app_args(options: &InteractiveLaunchOptions) -> Result<Vec<OsString>, CliError> {
    let payload = serde_json::to_string(options)
        .map_err(|err| CliError::new(format!("failed to encode GUI launch options: {err}"), 1))?;
    Ok(vec![OsString::from("--capture"), OsString::from(payload)])
}

fn reject_headless_only_flags(args: &CaptureArgs) -> Result<(), CliError> {
    let mut rejected = Vec::new();

    if args.output.is_some() {
        rejected.push("--output");
    }
    if args.region != "auto" {
        rejected.push("--region");
    }
    if args.fixture.is_some() {
        rejected.push("--fixture");
    }
    if args.dump_frames.is_some() {
        rejected.push("--dump-frames");
    }
    if args.debug_match_report.is_some() {
        rejected.push("--debug-match-report");
    }
    if args.max_frames != MAX_FRAMES_DEFAULT {
        rejected.push("--max-frames");
    }
    if args.quiet {
        rejected.push("--quiet");
    }
    if args.enable_akaze {
        rejected.push("--enable-akaze");
    }
    if args.disable_feature_fallback {
        rejected.push("--disable-feature-fallback");
    }

    if rejected.is_empty() {
        Ok(())
    } else {
        Err(CliError::new(
            format!(
                "the following flags are only supported with --headless: {}",
                rejected.join(", ")
            ),
            1,
        ))
    }
}

fn resolve_app_binary() -> Result<PathBuf, CliError> {
    let current_exe = std::env::current_exe()
        .map_err(|err| CliError::new(format!("failed to locate rollshot binary: {err}"), 1))?;
    resolve_app_binary_from_env_and_exe(std::env::var_os(APP_ENV), &current_exe)
}

fn resolve_app_binary_from_env_and_exe(
    env_path: Option<OsString>,
    current_exe: &Path,
) -> Result<PathBuf, CliError> {
    if let Some(path) = env_path {
        if path.is_empty() {
            return Err(CliError::new(
                format!("{APP_ENV} is set but empty; expected path to rollshot-app"),
                1,
            ));
        }
        let resolved = PathBuf::from(path);
        if !resolved.exists() {
            return Err(CliError::new(
                format!(
                    "{APP_ENV} points to {} but the file does not exist",
                    resolved.display()
                ),
                1,
            ));
        }
        return Ok(resolved);
    }

    let bin_dir = current_exe.parent().ok_or_else(|| {
        CliError::new(
            format!(
                "failed to locate {} next to rollshot",
                default_app_binary_name()
            ),
            1,
        )
    })?;
    let app_path = bin_dir.join(default_app_binary_name());
    if !app_path.exists() {
        let hint = if is_cargo_target_dir(bin_dir) {
            "hint: the GUI app must be built separately with the Tauri toolchain:\n  \
             pnpm --dir crates/rollshot-app install\n  \
             pnpm --dir crates/rollshot-app run tauri build --debug\n\
             or use --headless to skip the GUI"
        } else {
            "hint: reinstall rollshot or set ROLLSHOT_APP to the GUI binary path"
        };
        return Err(CliError::new(
            format!(
                "{} not found next to rollshot (looked in {})\n{hint}",
                default_app_binary_name(),
                bin_dir.display(),
            ),
            1,
        ));
    }
    Ok(app_path)
}

fn is_cargo_target_dir(dir: &Path) -> bool {
    dir.components().any(|c| c.as_os_str() == "target")
}

#[cfg(windows)]
fn default_app_binary_name() -> &'static str {
    "rollshot-app.exe"
}

#[cfg(not(windows))]
fn default_app_binary_name() -> &'static str {
    "rollshot-app"
}

fn status_label(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        app_args, launch_options, reject_headless_only_flags, resolve_app_binary_from_env_and_exe,
    };
    use crate::args::CaptureArgs;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn base_args() -> CaptureArgs {
        CaptureArgs {
            headless: false,
            backend: "linux-portal".to_string(),
            region: "auto".to_string(),
            output: None,
            fixture: None,
            dump_frames: None,
            debug_match_report: None,
            max_frames: 200,
            fps: 7,
            show_cursor: true,
            quiet: false,
            enable_akaze: false,
            disable_feature_fallback: false,
        }
    }

    #[test]
    fn launch_options_keep_only_interactive_fields() {
        let args = base_args();
        let options = launch_options(&args);

        assert_eq!(options.backend, "linux-portal");
        assert_eq!(options.fps, 7);
        assert!(options.show_cursor);
    }

    #[test]
    fn app_args_include_capture_flag_and_json_payload() {
        let args = base_args();
        let options = launch_options(&args);

        let app_args = app_args(&options).expect("build app args");
        assert_eq!(app_args[0], OsString::from("--capture"));

        let payload = app_args[1].to_string_lossy();
        assert!(payload.contains("\"backend\":\"linux-portal\""));
        assert!(payload.contains("\"fps\":7"));
        assert!(payload.contains("\"show_cursor\":true"));
    }

    #[test]
    fn reject_headless_only_flags_lists_all_rejected_flags() {
        let mut args = base_args();
        args.output = Some(PathBuf::from("out.png"));
        args.region = "10,20 100x200".to_string();
        args.fixture = Some(PathBuf::from("frames"));
        args.dump_frames = Some(PathBuf::from("dump"));
        args.debug_match_report = Some(PathBuf::from("report.json"));
        args.max_frames = 10;
        args.quiet = true;
        args.enable_akaze = true;
        args.disable_feature_fallback = true;

        let err = reject_headless_only_flags(&args).expect_err("flags rejected");
        assert!(err.message.contains("--headless"), "{}", err.message);
        for flag in [
            "--output",
            "--region",
            "--fixture",
            "--dump-frames",
            "--debug-match-report",
            "--max-frames",
            "--quiet",
            "--enable-akaze",
            "--disable-feature-fallback",
        ] {
            assert!(err.message.contains(flag), "{} missing {flag}", err.message);
        }
    }

    #[test]
    fn resolve_app_binary_prefers_env_override() {
        let current_exe_path = std::env::current_exe().expect("current exe");
        let env_path = current_exe_path.clone();

        let resolved = resolve_app_binary_from_env_and_exe(
            Some(OsString::from(env_path.as_os_str())),
            Path::new("target/debug/rollshot"),
        )
        .expect("env override resolves");

        assert_eq!(resolved, env_path);
    }

    #[test]
    fn resolve_app_binary_env_missing_file() {
        let env_path = PathBuf::from("/no/such/rollshot-app");
        let err = resolve_app_binary_from_env_and_exe(
            Some(OsString::from(env_path.as_os_str())),
            Path::new("target/debug/rollshot"),
        )
        .expect_err("missing env path");

        assert!(err.message.contains("ROLLSHOT_APP"), "{}", err.message);
        assert!(err.message.contains("does not exist"), "{}", err.message);
    }

    #[test]
    fn resolve_app_binary_sibling_missing_dev() {
        let err = resolve_app_binary_from_env_and_exe(
            None,
            Path::new("/project/target/debug/rollshot"),
        )
        .expect_err("sibling missing in dev");

        assert!(err.message.contains("not found"), "{}", err.message);
        assert!(err.message.contains("tauri"), "{}", err.message);
        assert!(err.message.contains("--headless"), "{}", err.message);
    }

    #[test]
    fn resolve_app_binary_sibling_missing_prod() {
        let err = resolve_app_binary_from_env_and_exe(
            None,
            Path::new("/usr/bin/rollshot"),
        )
        .expect_err("sibling missing in prod");

        assert!(err.message.contains("not found"), "{}", err.message);
        assert!(err.message.contains("ROLLSHOT_APP"), "{}", err.message);
        assert!(!err.message.contains("tauri"), "{}", err.message);
    }
}
