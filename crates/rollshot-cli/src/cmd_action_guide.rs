//! `rollshot action-guide` — launch the Action Guide input-capability probe
//! (P0b). Spawns the separate `rollshot-app` GUI binary in probe mode. Replaced
//! by the full overlay → record → review → export flow in the app-integration
//! plan.

use crate::args::ActionGuideArgs;
use crate::cli_error::CliError;

pub fn run(_args: &ActionGuideArgs) -> Result<String, CliError> {
    // The app binary must be built with `--features action-guide`, or it will
    // reject `--action-guide-probe` with a clear "unknown argument" error.
    let app = crate::cmd_capture_launcher::resolve_app_binary()?;
    let status = std::process::Command::new(&app)
        .arg("--action-guide-probe")
        .status()
        .map_err(|e| CliError::new(format!("failed to launch {}: {e}", app.display()), 1))?;

    if status.success() {
        Ok("action guide input probe completed".to_string())
    } else {
        Err(CliError::new("action guide input probe failed", 1))
    }
}
