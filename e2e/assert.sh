#!/usr/bin/env bash
# Assertions available to every e2e test. `run_test` sources this file before
# sourcing the test itself, so tests only describe the behavior under test.

ASSERT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR
source "$ASSERT_DIR/style.sh"

fail() {
  title="E2E assertion failed" err "$*"
  exit 1
}

skip() {
  notice "skipping: $*"
  exit 0
}

# Safeguard against running a test directly, which would run in the real home.
[[ -n ${TEST_NAME:-} ]] || fail "tests should be called using run_test"

quiet_assert_succeed() {
  local output exit_status=0
  debug "$ $1"
  output="$(bash -c "$1" 2>&1)" || exit_status=$?
  if [[ $exit_status -ne 0 ]]; then
    fail "[$1] command failed with status $exit_status: $output"
  fi
  printf '%s' "$output"
}

assert_succeed() {
  quiet_assert_succeed "$1" >/dev/null
  ok "[$1] succeeded"
}

assert() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ -z ${2:-} ]]; then
    ok "[$1]"
  elif [[ $actual == "$2" ]]; then
    ok "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual == *"$2"* ]]; then
    ok "[$1] '$2' is in output"
  else
    fail "[$1] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual != *"$2"* ]]; then
    ok "[$1] '$2' is not in output"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_matches() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual =~ $2 ]]; then
    ok "[$1] '$2' matches output"
  else
    fail "[$1] expected '$2' to match '$actual'"
  fi
}

assert_contains_text() {
  local actual="$1"
  if [[ $actual == *"$2"* ]]; then
    ok "[text] '$2' is in output"
  else
    fail "[text] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains_text() {
  local actual="$1"
  if [[ $actual != *"$2"* ]]; then
    ok "[text] '$2' is not in output"
  else
    fail "[text] expected '$2' not to be in '$actual'"
  fi
}

assert_exists() {
  if [[ -e $1 ]]; then
    ok "[$1] exists"
  else
    fail "[$1] does not exist"
  fi
}

assert_not_exists() {
  if [[ ! -e $1 ]]; then
    ok "[$1] does not exist"
  else
    fail "[$1] exists"
  fi
}

assert_executable() {
  if [[ -x $1 ]]; then
    ok "[$1] is executable"
  else
    fail "[$1] is not executable"
  fi
}

assert_file_contains() {
  local file=$1 needle=$2
  if [[ ! -f $file ]]; then
    fail "[$file] does not exist"
  elif grep -Fq -- "$needle" "$file"; then
    ok "[$file] contains '$needle'"
  else
    fail "[$file] does not contain '$needle': $(cat "$file")"
  fi
}

have_cmd() {
  type -p "$1" >/dev/null 2>&1
}

require_cmd() {
  if ! have_cmd "$1"; then
    title="E2E test $TEST_NAME aborted" err "'$1' is required but was not found in PATH"
    exit 2
  fi
}

skip_unless_cmd() {
  have_cmd "$1" || skip "'$1' is not installed"
}
