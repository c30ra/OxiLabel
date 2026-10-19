use std::{io, process::ExitCode};

fn main() -> ExitCode {
    let stdin = io::stdin();
    let result = oxilabel_cli::run(std::env::args(), &mut io::stdout(), &mut stdin.lock());
    if let Err(err) = &result {
        // clap's usage messages are complete; the others get the program's name.
        if oxilabel_cli::exit_code(&result) == 2 { eprint!("{err}") } else { eprintln!("oxilabel: {err}") }
    }
    ExitCode::from(oxilabel_cli::exit_code(&result))
}
