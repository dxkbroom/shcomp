//! Tracing setup: stderr by default, an optional file, filtered by environment.
//!
//! `SHCOMP_LOG` takes the usual `tracing` directives, so a specific module can
//! be turned up without turning everything up. `-v` is a shorthand for the
//! whole crate, and `-q` leaves errors only. `SHCOMP_LOG_FILE` (or the
//! `log_file` setting) appends the same events without color, which is the only
//! way to see what happened inside a Tab: the shell swallows stderr there.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::Settings;

static FILTER: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> = OnceLock::new();
static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Install the global subscriber. Safe to call more than once; later calls are
/// ignored, which is what a test binary wants.
pub fn init(verbose: u8, quiet: bool) {
    let filter = EnvFilter::try_from_env("SHCOMP_LOG")
        .unwrap_or_else(|_| EnvFilter::new(directive(verbose, quiet)));
    let (filter_layer, handle) = reload::Layer::new(filter);
    let _ = FILTER.set(handle);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(crate::report::colors_enabled())
        .with_target(false);
    // The file layer is always present and discards until a path is configured,
    // so enabling a log file later does not need a second subscriber.
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(FileSink)
        .with_ansi(false)
        .with_target(true);
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();

    if let Some(path) = std::env::var_os("SHCOMP_LOG_FILE") {
        set_file(Path::new(&path));
    }
}

/// Apply the persistent settings, unless the environment already said better.
pub fn configure(settings: &Settings) {
    if let Some(directive) = &settings.log
        && std::env::var_os("SHCOMP_LOG").is_none()
        && let Some(handle) = FILTER.get()
    {
        let _ = handle.reload(EnvFilter::new(directive.clone()));
    }
    if let Some(path) = &settings.log_file {
        set_file(path);
    }
}

fn directive(verbose: u8, quiet: bool) -> String {
    match (verbose, quiet) {
        (_, true) => "shcomp=error".to_string(),
        (0, false) => "shcomp=warn".to_string(),
        (1, false) => "shcomp=info".to_string(),
        (2, false) => "shcomp=debug".to_string(),
        _ => "shcomp=trace".to_string(),
    }
}

fn set_file(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = File::options().create(true).append(true).open(path);
    let slot = LOG_FILE.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = slot.lock() {
        *slot = file.ok();
    }
}

/// A writer that appends to the configured log file, or discards.
struct FileSink;

impl<'a> MakeWriter<'a> for FileSink {
    type Writer = FileSinkWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FileSinkWriter
    }
}

struct FileSinkWriter;

impl Write for FileSinkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(slot) = LOG_FILE.get() else {
            return Ok(buf.len());
        };
        let Ok(mut slot) = slot.lock() else {
            return Ok(buf.len());
        };
        match slot.as_mut() {
            Some(file) => file.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let Some(slot) = LOG_FILE.get() else {
            return Ok(());
        };
        let Ok(mut slot) = slot.lock() else {
            return Ok(());
        };
        match slot.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_map_to_directives() {
        assert_eq!(directive(0, false), "shcomp=warn");
        assert_eq!(directive(1, false), "shcomp=info");
        assert_eq!(directive(2, false), "shcomp=debug");
        assert_eq!(directive(3, false), "shcomp=trace");
        assert_eq!(directive(0, true), "shcomp=error");
    }

    #[test]
    fn the_file_sink_discards_until_a_path_is_set() {
        let mut sink = FileSinkWriter;
        assert!(sink.write(b"nothing").is_ok());
        assert!(sink.flush().is_ok());
    }
}
