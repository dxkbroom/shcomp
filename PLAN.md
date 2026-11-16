# shcomp — v0.0.1 plan

shcomp manages shell completions for CLIs that already know how to emit them.
It is a broker and an orchestrator, not a completion engine: it never invents a
flag list. It asks the tool for its completion, caches the result per active
version, and installs a thin shim that dispatches to the version the shell
actually resolved.

This file records the design agreed before implementation. `GLOSSARY.md` holds
the vocabulary, and `docs/adr/` holds the decisions that are hard to reverse.

## Goals

- No completion generator runs at shell startup.
- Switching tool versions (mise, or any other manager) changes the completion
  without regenerating or reinstalling anything.
- One registry format and one install model per shell; zsh is the v0.0.1 target.
- The registry is data, not code: adding a well-known CLI should be a TOML file.
- `shcomp complete-word` can be driven from outside a shell, so integration
  tests can prove that two versions complete differently.

## Non-goals for v0.0.1

- bash, fish, nu and PowerShell shims (issues filed; the zsh design must not
  make them harder).
- Synthesizing completions from `--help` or from a usage spec (issues filed).
- Multi-file completion plugins that must be added to `fpath` (issue filed).
- Project-level configuration or trust (issue filed).
- Version predicates or eager prewarming (issues filed).

## Domain model

| Term | Meaning |
| --- | --- |
| registry item | One TOML document describing a workspace and its tools. |
| workspace | `owner/repo` on a host, GitHub by default. |
| tool | An executable owned by a workspace, with completion sources. |
| reference | What the user types in config: `gh`, `cli/cli`, `cli/cli/gh`, a URL, an SSH URL, or `gh:cli/cli/gh`. |
| selection | A tool declared in the global config's `[tools]` table. |
| source | How a completion is obtained: `run`, `path`, or `url`, optionally `kind = "bashcomp"`. |
| shim | The generated shell file that registers completion and dispatches by resolved binary. |
| load | Resolving the active binary to a cached completion script. |
| body | The vendor completion function captured after loading a script. |
| harness | Target-shell code that loads a completion and probes candidates for `complete-word`. |
| mise-owned | A tool whose completion mise's packslip loader already provides; shcomp skips it. |

## References

A reference is normalized before lookup.

| Input | Meaning |
| --- | --- |
| `gh` | Tool name or alias, unique across the registry. |
| `cli/cli` | Workspace; resolves to its `main` tool. |
| `cli/cli/gh` | A specific tool in a workspace. |
| `https://github.com/cli/cli` | Workspace. |
| `git@github.com:cli/cli.git` | Workspace. |
| `gh:cli/cli` | Workspace with an explicit GitHub host. |
| `gh:cli/cli/gh` | Tool with an explicit GitHub host. |

Hosts other than GitHub are reserved: `gitlab:` and `codeberg:` prefixes and
their URLs must fail with "unsupported host" rather than being guessed. A bare
`owner/repo` is inferred as GitHub.

Ambiguity is an error that lists the candidates. A workspace with more than one
tool and no `main` cannot be referenced as `owner/repo`.

## Config

Global only in v0.0.1: `${XDG_CONFIG_HOME}/shcomp/config.toml`, overridable with
`--config` or `SHCOMP_CONFIG`.

```toml
[tools]
gh = true
"cli/cli" = true
"cli/cli/gh" = true
"eza-community/eza" = { aliases = ["ll", "lt"] }

[settings]
timeout = "5s"
fetch = true

[[registry]]
repo = "acme/tool"
main = "acme"
description = "A private tool"

[[registry.tools]]
name = "acme"
aliases = ["at"]

[registry.tools.completions.zsh]
run = ["{{ bin }}", "completion", "{{ shell }}"]
```

- `[tools]` values are `true` or a table with `aliases`, `disabled`, and
  `shells`. They never contain a completion recipe.
- `[[registry]]` is a full registry item and uses the registry-item schema.
- Precedence, lowest first: embedded registry, user registry directory
  (`${XDG_CONFIG_HOME}/shcomp/registry/*.toml`, lexical order), inline
  `[[registry]]`. A higher layer replaces a lower one with a warning. A
  duplicate identity inside one layer is an error.

## Registry item

One file per workspace. Embedded files live in `registry/` and are named
`owner-repo.toml`; `repo` is authoritative, and user files may use any name
under the user registry directory.

