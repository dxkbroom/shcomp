//! `shcomp edit`: edit the config in `$VISUAL`/`$EDITOR`, safely.
//!
//! The edit happens on a copy. The result is parsed, validated, and resolved
//! against the registry before it replaces the config, so a typo never leaves
//! shcomp unable to start; the diff is printed, and the shims are reconciled
//! like `install` would.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, WrapErr, bail};

use crate::config::Config;
use crate::registry::Registry;

/// What a new config starts with.
const NEW_CONFIG: &str = "# shcomp: tools to complete. See `shcomp schema`.\n[tools]\n";

#[derive(Debug, usage_rs::Args)]
pub struct Edit {
    /// Write the config but install nothing
    #[usage(long)]
    pub no_install: bool,

    /// Do not generate scripts now; the first Tab generates them
    #[usage(long)]
    pub no_prewarm: bool,
}

impl Edit {
    pub fn run(self) -> Result<()> {
        let path = crate::dirs::config_file()?;
        let original = if path.exists() {
            std::fs::read_to_string(&path)
                .wrap_err_with(|| format!("cannot read `{}`", path.display()))?
        } else {
            String::new()
        };
        let draft = draft_path(&path);
        if let Some(parent) = draft.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let start = if original.is_empty() {
            NEW_CONFIG
        } else {
            &original
        };
        std::fs::write(&draft, start)?;

        let edited = loop {
            run_editor(&draft)?;
            let edited = std::fs::read_to_string(&draft)?;
            if edited == original || (original.is_empty() && edited == NEW_CONFIG) {
                let _ = std::fs::remove_file(&draft);
                println!("no changes to {}", path.display());
                return Ok(());
            }
            match check(&edited, &path) {
                Ok(()) => break edited,
                Err(error) => {
                    eprintln!("shcomp: the edited config is invalid: {error:#}");
                    if ask("edit again? [Y/n] ") {
                        continue;
                    }
                    bail!(
                        "{} is unchanged; your edit is kept in {}",
                        path.display(),
                        draft.display()
                    );
                }
            }
        };

        print!("{}", diff(&original, &edited));
        crate::shell::write_atomic(&path, edited.as_bytes())?;
        let _ = std::fs::remove_file(&draft);
        println!("updated {}", path.display());

        if self.no_install {
            return Ok(());
        }
        let config = Config::load(&path)?;
        crate::cli::install::Install {
            no_prewarm: self.no_prewarm,
            ..Default::default()
        }
        .run_with(&config)
    }
}

/// The copy the editor works on: beside the config, so an editor that
/// detects TOML by name still does, and a rename stays on one filesystem.
fn draft_path(config: &Path) -> PathBuf {
    let name = config
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    config.with_file_name(format!(".{name}.edit-{}.toml", std::process::id()))
}

/// `$VISUAL`, then `$EDITOR`, then `vi`. The value may carry arguments
/// (`code --wait`), split like a shell would.
fn run_editor(path: &Path) -> Result<()> {
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    let words = crate::inline::split_words(&editor)?;
    let Some((program, args)) = words.split_first() else {
        bail!("the editor `{editor}` is empty");
    };
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .wrap_err_with(|| format!("cannot start the editor `{editor}`"))?;
    if !status.success() {
        bail!(
            "the editor exited with {status}; the config is unchanged and your edit is kept in {}",
            path.display()
        );
    }
    Ok(())
}

/// Parse, validate, and resolve every selection: what any later command would
/// do with this config.
fn check(text: &str, path: &Path) -> Result<()> {
    let mut config: Config = toml::from_str(text).wrap_err("not valid shcomp TOML")?;
    config.source = Some(path.to_path_buf());
    config.validate()?;
    let registry = Registry::load(&config)?;
    registry.selections(&config)?;
    Ok(())
}

/// Ask a yes/no question on the terminal; anything but a terminal is "no".
fn ask(question: &str) -> bool {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return false;
    }
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if stdin.lock().read_line(&mut answer).is_err() {
        return false;
    }
    !matches!(answer.trim(), "n" | "N" | "no" | "No")
}

/// A line diff: `-` for removed lines, `+` for added ones, unchanged lines
/// omitted except one line of context around each change.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // Longest common subsequence, by suffix lengths.
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // Removals before additions, as a reader expects.
            ops.push(('-', a[i]));
            i += 1;
        } else {
            ops.push(('+', b[j]));
            j += 1;
        }
    }
    let changed: Vec<bool> = ops.iter().map(|(op, _)| *op != ' ').collect();
    let near = |index: usize| {
        changed[index]
            || (index > 0 && changed[index - 1])
            || changed.get(index + 1).copied().unwrap_or(false)
    };
    let mut out = String::new();
    let mut skipped = false;
    for (index, (op, line)) in ops.iter().enumerate() {
        if near(index) {
            if skipped && !out.is_empty() {
                out.push_str("  …\n");
            }
            skipped = false;
            out.push_str(&format!("{op} {line}\n"));
        } else {
            skipped = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_shows_changes_with_one_line_of_context() {
        let old = "a\nb\nc\nd\ne\nf\n";
        let new = "a\nb\nC\nd\ne\nf\ng\n";
        assert_eq!(diff(old, new), "  b\n- c\n+ C\n  d\n  …\n  f\n+ g\n");
        assert_eq!(diff("", "x\n"), "+ x\n");
        assert_eq!(diff("same\n", "same\n"), "");
    }

    #[test]
    fn a_config_must_resolve_to_be_written() {
        let path = Path::new("/nonexistent/config.toml");
        assert!(check("[tools]\n", path).is_ok());
        assert!(check("[tools\n", path).is_err());
        assert!(check("[tools]\n\"no/such-tool-anywhere\" = true\n", path).is_err());
        assert!(check("[settings]\nbogus = 1\n", path).is_err());
    }
}
