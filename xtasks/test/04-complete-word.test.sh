#!/usr/bin/env bash
# complete-word delegates to the target shell. With mise selecting the
# version, the same command must report that version's candidates.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
bin="$root/target/debug/shcomp"
[[ -x $bin ]] || {
  echo "build the binary first: cargo build" >&2
  exit 1
}
command -v zsh >/dev/null || {
  echo "zsh is not installed; skipping"
  exit 0
}
command -v mise >/dev/null || {
  echo "mise is not installed; skipping"
  exit 0
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/data" "$tmp/cache" "$tmp/config" "$tmp/project"

for version in 1.0.0 2.0.0; do
  mkdir -p "$tmp/installs/gh/$version/bin"
  cat >"$tmp/installs/gh/$version/bin/gh" <<SH
#!/bin/sh
printf '#compdef gh\n_gh() { compadd version$version; }\n'
SH
  chmod +x "$tmp/installs/gh/$version/bin/gh"
done

export MISE_DATA_DIR="$tmp/mise-data"
export SHCOMP_CONFIG="$tmp/config/config.toml"
export SHCOMP_DATA_DIR="$tmp/data"
export SHCOMP_CACHE_DIR="$tmp/cache"

mise link gh@1.0.0 "$tmp/installs/gh/1.0.0" >/dev/null
mise link gh@2.0.0 "$tmp/installs/gh/2.0.0" >/dev/null

cat >"$SHCOMP_CONFIG" <<'TOML'
[tools]
gh = true
TOML

cd "$tmp/project"
one=$(mise x gh@1.0.0 -- "$bin" complete-word gh@1.0.0 gh "")
two=$(mise x gh@2.0.0 -- "$bin" complete-word gh@2.0.0 gh "")

[[ $one == *version1.0.0* ]] || {
  echo "version 1 did not complete as itself" >&2
  echo "$one" >&2
  exit 1
}
[[ $one != *version2.0.0* ]] || {
  echo "version 1 leaked version 2 candidates" >&2
  echo "$one" >&2
  exit 1
}
[[ $two == *version2.0.0* ]] || {
  echo "version 2 did not complete as itself" >&2
  echo "$two" >&2
  exit 1
}
[[ $two != *version1.0.0* ]] || {
  echo "version 2 leaked version 1 candidates" >&2
  echo "$two" >&2
  exit 1
}

echo "ok: complete-word follows the mise-selected version"