```toml
repo = "cli/cli"
main = "gh"
description = "GitHub's official CLI"

[[tools]]
name = "gh"
aliases = ["github"]

[tools.completions.zsh]
run = ["{{ bin }}", "completion", "-s", "{{ shell }}"]
```

A source table has exactly one of `run`, `path`, or `url`, plus optional
`kind` and `timeout`:

```toml
[tools.completions.zsh]
run = ["{{ bin }}", "completion", "{{ shell }}"]     # argv, no shell
kind = "native"

[tools.completions.zsh]
path = "{{ root }}/share/completions/_{{ command }}"

[tools.completions.zsh]
url = "https://raw.githubusercontent.com/eza-community/eza/v{{ version }}/completions/zsh/_eza"
sha256 = "…"

[tools.completions.zsh]
kind = "bashcomp"
run = ["{{ bin }}", "generate-completion", "{{ shell }}"]
```

- `kind = "native"` is the default; `bashcomp` means the emitted zsh script
  relies on `bashcompinit`.
- `run` is an argv array. It is executed without a shell, so no quoting or
  metacharacter expansion is needed.
- `path` is read from disk; it is version-scoped through `{{ root }}`.
- `url` is fetched only by `shcomp install`, verified against `sha256` for
  embedded entries, and never fetched by `load` or by Tab completion.

## Templates

shcomp uses Tera v2 (the engine and version mise uses), so the syntax is
`{{ … }}`, `{% … %}`, `{# … #}` — not a private `{shell}` syntax.

Available variables:

| Variable | Value |
| --- | --- |
| `bin` | Absolute path of the resolved executable. |
| `root` | Install root of the resolved executable. |
| `version` | Resolved version, if it can be determined. |
| `command` | The executable name being completed. |
| `shell` | Target shell (`zsh` in v0.0.1). |
| `cword` | Index of the word being completed. |
| `line` | The words joined with spaces. |
| `words` | The words as a list. |
| `env` | The process environment. |
| `config_root` | Directory of the file that declared the template. |
| `config_source` | Path of that file. |

Pure Tera filters are available (`quote`, `lower`, `replace`, `join`,
`hash`, `join_path`, `default`, …) and the mise-style helpers `os()`,
`arch()`, `os_family()`, `get_env()`.

`exec()`, `read_file()`, `read_json()`, `read_yaml()` and `read_toml()` are
deliberately absent. In mise they are trust-gated and run at render time; a
completion registry must stay reproducible and must never execute a command as
a side effect of rendering. An issue tracks adding them behind a trust model.

## Runtime: zsh

Install directory: `${XDG_DATA_HOME}/shcomp/zsh/site-functions`, one
`_<command>` file per tool. The user adds one static line before `compinit`:

```zsh
fpath=("$XDG_DATA_HOME/shcomp/zsh/site-functions" $fpath)
```

A shim contains `#compdef <command> <aliases>`, defines the wrapper, and is
autoloaded by zsh only when the command is completed.

Warm path:

1. Read `$commands[<command>]`.
2. If it is the same binary and the cached script is not older than it, call
   `__shcomp_body_<command> "$@"`. No fork, no file read beyond the stat.
3. Otherwise run `shcomp load --shell zsh --command <command> --bin <path>`.
4. Source the returned script inside a nested function, so a top-level `return`
   cannot skip the restore.
5. Capture the function `_comps[<command>]` points at into
   `__shcomp_body_<command>`, restore the wrapper and `compdef`, then call the
   body.

`load` caches a script under `${XDG_CACHE_HOME}/shcomp/scripts/<shell>/<command>/<key>`,
where the key covers the binary's realpath, mtime, size, the source descriptor
and the shcomp version. An in-place upgrade changes the mtime, so the shim's
`-ot` check reloads it. A version switch changes the resolved path, so it gets
its own entry.

Every generated script is validated by signature before it is cached: zsh
scripts must contain `#compdef`, `compdef`, or `complete`. This catches tools
that exit 0 and print help (stable `cargo` with `CARGO_COMPLETE` does exactly
this).

`shcomp install` is idempotent reconciliation: resolve selections, skip
mise-owned commands, write missing shims, remove stale ones, fetch URL sources
for installed versions, and print the `fpath` line. There is no separate
`sync` command in v0.0.1; it would have no distinct goal.

### mise integration

- shcomp never calls mise on the warm completion path.
- The resolved executable path is the source of truth for the active version,
  which is exactly what mise's activation writes to `PATH`.
