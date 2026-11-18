use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use shcomp::cli::Cli;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();

    // The completion script calls back with a hidden request rather than a
    // command anybody runs, so answer it before parsing: the tables it reads
    // are the ones the parse would build, and nothing else needs to load.
    let completion_argv: Vec<OsString> = args.iter().skip(1).cloned().collect();
    if let Some(answer) = Cli::completion_request(&completion_argv) {
        print!("{answer}");
        return ExitCode::SUCCESS;
    }

    let argv: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    match Cli::parse_from_argv(&argv) {
        Ok(cli) => {
            shcomp::logging::init(cli.verbose, cli.quiet);
            // The config may raise or redirect the log; a broken one is the
            // command's error to report, not logging's.
            if let Ok(config) = shcomp::config::Config::load_default() {
                shcomp::logging::configure(&config.settings);
            }
            match cli.run() {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("shcomp: {err}");
                    for cause in err.chain().skip(1) {
                        eprintln!("  caused by: {cause}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
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
