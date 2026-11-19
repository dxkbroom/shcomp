# shcomp

`shcomp` manages shell completions for CLIs that already know how to emit them.

It is a broker, not a completion engine. It asks a tool for its completion
script, caches the script for the executable the shell actually resolved, and
installs a thin shim that dispatches to the right version. Nothing runs at
shell startup, and switching tool versions does not require regenerating or
reinstalling anything.

> **Pre-v1 software**
>
> shcomp is pre-v1. Its CLI, configuration, storage format, and behavior are
> not finalized.
>
> Breaking changes may land between releases, including changes that require
> existing configuration or cached data to be updated. Every break is listed in
> `CHANGELOG.md` under the version that makes it.

Status: v0.0.2 targets zsh and fish. bash, Nushell and PowerShell are tracked as
follow-up issues.

## Why

A completion file generated once goes stale in three ways:

- it is regenerated on every shell start, or not at all;
- it is tied to one shell's syntax and setup;
- it is tied to the tool version that happened to be active when it was
  generated.

`shcomp` separates the three. The registry says how a tool emits completion.
The config says which tools you use. The shim resolves the active executable at
completion time and loads the matching cached script.

## Install

The supported install path is packslip: it gives you the binary, the
version-matched man page, and shell completions.

```sh
mise use packslip:github.com/dxkbroom/shcomp --minimum-release-age "0s"
shcomp add gh
shcomp install
```

Or pin a version in `mise.toml`:

```toml
[tools]
"packslip:github.com/dxkbroom/shcomp" = "0.0.2"
```

For local development, build from the checkout:

```sh
cargo install --path .
shcomp add gh
shcomp install
```

Not sure what to call a tool? `shcomp search` matches against command names,
aliases, workspaces, and full references, across the embedded registry and your
own:

```sh
shcomp search jq
shcomp search --match-type equal cli/cli
shcomp search -i
```

With `-i`, the picker adds what you choose.

`shcomp add` accepts any reference the registry understands, and `--alias`
registers extra command names:

```sh
shcomp add cli/cli --alias github
shcomp add eza-community/eza --alias ll --alias lt
```

`add` finishes the job: it writes the selection, writes the shim, fetches any
URL source, and prints the loading line. `--no-install` writes only the
config, `--force` replaces the selection's aliases instead of adding to them,
and `-n/--dry-run` reports what it would do without touching anything.
`--dry-run-code` exits 1 when there is work, which is what a script wants to
know.

`shcomp install` writes one shim per selected command, plus shcomp's own, into
`${XDG_DATA_HOME}/shcomp/zsh/site-functions` and prints the one line to add
before `compinit`:

```zsh
fpath=("$XDG_DATA_HOME/shcomp/zsh/site-functions" $fpath)
```

Then restart the shell, or run `compinit` again.

For fish, the same install writes
`${XDG_DATA_HOME}/shcomp/fish/completions`, and the one line goes in
`~/.config/fish/config.fish`:

```fish
set -p fish_complete_path "$XDG_DATA_HOME/shcomp/fish/completions"
```

`install -s fish` (or `-s zsh`) narrows a run; `[settings].shells` sets the
default, which is both shells. A tool gets a fish shim only when its registry
item declares a fish source, so a zsh-only vendor script is never guessed at.

Unlike zsh, fish has no per-command completion function to capture, so each
addition costs one child fish per Tab — never an extra `shcomp`. That tradeoff
is ADR-0009.

shcomp's own completion ships with it: `shcomp install` writes `_shcomp` even
though there is no `shcomp` entry in your config. Once it is loaded,
`shcomp add <TAB>` completes every tool the registry knows — including items
from your own registry — with the description beside it. It inserts the tool
name, because that is what `add` and `complete-word` accept; when two entries
share a name, it inserts the full reference instead.
`shcomp search <TAB>` and `shcomp complete-word <TAB>` use the same corpus.

`shcomp ls` answers the questions you actually have about an entry: what it is
called (with its aliases), which workspace it came from, how its completion is
produced, whether the next Tab will be warm or cold, who owns it when mise also
provides one, and what the registry says it is. `--all` adds selections you have
disabled, and `--json` carries the same fields for scripts.

Every cached script has a sidecar `<key>.json` beside it that names the
command, shell, and binary it came from. `shcomp clean --stale` uses it to
remove scripts no Tab can reach any more — the binary is gone, it was rebuilt,
or the tool is no longer selected — and `-n` lists them first. `install` does
the same pruning quietly. `shcomp clean gh` and `shcomp clean -s fish` scope a
full removal; plain `shcomp clean` still removes the whole cache.

