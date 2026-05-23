pub mod args;
pub mod cli_error;
pub mod cmd_capture;
pub mod cmd_capture_launcher;
pub mod cmd_probe;
pub mod cmd_stitch_folder;
mod frame_slot;

use clap::Parser;

pub use cli_error::CliError;

use crate::args::{Cli, Command};

pub fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| {
        use clap::error::ErrorKind;
        let msg = err.to_string();
        match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => CliError::stdout(msg, 0),
            _ => CliError::new(msg, 1),
        }
    })?;

    match &cli.command {
        Command::Capture(a) => cmd_capture::run(a),
        Command::Probe(a) => cmd_probe::run(a),
        Command::StitchFolder(a) => cmd_stitch_folder::run(a),
    }
}
