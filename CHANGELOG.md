# Changelog

## Unreleased (0.1.0)

shcomp is pre-v1: this release settles the CLI and is allowed to break v0.0.1.
The full rationale is in `PLAN.md` under "v0.1.0 plan"; this is the list a user
has to act on.

### Breaking changes

- `shcomp ls` prints a table with `Tool`, `Repo`, `Source`, `Cache`, `Owner`, and
  `Description` columns, and gains `--no-header` and `--all`. `--json` replaces
  the flat `cached` / `mise_owned` booleans with `cache { state, script, key }`
  and `owner`.
- `shcomp completion <shell> --install` is removed. `shcomp install` owns
  shcomp's own completion entry, in the same directory as every other shim;
  `shcomp completion <shell>` still prints a script.
- `shcomp doctor` exits 1 when a check fails, and reports sections instead of
  prose lines.
- `shcomp add` reconciles by default: it writes the shim and fetches URL
  sources, like `mise use`. Pass `--no-install` for the v0.0.1 behavior. It
  gains `-n/--dry-run`, `--dry-run-code`, and `-f/--force`, which replaces a
  selection's aliases instead of merging them.
- `shcomp install` prints a status table per tool and gains `-n/--dry-run` and
  `--json`.
- shcomp installs its own completion without a config entry, and `owner` in
  `ls` is `shcomp` or `shcomp+mise` rather than `mise`.
- Commands mise installed from a packslip are no longer skipped: shcomp writes
  its shim for every selected tool, and `doctor` reports the overlap so a
  duplicate mise stub can be removed.
- A registry source may declare `kind = "usage"` (later in this release),
  `[settings]` gains `log` and `log_file`, and `-v`/`-q`, `SHCOMP_LOG`, and
  `SHCOMP_LOG_FILE` control tracing.

## v0.0.1

First tagged release. zsh only.

- A curated embedded registry for `gh`, `chezmoi`, `uv`, `cargo`, `android`,
  `tv`, `zellij`, `herdr`, `dotnet`, `bun`, `eza`, `gradle`, and `just`.
- User registry items under `$XDG_CONFIG_HOME/shcomp/registry/` and inline
  `[[registry]]` items.
- Tool references by name, workspace, workspace/tool, GitHub URL, SSH URL, or
  `gh:` prefix.
- A version-dispatching zsh shim: one autoloaded file per command, no process
  at startup, no fork on a warm Tab.
- `shcomp install`, `uninstall`, `doctor`, `ls`, `add`, `clean`,
  `complete-word`, `schema`, `usage`, and `completion`.
- URL sources fetched only at install, with optional `sha256` verification.
- Registry-declared version discovery for tools whose install path does not
  carry the version.
- `complete-word` orchestrates a real zsh completion context through a PTY
  harness; it does not reimplement completion.
