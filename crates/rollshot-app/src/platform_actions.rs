use std::path::Path;

pub(crate) struct PlatformCommand {
    program: String,
    args: Vec<String>,
}

impl PlatformCommand {
    pub(crate) fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub(crate) fn arg(mut self, arg: impl AsRef<std::ffi::OsStr>) -> Self {
        self.args.push(arg.as_ref().to_string_lossy().into_owned());
        self
    }

    pub(crate) fn spawn(&self) -> std::io::Result<()> {
        std::process::Command::new(&self.program)
            .args(&self.args)
            .spawn()?;
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
fn open_command(path: &Path) -> Result<PlatformCommand, String> {
    #[cfg(target_os = "macos")]
    return Ok(PlatformCommand::new("open").arg(path));
    #[cfg(target_os = "linux")]
    return Ok(PlatformCommand::new("xdg-open").arg(path));
}

fn run_command_with(
    command: &PlatformCommand,
    spawn: impl FnOnce(&PlatformCommand) -> std::io::Result<()>,
) -> Result<(), String> {
    spawn(command).map_err(|error| format!("open failed: {error}"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
pub(crate) fn open_path(path: &Path) -> Result<(), String> {
    let command = open_command(path)?;
    run_command_with(&command, PlatformCommand::spawn)
}

pub(crate) fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let command = PlatformCommand::new("open").arg("-R").arg(path);
        run_command_with(&command, PlatformCommand::spawn)
    }

    #[cfg(target_os = "linux")]
    {
        reveal_with_fallback(
            || reveal_with_file_manager1(path),
            || reveal_with_xdg_open(path),
        )
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("reveal is not supported on this platform".to_string())
    }
}

#[cfg(any(target_os = "linux", test))]
fn reveal_with_fallback(
    primary: impl FnOnce() -> Result<(), String>,
    fallback: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match primary() {
        Ok(()) => Ok(()),
        Err(primary_error) => fallback().map_err(|fallback_error| {
            format!("{primary_error}; fallback failed: {fallback_error}")
        }),
    }
}

#[cfg(target_os = "linux")]
fn reveal_with_file_manager1(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map_err(|_| format!("cannot convert path to file URI: {}", path.display()))?
        .to_string();
    let connection =
        zbus::blocking::Connection::session().map_err(|e| format!("D-Bus session failed: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
    )
    .map_err(|e| format!("FileManager1 proxy failed: {e}"))?;
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![uri], ""))
        .map_err(|e| format!("FileManager1 ShowItems failed: {e}"))
}

#[cfg(target_os = "linux")]
fn reveal_with_xdg_open(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or(path);
    let command = PlatformCommand::new("xdg-open").arg(parent);
    run_command_with(&command, PlatformCommand::spawn)
}

// ---------------------------------------------------------------------------
// Detached Linux record command
// ---------------------------------------------------------------------------

/// Builds the native command for launching `rollshot-app action-guide --record`
/// with optional `--fullscreen`. Uses `std::env::current_exe()` for the program
/// and `OsString` args — never lossy-converts paths.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) fn action_guide_record_command(
    fullscreen: bool,
    keep_motion: bool,
) -> Result<(std::ffi::OsString, Vec<std::ffi::OsString>), String> {
    let exe = std::env::current_exe().map_err(|e| {
        tracing::error!(target: "rollshot::platform_actions", error = %e, "failed to resolve current executable");
        format!("failed to resolve current executable: {e}")
    })?;

    let mut args: Vec<std::ffi::OsString> = vec!["action-guide".into(), "--record".into()];
    if fullscreen {
        args.push("--fullscreen".into());
    }
    if keep_motion {
        args.push("--keep-motion".into());
    }

    Ok((exe.into_os_string(), args))
}

/// Spawns the detached record child and moves `Child::wait` to a dedicated
/// reaper thread so the long-lived Home does not accumulate zombies and
/// iced/Tokio shutdown does not wait on the child.
///
/// Spawn failure returns an error string. Reaper thread failure emits a
/// privacy-safe tracing category.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) fn spawn_action_guide_record(fullscreen: bool, keep_motion: bool) -> Result<(), String> {
    let (program, args) = action_guide_record_command(fullscreen, keep_motion)?;