- `install` may call `mise ls --json` to enumerate installed versions for URL
  prefetching, and `mise completion <shell> --tool <cmd>` to detect a
  mise-owned command. Both are install-time only, and both are skipped when
  mise is absent.
- shcomp uses uniquely named functions (`__shcomp_*`) and its own install
  directory, so mise's packslip loader can override a command without either
  side corrupting the other.

## `shcomp complete-word`

```text
shcomp complete-word [--shell zsh] [--cword N] <tool[@version]> <words>...
```

This is orchestration, not a completion implementation. It resolves the tool
and its source, ensures the script is cached, then runs the target shell in a
controlled environment with a harness that:

1. starts as a clean zsh (`zsh -f`);
2. initializes the real completion system (`compinit`, and `bashcompinit` when
   the source is `bashcomp`);
3. installs the cached script into the harness's `fpath`;
4. sets `words`, `CURRENT`, `PREFIX`, `SUFFIX`, `compstate`, `state`;
5. invokes the completion function the shell registered;
6. captures candidates at `compadd` and prints one candidate per line.

The harness is target-shell code shipped with shcomp; Rust only prepares the
environment, runs it, enforces a timeout, and normalizes output. This is the
interface `mise x tool@version -- shcomp complete-word tool …` uses in tests:
the same harness against two cached version scripts must produce different
candidates.

Script-only sources work because the target shell is doing the completion.
There is no `candidates`-only restriction.

## Commands

| Command | Purpose |
| --- | --- |
| `shcomp install` | Reconcile shims, fetch URL sources, print setup. |
| `shcomp uninstall` | Remove shcomp-owned shims. |
| `shcomp load` | Internal: print the cached script path for a command/binary. |
| `shcomp complete-word` | Probe candidates through the target shell. |
| `shcomp doctor` | Read-only checks; `--check` runs each generator. |
| `shcomp ls` | Selections, sources, cache state, mise-owned state; `--json`. |
| `shcomp add <reference>` | Add a selection to the config with `toml_edit`. |
| `shcomp clean` | Remove the cache. |
| `shcomp schema [--registry]` | Print the JSON Schema. |
| `shcomp usage` | Print the usage KDL. |
| `shcomp completion <shell>` | usage-rs completion for shcomp itself. |

## Embedded registry (v0.0.1)

`cli/cli` (gh), `chezmoi-org/chezmoi` (chezmoi), `astral-sh/uv` (uv),
`rust-lang/cargo` (cargo, `CARGO_COMPLETE=zsh` with `+nightly`),
`android-sdk` (android), `television` (tv), `zellij-org/zellij` (zellij),
`herdrdev/herdr` (herdr), `dotnet/sdk` (dotnet, best effort),
`oven-sh/bun` (bun, URL), `eza-community/eza` (eza, URL),
`gradle/gradle` (gradle, URL from `gradle/gradle-completion`).

These cover every generator in the current zsh setup. `mise`, `usage`, `fzf`
and multi-file plugins are intentionally not registry items.

## Stack

1. `feat(foundation)` — crate, usage-rs CLI, config/registry types, Tera
   rendering, JSON Schema, glossary and ADRs.
2. `feat(registry)` — embedded/user/inline registry loading, reference
   resolution, `ls`.
3. `feat(sources)` — run/path/url/bashcomp sources, cache keys, `load`,
   `clean`, signature validation.
4. `feat(zsh)` — shim generation, `install`/`uninstall`, zsh e2e harness.
5. `feat(complete-word)` — zsh harness orchestration and mise-linked e2e
   fixtures.
6. `feat(doctor)` — mise deference, alias/stale checks, `add`.
7. `docs(release)` — README, AGENTS.md, rendered schemas, issues, `v0.0.1`.

## Acceptance for v0.0.1

- `shcomp install` on a clean config writes one shim per selected command and
  removes nothing it did not create.
- A warm Tab in zsh forks nothing.
- Switching `$PATH` between two fixture versions loads the matching cached
  script, and `shcomp complete-word` returns different candidates for each.
- A generator that exits 0 but prints help fails signature validation with a
  clear message.
- `shcomp doctor` reports stale aliases, missing shims, mise-owned commands,
  and invalid cached scripts.
- `shcomp schema` and `shcomp schema --registry` validate the shipped configs
  and registry items.
- The public README contains no personal paths, hosts, or dotfile names.