A Tab never prints shcomp's errors into your prompt, so `shcomp load` records
what each cold load did instead. `shcomp doctor` reads those records: a
generator that failed, with the first line it wrote to stderr, and a shell
whose shims are installed but have never loaded — usually a missing loading
line. The warm paths never call shcomp, so they never write anything.

## Config

`${XDG_CONFIG_HOME}/shcomp/config.toml` declares which tools you want. It does
not contain completion recipes; those belong to the registry.

```toml
[tools]
gh = true
"cli/cli" = true
"cli/cli/gh" = true
"eza-community/eza" = { aliases = ["ll", "lt"] }

[settings]
timeout = "5s"
```

A selection can also be disabled or limited to some shells:

```toml
[tools."gradle/gradle"]
disabled = true
shells = ["zsh"]
```

## Registry

A registry item describes one workspace and its tools. `shcomp` embeds a
curated set, discovers more under
`${XDG_CONFIG_HOME}/shcomp/registry/*.toml`, and accepts inline items in the
config with `[[registry]]`.

```toml
repo = "cli/cli"
main = "gh"
description = "GitHub's official command line tool"

[[tools]]
name = "gh"
aliases = ["github"]

[tools.completions.zsh]
run = ["{{ bin }}", "completion", "-s", "{{ shell }}"]
```

A tool whose version is not encoded in its install path can declare how to ask
it, which is what version-templated URLs use:

```toml
[[tools]]
name = "eza"

[tools.version]
run = ["{{ bin }}", "--version"]
# regex = 'Gradle ([0-9]+\.[0-9]+(?:\.[0-9]+)?)'
```

A tool can be referred to by bare name, workspace, workspace plus tool, GitHub
URL, SSH URL, or a host prefix:

```text
gh
cli/cli
cli/cli/gh
https://github.com/cli/cli
git@github.com:cli/cli.git
gh:cli/cli/gh
```

Sources are one of:

```toml
[tools.completions.zsh]
run = ["{{ bin }}", "completion", "{{ shell }}"]

[tools.completions.zsh]
path = "{{ root }}/share/completions/_{{ command }}"

[tools.completions.zsh]
url = "https://raw.githubusercontent.com/eza-community/eza/v{{ version }}/completions/zsh/_eza"
sha256 = "…"

[tools.completions.zsh]
kind = "bashcomp"
run = ["{{ bin }}", "generate-completion", "{{ shell }}"]

[tools.completions.zsh]
run = ["{{ bin }}", "+nightly"]
env = { CARGO_COMPLETE = "zsh" }

[tools.completions.zsh]
kind = "usage"
run = ["{{ bin }}", "--usage"]
```

A `kind = "usage"` source means the command prints a usage KDL spec, and the
`usage` CLI renders the completion from it — shcomp does not parse the spec
itself (ADR-0008). `usage` must be on `PATH` wherever that completion runs;
shcomp passes a binary-scoped cache key, so the spec command runs once per
installed version. The spec cache lives in `$XDG_CACHE_HOME/usage`, outside
`shcomp clean`. `--usage-bin` selects a different executable, and
`[settings].usage_bin` sets the default.

Templates use Tera v2, the same syntax mise uses. `exec()` and `read_file()`
are deliberately unavailable: rendering a registry item must not run a
command. URL sources are fetched only by `shcomp install`; completion never
touches the network. A `sha256` is verified when present, and the cache records
the hash it fetched for a version-pinned URL.

## Commands

