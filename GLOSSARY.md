# Glossary

This file is a glossary, not a specification. It defines the words shcomp uses
so that code, issues, and documentation mean the same thing.

## Completion

The candidates a shell offers for a partial command line. shcomp does not
compute them; the tool's own completion does.

## Registry item

A single TOML document describing one workspace and the tools it owns, including
how each tool's completion is obtained. One item may describe several tools.

## Workspace

The repository identity of a registry item: an owner and a repository on a host,
such as `cli/cli` on GitHub. A workspace groups tools; it is not an executable.

## Tool

One executable owned by a workspace, such as `gh` in `cli/cli`. A tool has a
name, optional aliases, and a completion source for each shell.

## Reference

The string a user writes to identify a tool: a bare name, a workspace, a
workspace plus a tool, a repository URL, or a host-prefixed form such as
`gh:cli/cli/gh`.

## Selection

A tool the user declares in the global config's `[tools]` table. A selection is
a wish, not a recipe.

## Source

The way a completion is obtained: by running a command, reading a file, or
fetching a URL. A source belongs to a tool and a shell.

## Shim

A small shell file installed where the shell looks for completions. It
registers the command and dispatches to the completion for the executable the
shell currently resolves.

## Load

The act of resolving the active executable to its cached completion script. The
active executable's path is the version key.

## Body

The completion function a vendor script defines, captured after loading so the
shim can call it directly on later completions.

## Harness

Target-shell code that loads a completion and collects candidates for
`complete-word`. The harness uses the shell's real completion system; shcomp
orchestrates it.

## Owner

Who writes the completion for a command: `shcomp`, or `mise` when mise's
packslip loader provides one. shcomp does not yield a command it was asked to
manage; the field exists so a duplicate stub can be found and removed.

## Implicit selection

A selection shcomp installs without a config entry. shcomp's own entry is the
only one: it follows from using shcomp, not from asking for it.

## Spec source

A source whose completion is rendered from a usage KDL specification by the
`usage` CLI. shcomp supplies the spec command and a version-scoped cache key and
caches the rendered script; it does not parse the spec.

## Cache state

Whether the next Tab will fork: `warm` (a cached script for the resolved
binary), `cold` (resolvable, not generated yet), `missing` (no binary on PATH),
or `n/a` (no source for that shell).

## Warm completion

A completion that uses an already-loaded body: no process is started and no
script is read.

## Cold completion

The first completion for a tool in a shell session, or the first after its
executable changes. It may run the generator once and cache the result.
