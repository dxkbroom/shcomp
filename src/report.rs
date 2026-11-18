//! Shared reporting: severity, findings, and the table `ls` and `doctor` print.

use std::io::IsTerminal;

use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets};

/// How bad a finding is. Only `Error` changes an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Ok,
    Warn,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// One line of a report, attributed to the section it belongs to.
#[derive(Debug, Clone)]
pub struct Finding {
    pub section: &'static str,
    pub severity: Severity,
    pub message: String,
}

impl Finding {
    pub fn ok(section: &'static str, message: impl Into<String>) -> Self {
        Self {
            section,
            severity: Severity::Ok,
            message: message.into(),
        }
    }

    pub fn warn(section: &'static str, message: impl Into<String>) -> Self {
        Self {
            section,
            severity: Severity::Warn,
            message: message.into(),
        }
    }

    pub fn error(section: &'static str, message: impl Into<String>) -> Self {
        Self {
            section,
            severity: Severity::Error,
            message: message.into(),
        }
    }
}

/// Whether to style output: a real terminal, not `TERM=dumb`, no `NO_COLOR`.
pub fn colors_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|term| term != "dumb")
        && std::io::stdout().is_terminal()
}

/// A borderless table with the same header treatment mise uses.
pub fn table(headers: &[&str], no_header: bool) -> Table {
    let mut table = Table::new();
    table
        .load_style(presets::NOTHING)
        .set_content_arrangement(ContentArrangement::Dynamic);
    if colors_enabled() {
        table.enforce_styling();
    } else {
        table.force_no_tty();
    }
    if !no_header {
        let header = headers.iter().map(|title| header_cell(title));
        table.set_header(header);
    }
    table
}

fn header_cell(title: &str) -> Cell {
    let cell = Cell::new(title);
    if colors_enabled() {
        cell.add_attribute(Attribute::Italic).fg(Color::Magenta)
    } else {
        cell
    }
}

pub fn severity_cell(severity: Severity) -> Cell {
    let cell = Cell::new(severity.as_str());
    if !colors_enabled() {
        return cell;
    }
    match severity {
        Severity::Ok => cell.fg(Color::Green),
        Severity::Warn => cell.fg(Color::Yellow),
        Severity::Error => cell.fg(Color::Red),
    }
}

/// Print a rendered table with trailing whitespace removed.
///
/// `comfy-table` pads every cell to its column width, so the last column
/// otherwise leaves trailing spaces on every row that is not the widest.
pub fn print(table: &Table) {
    let rendered = table.to_string();
    for line in rendered.lines() {
        println!("{}", line.trim_end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_errors_fail_a_check() {
        assert!(!Severity::Ok.is_error());
        assert!(!Severity::Warn.is_error());
        assert!(Severity::Error.is_error());
    }

    #[test]
    fn a_headerless_table_has_no_header_row() {
        let mut table = table(&["Section", "Status"], true);
        table.add_row(["zsh", "ok"]);
        let rendered = table.to_string();
        assert!(!rendered.contains("Section"), "{rendered}");
    }
}
