use inspecteye::cli::run_cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    exit_quietly_on_closed_stdout();
    run_cli()
}

/// `println!` panics when stdout is closed early (`inspecteye ... | head`).
/// A reader that stopped reading is not an error, so exit without a panic message.
fn exit_quietly_on_closed_stdout() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if message.starts_with("failed printing to stdout") {
            std::process::exit(0);
        }
        default_hook(info);
    }));
}