| Command | Purpose |
| --- | --- |
| `shcomp install [-n] [-s <shell>] [--json]` | Reconcile zsh/fish shims (including shcomp's own) and fetch URL sources. |
| `shcomp uninstall` | Remove shcomp-owned shims. |
| `shcomp doctor` | Sectioned checks for zsh, fish, mise, the cache, and recent loads; `--check` runs each generator, `--json` for machines, exits 1 on an error. |
| `shcomp ls [--all] [--no-header] [--json]` | Selected tools, sources, cache state, and owner. |
| `shcomp search [name]` | Find a reference to add; `-i` picks from a menu. |
| `shcomp add [-n] [--no-install] <reference>` | Add a selection and reconcile its shim. |
| `shcomp clean [--stale] [-n] [-s <shell>] [tool...]` | Remove cached scripts: all of them, one tool's or shell's, or only the stale ones. |
| `shcomp complete-word` | Probe candidates through the target shell. |
| `shcomp schema [--registry]` | Print JSON Schema. |

The generated schemas live in `schema/config.json` and `schema/registry.json`;
`mise run render:schema` refreshes them.

## How version dispatch works

The shim reads the executable the shell resolved, asks `shcomp load` for a
cached script keyed by that executable's path, mtime and size, and stores the
completion function the vendor script registered. Later completions call that
function directly. A version switch is a different path; an in-place upgrade
changes the mtime; both cause one reload and nothing more.

`shcomp complete-word` is the external version of the same idea: it prepares
the cached script and runs the target shell under a controlled harness, then
reports the candidates the shell produced. This is how tests prove that two
versions complete differently:

```sh
mise x gh@2.104.0 -- shcomp complete-word gh@2.104.0 gh ""
mise x gh@2.103.3 -- shcomp complete-word gh@2.103.3 gh ""
```

`complete-word` takes the words after the command, and `--cword` when the
cursor is not on the last word:

```sh
shcomp complete-word gh gh co
shcomp complete-word gh gh --lo
```

It loads the cached script, starts an interactive zsh in a controlled PTY
harness, lets the real completion system run the vendor function, and reports
what the shell produced. shcomp does not reimplement completion.

## What ships

The embedded registry covers `gh`, `chezmoi`, `uv`, `cargo`, `android`, `tv`,
`zellij`, `herdr`, `dotnet`, `bun`, `eza`, `gradle`, and `just`. A tool can be
added by dropping a registry item into
`${XDG_CONFIG_HOME}/shcomp/registry/`, or by declaring one inline with
`[[registry]]`.

## Debugging

`-v` raises the log level (`-vv`, `-vvv` for more), and `-q` leaves errors
only. `SHCOMP_LOG` takes tracing directives when you want to narrow rather than
broaden:

```sh
SHCOMP_LOG=shcomp::cache=debug shcomp ls
```

`SHCOMP_LOG_FILE` (or `[settings].log_file`) appends the same events to a file,
which is how to see what happened during a Tab — the shell swallows stderr
there. Logs never go to stdout, so `shcomp load` and `shcomp completion` stay
script-safe.

## When a vendor completion is broken

shcomp brokers completions; it does not write them. If a Tab offers nothing or
prints an error, check the vendor script before assuming shcomp:

```sh
shcomp complete-word <command> <command> ""
shcomp doctor --check
```

A vendor zsh script sometimes writes an `_arguments` spec with its exclusion
group unquoted, as `(--no-build-cache)'--build-cache[…]'`. zsh reads the leading
`(` as a glob group, and under the default `NOMATCH` a word that matches
nothing aborts the whole completion — which is why gradle's completion failed
on flags with a negative form. shcomp's shim sets `no_nomatch` for the vendor
body, so the literal word reaches `_arguments` as the author intended. Other
vendor failures are reported by `doctor --check`, which runs one real completion
per tool and names the command that failed.

## Known limitations in v0.0.2

- zsh and fish only; bash, Nushell and PowerShell are follow-up issues.
- A vendor completion that needs several Tab presses to reach a different state
  can report no candidates; `complete-word` runs one completion pass.
- Candidate descriptions are captured when the vendor supplies them, but some
  completion styles discard them.
- File-only matches are reported, but fuzzy matching and menu selection are the
  shell's job, not shcomp's.
- `exec()` and file-reading template functions are intentionally unavailable.

## mise

`shcomp` does not require mise and never calls it on the completion path. When
mise is present, `install` may use `mise ls --json` to prefetch URL sources for
installed versions.

mise is not consulted about ownership: if you selected a tool in shcomp, shcomp
writes the shim, even when mise's packslip loader could complete it too.
`doctor` and `ls` say when mise also provides one (`owner: shcomp+mise`), so a
duplicate stub can be found and removed instead of silently competing. The
one exception is shcomp itself: installed through packslip, it always has a mise
stub, its own shim wins, and `doctor` reports that as `ok`.

## Roadmap

The follow-up issues are the plan beyond v0.0.2:

|Area|Issue|
|---|---|
|bash shims|#1|
|Nushell and PowerShell|#3|
|pacman and AUR helpers|#4|
|help-scraping broker|#6|
|project config and trust|#7|
|bash-only output adapter|#8|
|multi-file plugins|#9|
|post-install prewarm|#10|
|version predicates|#11|
|trusted `exec()` templates|#12|
|dangling helper functions|#13|
|multi-pass completions|#14|
|candidate descriptions|#15|
|URL checksum policy|#16|
|GitLab and other hosts|#17|
|fish zero-fork experiment|#49|

## License

MIT
