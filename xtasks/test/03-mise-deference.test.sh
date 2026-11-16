#!/usr/bin/env bash
# A command mise's packslip loader already completes must not get a shcomp shim.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
bin="$root/target/debug/shcomp"
[[ -x $bin ]] || {
  echo "build the binary first: cargo build" >&2
  exit 1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/mise-install/bin" "$tmp/bin" "$tmp/data" "$tmp/cache" "$tmp/config"

cat >"$tmp/mise-install/bin/fake" <<'SH'
#!/bin/sh
printf '#compdef fake\n_fake() { compadd one; }\n'
SH
chmod +x "$tmp/mise-install/bin/fake"

cat >"$tmp/bin/mise" <<SH
#!/bin/sh
case "\$1 \$2" in
  "ls --json")
    printf '{"fake":[{"version":"1.0.0","install_path":"$tmp/mise-install","installed":true}]}'
    ;;
  "completion zsh")
    printf '#compdef fake\n_fake() { compadd from-mise; }\n'
    ;;
  *)
    exit 1
    ;;
esac
SH
chmod +x "$tmp/bin/mise"

cat >"$tmp/config/config.toml" <<'TOML'
[tools]
"example/fake" = true

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
export PATH="$tmp/bin:$tmp/mise-install/bin:$PATH"

out=$("$bin" install)
[[ $out == *"skipped fake"* ]] || {
  echo "install did not defer to mise" >&2
  echo "$out" >&2
  exit 1
}
[[ ! -e "$tmp/data/zsh/site-functions/_fake" ]] || {
  echo "install wrote a shim for a mise-owned command" >&2
  exit 1
}

doctor=$("$bin" doctor)
[[ $doctor == *"mise's packslip loader owns it"* ]] || {
  echo "doctor did not report the mise-owned command" >&2
  echo "$doctor" >&2
  exit 1
}

echo "ok: mise packslip deference"
