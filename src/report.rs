//! Shared reporting: severity, findings, and the table `ls` and `doctor` print.

use std::io::IsTerminal;

use comfy_table::{
    Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Table, Width, presets,
};

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
    // Styling and width are separate questions. comfy-table answers both from
    // one tty check, so `NO_COLOR`, `TERM=dumb`, or a pipe would also turn off
    // wrapping. Decide styling here and hand it the width explicitly.
    table.force_no_tty();
    if colors_enabled() {
        table.enforce_styling();
    }
    if let Some(width) = output_width() {
        table.set_width(width);
    }
    if !no_header {
        let header = headers.iter().map(|title| header_cell(title));
        table.set_header(header);
    }
    table
}

/// The width a table may fill: an exported `COLUMNS` first, so a pipe or a
/// script can ask for one, then the size of the terminal on stdout. A pipe
/// without `COLUMNS` gets `None`, so each row stays on one line for `grep` and
/// `awk`.
pub fn output_width() -> Option<u16> {
    width_from(std::env::var("COLUMNS").ok().as_deref()).or_else(|| Table::new().width())
}

fn width_from(columns: Option<&str>) -> Option<u16> {
    columns
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|width| *width > 0)
}

/// Size the columns to the table's width so that wrapping only ever breaks at
/// spaces.
///
/// comfy-table's dynamic arrangement shares the width out without regard to
/// words, so it splits `eza-community/eza` as `eza-communit` / `y/eza`, and its
/// lower boundaries can still overflow the width by a column. Instead, start
/// every column at its longest word, then let the columns that need the least
/// extra room become whole first, so short fields stay on one line and the
/// longest text (a description, a message) absorbs the wrapping.
///
/// When even the longest words do not fit, the `droppable` columns are hidden
/// in order until they do; if they still do not, the table is as narrow as it
/// can be without splitting a word.
fn fit_columns(table: &mut Table, droppable: &[usize]) {
    let Some(width) = table.width() else {
        return;
    };
    let mut longest_word: Vec<usize> = Vec::new();
    let mut longest_line: Vec<usize> = Vec::new();
    let header = table
        .header()
        .map(|row| row.cell_iter().cloned().collect::<Vec<_>>());
    let rows = table
        .row_iter()
        .map(|row| row.cell_iter().cloned().collect::<Vec<_>>());
    for cells in header.into_iter().chain(rows) {
        for (index, cell) in cells.iter().enumerate() {
            if longest_word.len() <= index {
                longest_word.resize(index + 1, 0);
                longest_line.resize(index + 1, 0);
            }
            let content = cell.content();
            let word = content
                .split_whitespace()
                .map(text_width)
                .max()
                .unwrap_or(0);
            let line = content.lines().map(text_width).max().unwrap_or(0);
            longest_word[index] = longest_word[index].max(word);
            longest_line[index] = longest_line[index].max(line);
        }
    }
    let padding: Vec<usize> = table
        .column_iter()
        .map(|column| usize::from(column.padding_width()))
        .collect();
    let mut visible: Vec<usize> = (0..longest_line.len()).collect();
    let needs = |visible: &[usize], widths: &[usize]| -> usize {
        visible
            .iter()
            .map(|&index| widths[index] + padding.get(index).copied().unwrap_or(0))
            .sum()
    };
    let width = usize::from(width);
    if needs(&visible, &longest_line) <= width {
        return;
    }
    let mut hidden = Vec::new();
    for &index in droppable {
        if needs(&visible, &longest_word) <= width {
            break;
        }
        visible.retain(|&column| column != index);
        hidden.push(index);
    }
    let budget = width.saturating_sub(needs(&visible, &vec![0; longest_line.len()]));
    let min: Vec<usize> = visible.iter().map(|&index| longest_word[index]).collect();
    let max: Vec<usize> = visible.iter().map(|&index| longest_line[index]).collect();
    let widths = share(&min, &max, budget);
    for (&index, content) in visible.iter().zip(widths) {
        if let Some(column) = table.column_mut(index) {
            let total = u16::try_from(content)
                .unwrap_or(u16::MAX)
                .saturating_add(column.padding_width());
            column.set_constraint(ColumnConstraint::Absolute(Width::Fixed(total)));
        }
    }
    for index in hidden {
        if let Some(column) = table.column_mut(index) {
            column.set_constraint(ColumnConstraint::Hidden);
        }
    }
}

