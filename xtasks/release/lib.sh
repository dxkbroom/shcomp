#!/usr/bin/env bash
# Shared helpers for xtasks/release/*. Sourced, never executed as a task.
# This file is intentionally not executable so mise does not register it.

release_root() {
  if [[ -n ${MISE_PROJECT_ROOT:-} ]]; then
    printf '%s\n' "$MISE_PROJECT_ROOT"
    return
  fi
  local here
  here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  (cd -- "$here/../.." && pwd)
}

release_version() {
  local id
  id="$(cargo pkgid shcomp)"
  printf '%s\n' "${id##*@}"
}

release_host_triple() {
  rustc -vV | sed -n 's/^host: //p'
}

# Sets RELEASE_OS / RELEASE_ARCH using packslip's naming.
# shellcheck disable=SC2034 # consumed by the task that sources this file
release_platform() {
  local triple="$1"
  case "$triple" in
    *-linux-gnu) RELEASE_OS=linux ;;
    *-apple-darwin) RELEASE_OS=macos ;;
    *)
      echo "unsupported target triple: $triple" >&2
      return 1
      ;;
  esac
  case "$triple" in
    x86_64-*) RELEASE_ARCH=x64 ;;
    aarch64-*) RELEASE_ARCH=arm64 ;;
    *)
      echo "unsupported target arch: $triple" >&2
      return 1
      ;;
  esac
}

release_require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required" >&2
    return 1
  fi
}

# A triple is buildable when we are on its OS and, for cross builds, the
# target is installed in the active rustup toolchain.
release_triple_buildable() {
  local triple="$1" host="$2"
  [[ $triple == "$host" ]] && return 0
  case "$triple" in
    *-apple-darwin) [[ $(uname -s) == Darwin ]] || return 1 ;;
    *-linux-gnu) [[ $(uname -s) == Linux ]] || return 1 ;;
  esac
  command -v rustup >/dev/null 2>&1 || return 1
  rustup target list --installed | grep -qx "$triple"
}
