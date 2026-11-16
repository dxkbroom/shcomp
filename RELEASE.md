# Releasing shcomp

Releases are CI-only. There is no local publish task: a release PR carries the
version bump and the hand-written docs, and `.github/workflows/release.yml`
turns the merge into a tag, a GitHub release, and a signed packslip bundle.

The full matrix is active in `.github/workflows/release.yml`. Keyless
[`packslip-sign.yml`](.github/workflows/packslip-sign.yml) runs after the
release assets are uploaded and publishes the signed bundle.

## What a release PR contains

1. `docs/releases/<version>.md` — hand-written, user-facing release notes.
2. `CHANGELOG.md` — hand-written, more detailed than the release notes.
3. `Cargo.toml` + `Cargo.lock` — the actual version bump. The tagged commit is
   the PR merge commit, so the commit that CI tested is the commit that ships.
4. `release.json` — the machine-readable authorization manifest:

   ```json
   {
     "version": "0.0.1",
     "crates": ["shcomp"]
   }
   ```

`crates` lists the workspace crates that move with this release. `release:check`
verifies every listed crate's version matches `version`.

## Bump the version

```sh
mise run release:bump 0.0.2          # or: patch | minor | major
```

This updates `Cargo.toml` and `Cargo.lock`. It does not publish and does not
commit.

## Validate the release PR

```sh
mise run release:check
```

The check refuses to pass unless:

- `release.json` names a valid semver;
- `docs/releases/<version>.md` exists and is non-empty;
- `CHANGELOG.md` contains `## v<version>`;
- every crate listed in `release.json` has that version in `Cargo.toml`.

The same task runs in CI on every pull request. Normal PRs have no
`release.json`, so the check exits successfully without doing anything.

## What happens after merge

`.github/workflows/release.yml` runs on `push` to `main`:

1. `release:check` validates the manifest.
2. If `release.json` is absent, or `v<version>` already exists, the run exits
   successfully without releasing (idempotent for ordinary pushes).
3. The matrix builds the four target triples and uploads tarballs as workflow
   artifacts.
4. A fan-in job downloads every tarball, regenerates the flat usage/completion
   assets, writes `dist/SHASUMS256.txt`, and creates a **draft** release with
   `docs/releases/<version>.md`.
5. The reusable `packslip-sign.yml` workflow signs the release assets with
   OIDC and uploads `packslip.sigstore.json`.
6. A final job publishes the draft release.

No crates are published to crates.io.

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
zsh / fish / powershell completions, checksums, and the packslip sigstore
bundle. Layout template: [packaging/release.toml](packaging/release.toml).

## Install

Use the packslip backend for the full experience — binary, version-matched man
page, and shell completions:

```sh
mise use packslip:github.com/dxkbroom/shcomp --minimum-release-age "0s"
```

Or pin a version in `mise.toml`:

```toml
[tools]
"packslip:github.com/dxkbroom/shcomp" = "0.0.1"
```

The `github:` backend installs the binary only; it does not activate packslip
completions or man pages.

## Lower-level tasks

Prefer the release PR flow. For debugging a single stage:

| Task | Role |
| --- | --- |
| `mise run release:check` | Validate `release.json`, notes, changelog, crates |
| `mise run release:version` | Print the current Cargo package version |
| `mise run release:bump <target>` | Bump `Cargo.toml` and `Cargo.lock` |
| `mise run release:assets` | Stage usage KDL + completions under `dist/` |
| `mise run release:tarball [triple]` | One archive under `dist/` |
| `mise run release:checksums` | `dist/SHASUMS256.txt` |

## Checklist

1. `mise run release:bump` for the next version
2. `docs/releases/<version>.md` written
3. `CHANGELOG.md` updated
4. `release.json` added with the version and changed crates
5. `mise run release:check`, `mise run lint`, and `mise run test`
6. Open the release PR; required CI must be green
7. Merge; wait for `release.yml` and `packslip-sign.yml`
8. Verify `mise use packslip:github.com/dxkbroom/shcomp` in a clean directory