/// Column content widths within `budget`: each at least `min`, at most `max`,
/// with the spare room going to the columns that are closest to whole.
fn share(min: &[usize], max: &[usize], budget: usize) -> Vec<usize> {
    let mut widths = min.to_vec();
    let mut spare = budget.saturating_sub(min.iter().sum());
    let mut order: Vec<usize> = (0..min.len()).collect();
    order.sort_by_key(|&index| max[index].saturating_sub(min[index]));
    for index in order {
        let give = spare.min(max[index].saturating_sub(widths[index]));
        widths[index] += give;
        spare -= give;
    }
    widths
}

fn text_width(text: &str) -> usize {
    text.chars().count()
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
///
/// Columns never split a word; see [`fit_columns`].
pub fn print(table: &mut Table) {
    print_dropping(table, &[]);
}

/// [`print`], hiding the `droppable` columns, in order, when the table cannot
/// otherwise fit its width without splitting a word.
pub fn print_dropping(table: &mut Table, droppable: &[usize]) {
    let rendered = render(table, droppable);
    for line in rendered.lines() {
        println!("{}", line.trim_end());
    }
}

fn render(table: &mut Table, droppable: &[usize]) -> String {
    fit_columns(table, droppable);
    table.to_string()
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

    fn ls_like(width: u16) -> Table {
        let mut table = table(
            &[
                "Tool",
                "Repo",
                "Source",
                "Shells",
                "Cache",
                "Owner",
                "Description",
            ],
            false,
        );
        table.set_width(width);
        let rows = [
            [
                "gh (github)",
                "cli/cli",
                "run",
                "bash, fish, pwsh, zsh",
                "cold",
                "shcomp",
                "GitHub's official command line tool",
            ],
            [
                "eza",
                "eza-community/eza",
                "url",
                "bash, fish, zsh",
                "cold",
                "shcomp",
                "A modern replacement for ls",
            ],
            [
                "chezmoi",
                "twpayne/chezmoi",
                "run",
                "bash, fish, nushell, zsh",
                "missing",
                "shcomp",
                "Manage your dotfiles across multiple machines",
            ],
            [
                "shcomp (implicit)",
                "dxkbroom/shcomp",
                "run",
                "fish, zsh",
                "cold",
                "shcomp",
                "Manage shell completions that follow the active tool version",
            ],
        ];
        for row in rows {
            table.add_row(row);
        }
        table
    }

    #[test]
    fn a_table_fits_80_columns_without_splitting_words() {
        let rendered = render(&mut ls_like(80), &[6]);
        for line in rendered.lines() {
            assert!(line.trim_end().chars().count() <= 80, "{rendered}");
        }
        let words: Vec<&str> = rendered.split_whitespace().collect();
        for whole in [
            "eza-community/eza",
            "twpayne/chezmoi",
            "dxkbroom/shcomp",
            "nushell,",
            "(implicit)",
            "completions",
        ] {
            assert!(words.contains(&whole), "`{whole}` was split:\n{rendered}");
        }
    }

    #[test]
    fn a_column_is_dropped_before_a_word_is_split() {
        let rendered = render(&mut ls_like(60), &[6, 2]);
        for line in rendered.lines() {
            assert!(line.trim_end().chars().count() <= 60, "{rendered}");
        }
        assert!(!rendered.contains("Description"), "{rendered}");
        assert!(
            rendered
                .split_whitespace()
                .any(|word| word == "eza-community/eza"),
            "{rendered}"
        );
    }

    #[test]
    fn spare_room_goes_to_the_columns_closest_to_whole() {
        // The short column becomes whole; the long one takes what is left.
        assert_eq!(share(&[4, 10], &[6, 40], 20), vec![6, 14]);
        assert_eq!(share(&[4, 10], &[6, 40], 5), vec![4, 10]);
    }

    #[test]
    fn columns_parses_a_positive_width() {
        assert_eq!(width_from(Some("80")), Some(80));
        assert_eq!(width_from(Some(" 120 ")), Some(120));
        assert_eq!(width_from(Some("0")), None);
        assert_eq!(width_from(Some("wide")), None);
        assert_eq!(width_from(None), None);
    }

    #[test]
    fn a_headerless_table_has_no_header_row() {
        let mut table = table(&["Section", "Status"], true);
        table.add_row(["zsh", "ok"]);
        let rendered = table.to_string();
        assert!(!rendered.contains("Section"), "{rendered}");
    }
}
