#!/usr/bin/env bash
# install reconciles shims, doctor notices stale aliases, and uninstall removes
# only files shcomp wrote.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
bin="$root/target/debug/shcomp"
[[ -x $bin ]] || {
  echo "build the binary first: cargo build" >&2
  exit 1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/data" "$tmp/cache" "$tmp/config"

cat >"$tmp/bin/fake" <<'SH'
#!/bin/sh
printf '#compdef fake\n_fake() { compadd one; }\n'
SH
chmod +x "$tmp/bin/fake"

write_config() {
  cat >"$tmp/config/config.toml"
}

write_config <<'TOML'
[tools]
"example/fake" = { aliases = ["fk"] }

[[registry]]
repo = "example/fake"
main = "fake"

[[registry.tools]]
name = "fake"

[registry.tools.completions.zsh]
run = ["{{ bin }}"]
TOML

export SHCOMP_CONFIG="$tmp/config/config.toml"
export SHCOMP_DATA_DIR="$tmp/data"
export SHCOMP_CACHE_DIR="$tmp/cache"
export PATH="$tmp/bin:$PATH"

"$bin" install >/dev/null
shim="$tmp/data/zsh/site-functions/_fake"
[[ -f $shim ]] || {
  echo "install did not write $shim" >&2
  exit 1
}
grep -q '#compdef fake fk' "$shim" || {
  echo "shim does not register the alias" >&2
  cat "$shim" >&2
  exit 1
}

write_config <<'TOML'
[tools]
"example/fake" = { aliases = ["fk", "fk2"] }

[[registry]]
repo = "example/fake"
main = "fake"

[[registry.tools]]
name = "fake"

[registry.tools.completions.zsh]
run = ["{{ bin }}"]
TOML

doctor=$("$bin" doctor)
[[ $doctor == *stale* ]] || {
  echo "doctor did not report the stale alias" >&2
  echo "$doctor" >&2
  exit 1
}

"$bin" install >/dev/null
grep -q '#compdef fake fk fk2' "$shim" || {
  echo "install did not refresh the aliases" >&2
  cat "$shim" >&2
  exit 1
}

dry=$("$bin" uninstall --dry-run)
[[ $dry == *"would remove"* && -f $shim ]] || {
  echo "uninstall --dry-run changed the tree" >&2
  echo "$dry" >&2
  exit 1
}

"$bin" uninstall >/dev/null
[[ ! -e $shim ]] || {
  echo "uninstall left $shim" >&2
  exit 1
}

echo "ok: install, doctor, and uninstall"
