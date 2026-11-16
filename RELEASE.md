# Releasing shcomp

Releases are cut with `mise run release:publish` on a clean `main` checkout.
That task builds and uploads assets, then dispatches
[`.github/workflows/packslip-sign.yml`](.github/workflows/packslip-sign.yml)
for keyless packslip (no `pubkey` for mise). The full
[`.github/workflows/release.yml`](.github/workflows/release.yml) matrix stays
deactivated.

## Prerequisites

- Clean git tree on `main`, push access, `gh` authenticated
- Version in `Cargo.toml` already bumped to the version you will publish — the
  task refuses a mismatch
- Optional: extra `rustup` targets if you want more than the host triple

## Bump the version

```sh
mise run release:bump 0.0.2          # or: patch | minor | major
```

The task updates `Cargo.toml` and keeps `Cargo.lock` in lockstep. Write GitHub
release notes at `release-notes/<version>.md` (required; publish refuses to run
without a non-empty file). Start from
[release-notes/TEMPLATE.md](release-notes/TEMPLATE.md).

Commit on `main` before publishing. Confirm what publish will require:

```sh
mise run release:version
```

## Dry-run (recommended)

Builds assets under `dist/` only — no tag, no `gh release`, no packslip.

```sh
git checkout main && git pull
mise run release:publish 0.0.1 --dry-run --host-only
ls -lh dist/
```

Useful flags:

| Flag | Effect |
| --- | --- |
| `--dry-run` | Build artifacts only |
| `--host-only` | Only the current rustc host triple |

## Publish

```sh
mise run release:publish 0.0.1 --host-only   # or omit --host-only
```

The task will:

1. Regenerate usage KDL, shell completions, and man page
2. Build platform tarballs
3. Write `dist/SHASUMS256.txt`
4. Create annotated tag `v<ver>`, push it, and `gh release create`
5. Dispatch `packslip-sign.yml` for that tag and wait until it is green

Cross-OS triples are skipped when this host cannot build them (e.g. no Darwin
archives from Linux). For a full matrix, run publish once per OS and attach
missing assets with `gh release upload`, then re-run packslip-sign (or
`gh workflow run packslip-sign.yml -f tag=…`).

## What ships

**Tarball** (`shcomp-<ver>-<os>-<arch>.tar.gz`):

```text
shcomp/
  bin/shcomp
  man/man1/shcomp.1
  README.md
  LICENSE
```

**Separate release assets** (not inside the archive): usage KDL, bash /
zsh / fish / powershell completions, checksums, and packslip sigstore.

Layout template: [packaging/release.toml](packaging/release.toml).

## After the release — install

PC via packslip:

```toml
[tools]
"packslip:github.com/dxkbroom/shcomp" = "0.0.1"
```

## Lower-level tasks

Prefer `release:publish`. For debugging a single step:

| Task | Role |
| --- | --- |
| `mise run release:assets` | usage KDL + completions staged under `dist/` |
| `mise run release:tarball [triple]` | One archive under `dist/` |
| `mise run release:checksums` | `dist/SHASUMS256.txt` |
| `mise run release:version` | Print the current Cargo package version |
| `mise run release:bump <target>` | Bump `Cargo.toml` and `Cargo.lock` |

## Checklist

1. Version bumped and on `main`
2. `release-notes/<version>.md` written
3. `mise run lint` and `mise run test` (or green CI on `main`)
4. Dry-run with `--host-only`
5. Real publish (`mise run release:publish` — waits on packslip-sign)
6. Spot-check mise install (no `pubkey`)
