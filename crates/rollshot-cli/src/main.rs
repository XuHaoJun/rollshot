use std::process::ExitCode;

fn main() -> ExitCode {
    match rollshot_cli::run(std::env::args()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
