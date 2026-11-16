# shcomp

`shcomp` manages shell completions for CLIs that already know how to emit them.

It is a broker, not a completion engine. It asks a tool for its completion
script, caches the script for the executable the shell actually resolved, and
installs a thin shim that dispatches to the right version. Nothing runs at
shell startup, and switching tool versions does not require regenerating or
reinstalling anything.

> Status: v0.0.1 targets zsh. bash, fish, Nushell and PowerShell are tracked as
> follow-up issues.

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
"packslip:github.com/dxkbroom/shcomp" = "0.0.1"
```

For local development, build from the checkout:

```sh
cargo install --path .
shcomp add gh
shcomp install
```

`shcomp add` accepts any reference the registry understands, and `--alias`
registers extra command names:

```sh
shcomp add cli/cli --alias github
shcomp add eza-community/eza --alias ll --alias lt
```

`shcomp install` writes one shim per command into
`${XDG_DATA_HOME}/shcomp/zsh/site-functions` and prints the one line to add
before `compinit`:

```zsh
fpath=("$XDG_DATA_HOME/shcomp/zsh/site-functions" $fpath)
```

Then restart the shell, or run `compinit` again.

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
```

Templates use Tera v2, the same syntax mise uses. `exec()` and `read_file()`
are deliberately unavailable: rendering a registry item must not run a
command. URL sources are fetched only by `shcomp install`; completion never
touches the network. A `sha256` is verified when present, and the cache records
the hash it fetched for a version-pinned URL.

## Commands

| Command | Purpose |
| --- | --- |
| `shcomp install` | Reconcile shims and fetch URL sources. |
| `shcomp uninstall` | Remove shcomp-owned shims. |
| `shcomp doctor` | Read-only checks; `--check` runs each generator. |
| `shcomp ls [--json]` | Selected tools, sources, and cache state. |
| `shcomp add <reference>` | Add a selection to the config. |
| `shcomp clean` | Remove the completion cache. |
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

## Known limitations in v0.0.1

- zsh only.
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
installed versions and `mise completion <shell> --tool <cmd>` to skip commands
whose completions mise already owns through its packslip loader.

## Roadmap

The follow-up issues are the plan beyond v0.0.1:

|Area|Issue|
|---|---|
|bash shims|#1|
|fish shims|#2|
|Nushell and PowerShell|#3|
|pacman and AUR helpers|#4|
|usage-spec broker|#5|
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

## License

MIT