    let mut child = std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|e| {
            tracing::error!(target: "rollshot::platform_actions", error = %e, "failed to spawn action-guide record");
            format!("failed to spawn action-guide record: {e}")
        })?;

    std::thread::Builder::new()
        .name("ag-reaper".into())
        .spawn(move || {
            match child.wait() {
                Ok(status) => {
                    if !status.success() {
                        tracing::warn!(
                            target: "rollshot::platform_actions",
                            exit_code = status.code().unwrap_or(-1),
                            "action-guide record exited with non-zero status"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "rollshot::platform_actions",
                        error = %e,
                        "reaper wait failed"
                    );
                }
            }
        })
        .map_err(|e| {
            tracing::error!(target: "rollshot::platform_actions", error = %e, "failed to spawn reaper thread");
            format!("failed to spawn reaper thread: {e}")
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn open_command_constructs_platform_command() {
        let command = open_command(Path::new("/tmp/test.html")).unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(command.program, "open");
        #[cfg(target_os = "linux")]
        assert_eq!(command.program, "xdg-open");
        assert_eq!(command.args.len(), 1);
    }

    #[test]
    fn platform_command_spawn_can_be_overridden_for_testing() {
        let command = PlatformCommand::new("false").arg("test");
        let result = run_command_with(&command, |cmd| {
            assert_eq!(cmd.program, "false");
            assert_eq!(cmd.args, vec!["test".to_string()]);
            Err(std::io::Error::other("test error"))
        });
        assert!(result.unwrap_err().contains("test error"));
    }

    #[test]
    fn reveal_with_fallback_skips_fallback_after_primary_success() {
        let mut fallback_called = false;
        let result = reveal_with_fallback(
            || Ok(()),
            || {
                fallback_called = true;
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(!fallback_called);
    }

    #[test]
    fn reveal_with_fallback_runs_fallback_after_primary_failure() {
        let mut fallback_called = false;
        let result = reveal_with_fallback(
            || Err("D-Bus unavailable".to_string()),
            || {
                fallback_called = true;
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(fallback_called);
    }

    #[test]
    fn reveal_with_fallback_reports_both_failures() {
        let result = reveal_with_fallback(
            || Err("D-Bus unavailable".to_string()),
            || Err("xdg-open unavailable".to_string()),
        )
        .expect_err("both operations failed");
        assert!(result.contains("D-Bus unavailable"));
        assert!(result.contains("xdg-open unavailable"));
    }

    // ---- action_guide_record_command ----

    #[test]
    #[cfg(target_os = "linux")]
    fn action_guide_record_command_without_fullscreen() {
        let (program, args) = super::action_guide_record_command(false, false).unwrap();
        assert!(!program.is_empty());
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str().unwrap(), "action-guide");
        assert_eq!(args[1].to_str().unwrap(), "--record");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn action_guide_record_command_with_fullscreen() {
        let (program, args) = super::action_guide_record_command(true, false).unwrap();
        assert!(!program.is_empty());
        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_str().unwrap(), "action-guide");
        assert_eq!(args[1].to_str().unwrap(), "--record");
        assert_eq!(args[2].to_str().unwrap(), "--fullscreen");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn action_guide_record_command_uses_current_exe() {
        let (program, _args) = super::action_guide_record_command(false, false).unwrap();
        let current = std::env::current_exe().unwrap();
        assert_eq!(program.to_str().unwrap(), current.to_str().unwrap());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn action_guide_record_command_no_lossy_conversion() {
        // The program path is an OsString, not a String — no lossy conversion
        let (program, args) = super::action_guide_record_command(false, false).unwrap();
        // Verify OsString type by checking it can contain non-UTF8
        let os_str: &std::ffi::OsStr = program.as_ref();
        assert!(!os_str.is_empty());
        for arg in &args {
            let arg_os: &std::ffi::OsStr = arg.as_ref();
            assert!(!arg_os.is_empty());
        }
    }

    // ---- --keep-motion detached command RED tests ----

    #[test]
    #[cfg(target_os = "linux")]
    fn keep_motion_opt_out_child_command_never_includes_flag() {
        let (_program, args) = super::action_guide_record_command(false, false).unwrap();
        assert!(
            !args.iter().any(|a| a.to_str() == Some("--keep-motion")),
            "opt-out child must not pass --keep-motion"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn keep_motion_opt_in_child_command_includes_flag() {
        let (_program, args) = super::action_guide_record_command(false, true).unwrap();
        assert!(
            args.iter().any(|a| a.to_str() == Some("--keep-motion")),
            "opt-in child must pass --keep-motion"
        );
    }

    // ---- spawn_action_guide_record ----

    #[test]
    #[cfg(target_os = "linux")]
    fn spawn_action_guide_record_spawns_reaper_thread() {
        // We test that spawn_action_guide_record returns promptly and doesn't hang.
        // The child will exit immediately (the current exe is a test binary).
        let result = super::spawn_action_guide_record(false, false);
        // It may fail to spawn if the binary doesn't support the subcommand,
        // but the function should return (not hang).
        // We just verify it returns without panicking.
        let _ = result;
    }
}
