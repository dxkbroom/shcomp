#!/usr/bin/env bash
# The zsh shim loads a version's script once, calls the captured body on later
# completions, and reloads when PATH selects a different executable.
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

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/v1" "$tmp/v2" "$tmp/logbin" "$tmp/data" "$tmp/cache" "$tmp/config"

cat >"$tmp/v1/fake" <<'SH'
#!/bin/sh
printf '#compdef fake\n_fake() { compadd one two; }\n'
SH
cat >"$tmp/v2/fake" <<'SH'
#!/bin/sh
printf '#compdef fake\n_fake() { compadd three four; }\n'
SH
chmod +x "$tmp/v1/fake" "$tmp/v2/fake"

cat >"$tmp/config/config.toml" <<'TOML'
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
export SHCOMP_LOG="$tmp/load.log"
export SHCOMP_TMP="$tmp"

PATH="$tmp/v1:$PATH" "$bin" install >/dev/null

cat >"$tmp/logbin/shcomp" <<SH
#!/bin/sh
printf '%s\n' "\$*" >>"\$SHCOMP_LOG"
exec "$bin" "\$@"
SH
chmod +x "$tmp/logbin/shcomp"

cat >"$tmp/harness.zsh" <<'ZSH'
unset FPATH
fpath=("$SHCOMP_DATA_DIR/zsh/site-functions" /usr/share/zsh/functions/Completion /usr/share/zsh/functions/Completion/*)
autoload -Uz compinit
compinit -u -d "$SHCOMP_TMP/zcompdump"

__cands=()
compadd() { __cands+=("$@") }

path=("$SHCOMP_TMP/v1" $path)
rehash
words=(fake "")
CURRENT=2
__cands=(); _fake
print -r -- "v1a:${(j:,:)__cands}"
__cands=(); _fake
print -r -- "v1b:${(j:,:)__cands}"

path=("$SHCOMP_TMP/v2" $path)
rehash
words=(fake "")
CURRENT=2
__cands=(); _fake
print -r -- "v2a:${(j:,:)__cands}"
__cands=(); _fake
print -r -- "v2b:${(j:,:)__cands}"

words=(fk "")
CURRENT=2
__cands=(); _fake
print -r -- "alias:${(j:,:)__cands}"

if [[ ${functions[_fake]} == *__shcomp_bin_fake_* ]]; then
  print -r -- "wrapper:restored"
else
  print -r -- "wrapper:lost"
fi
ZSH

out=$(PATH="$tmp/logbin:$tmp/v1:$PATH" zsh -f "$tmp/harness.zsh")
printf '%s\n' "$out"

assert_contains() {
  local haystack=$1 needle=$2
  [[ $haystack == *"$needle"* ]] || {
    echo "missing: $needle" >&2
    echo "$haystack" >&2
    exit 1
  }
}

assert_contains "$out" "v1a:one,two"
assert_contains "$out" "v1b:one,two"
assert_contains "$out" "v2a:three,four"
assert_contains "$out" "v2b:three,four"
assert_contains "$out" "alias:three,four"
assert_contains "$out" "wrapper:restored"

loads=$(wc -l <"$SHCOMP_LOG")
if [[ $loads -ne 2 ]]; then
  echo "expected 2 loads (one per version), got $loads" >&2
  cat "$SHCOMP_LOG" >&2
  exit 1
fi

echo "ok: zsh shim dispatch and warm path"
