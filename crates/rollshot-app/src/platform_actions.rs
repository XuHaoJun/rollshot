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

#[allow(dead_code)]
fn open_command(path: &Path) -> Result<PlatformCommand, String> {
    #[cfg(target_os = "macos")]
    return Ok(PlatformCommand::new("open").arg(path));
    #[cfg(target_os = "linux")]
    return Ok(PlatformCommand::new("xdg-open").arg(path));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("open is not supported on this platform".to_string())
    }
}

fn run_command_with(
    command: &PlatformCommand,
    spawn: impl FnOnce(&PlatformCommand) -> std::io::Result<()>,
) -> Result<(), String> {
    spawn(command).map_err(|error| format!("open failed: {error}"))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
}
