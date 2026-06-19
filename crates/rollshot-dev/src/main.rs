use std::process::ExitCode;

use rollshot_dev::cli_error::Stream;

fn main() -> ExitCode {
    match rollshot_dev::run(std::env::args_os()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            match error.stream {
                Stream::Stdout => print!("{}", error.message),
                Stream::Stderr => eprintln!("{}", error.message),
            }
            ExitCode::from(error.exit_code)
        }
    }
}
