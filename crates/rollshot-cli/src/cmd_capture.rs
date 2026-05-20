use crate::args::CaptureArgs;
use crate::cli_error::CliError;

pub fn run(_args: &CaptureArgs) -> Result<String, CliError> {
    Err(CliError::new(
        "rollshot capture is not implemented yet (filled in by later tasks)",
        1,
    ))
}
