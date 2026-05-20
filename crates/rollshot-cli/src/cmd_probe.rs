use crate::args::ProbeArgs;
use crate::cli_error::CliError;

pub fn run(_args: &ProbeArgs) -> Result<String, CliError> {
    Ok("probe: not implemented yet\n".to_string())
}
