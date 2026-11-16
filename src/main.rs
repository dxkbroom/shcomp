use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use shcomp::cli::Cli;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let argv: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    match Cli::parse_from_argv(&argv) {
        Ok(cli) => match cli.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("shcomp: {err}");
                for cause in err.chain().skip(1) {
                    eprintln!("  caused by: {cause}");
                }
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            if let usage_rs::Error::Version { .. } = error {
                println!("shcomp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            usage_rs::__usage_exit_on_error(
                error,
                Cli::spec(),
                Cli::spec().root.cmd,
                &argv,
                &argv[1..],
                None,
            );
        }
    }
}
