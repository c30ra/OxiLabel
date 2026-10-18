use std::{io, process::ExitCode};

fn main() -> ExitCode {
    let stdin = io::stdin();
    let result = rtag_cli::run(std::env::args(), &mut io::stdout(), &mut stdin.lock());
    if let Err(err) = &result {
        // clap's usage messages are complete; the others get the program's name.
        if rtag_cli::exit_code(&result) == 2 { eprint!("{err}") } else { eprintln!("rtag: {err}") }
    }
    ExitCode::from(rtag_cli::exit_code(&result))
}
