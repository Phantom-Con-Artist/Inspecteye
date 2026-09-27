use inspecteye::cli::run_cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(err) = run_cli() {
        eprintln!("Error: {err}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
