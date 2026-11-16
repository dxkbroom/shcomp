# Repository Agent Guide

`shcomp` manages shell completions that follow the active tool version. It is
a broker: it asks a tool for its completion, caches it per executable, and
installs a shim that dispatches at completion time. It is not a completion
engine.

`PLAN.md` is the design record. `GLOSSARY.md` defines the domain vocabulary.
`docs/adr/` records decisions that are hard to reverse.

## Commands

- `mise run build` / `mise run b` - build the crate
- `mise run test` / `mise run t` - all tests
- `mise run test:unit` - `cargo test --all-features --workspace`
- `mise run test:e2e --all` - end-to-end tests under `e2e/`
- `mise run lint` / `mise run check` - rumdl, tombi, `cargo check`
- `mise run fmt` / `mise run format` - cargo fmt, rumdl fmt, tombi format
- `mise run render` - regenerate usage KDL, completions, man pages, schemas
- `mise run release:check` - validate `release.json`, notes, changelog, crates
- `mise run release:bump <target>` - bump `Cargo.toml` and `Cargo.lock` for a release PR
- `mise run release:assets` - stage usage KDL and completions under `dist/`
- `mise run release:tarball [triple]` - build one platform tarball
- `mise run release:checksums` - write `dist/SHASUMS256.txt`
- `mise run bootstrap` - install the binary so `shcomp` is on `PATH`

Run `cargo fmt --all` and `mise run lint` before committing.

## Releasing

Releases are CI-only. A release PR adds `release.json`, hand-written
`docs/releases/<version>.md`, a `CHANGELOG.md` entry, and the `Cargo.toml` /
`Cargo.lock` bump. After it merges, `.github/workflows/release.yml` verifies the
manifest, builds every platform, creates the tag and draft release, signs the
packslip bundle, and publishes. Do not publish from a local checkout.

## Layout

- `src/cli.rs` - usage-rs CLI surface and dispatch.
- `src/config.rs` - user config: selections and settings, never recipes.
- `src/registry/` - registry items, loading, resolution, `ls`.
- `src/reference.rs` - reference and workspace normalization.
- `src/template.rs` - Tera rendering with a closed context.
- `src/dirs.rs` - XDG paths and environment overrides.
- `src/schema.rs` - JSON Schema output.
- `registry/` - embedded registry items, one per workspace.
- `e2e/` - isolated end-to-end harness and `test_*` scripts.
- `xtasks/tasks/` - file tasks (`test:e2e`, `release:*`).
- `xtasks/lib/release.sh` - non-executable release helpers.
- `docs/releases/` - hand-written release notes, one file per version.
- `release.json` - release authorization manifest added by a release PR.
- `packaging/release.toml` - packslip resource layout.
- `.github/workflows/ci.yml` - lint, test, and release check on PRs.
- `.github/workflows/release.yml` - build, tag, and publish after a release PR merges.
- `.github/workflows/packslip-sign.yml` - reusable keyless packslip signing.
- `schema/` - generated JSON Schema; do not edit by hand.

## Design rules

- **The registry owns recipes; the config owns selections.** A `[tools]` value
  must never contain how to generate a completion.
- **PATH is the version source.** Do not ask mise or another manager which
  version is active on the completion path. Resolve the executable the shell
  resolved; key the cache by its realpath, mtime and size.
- **Never fetch on the completion path.** URL sources are fetched by `install`
  only.
- **No `exec()` or file-reading template functions.** Templates render argv and
  paths; they do not run commands.
- **Signatures are validated.** A generator that exits 0 and prints help is a
  failure, not a completion.
- **Version discovery has a fallback.** Prefer the install path; a registry item
  may declare `[tools.version] run` for tools such as `eza` and `gradle`.
- **Do not fight mise.** Skip commands mise's packslip loader owns, and use
  uniquely named functions (`__shcomp_*`) so an overlap cannot corrupt either
  side.

## Dependencies

Use the same crate versions mise uses. The current set is Tera 2, `toml` 1,
`schemars` 1, `indexmap` 2, `usage-rs` 7, `eyre` 0.6, `dirs` 6, `url` 2. Do not
add a dependency mise does not use without a written reason in the pull
request.

## Workflow

This repository uses `gh stack`. Work bottom-up:

```sh
gh stack init <layer>
# commit
gh stack add <next-layer>
# commit
gh stack submit --auto
```

Use conventional commit subjects. Keep each layer reviewable on its own and
green before moving up.
