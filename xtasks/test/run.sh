#!/usr/bin/env bash
# End-to-end tests for shcomp. Each test is a standalone bash script named
# `*.test.sh` in this directory; the harness runs them in lexical order.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
status=0
for test in "$here"/*.test.sh; do
  [[ -e $test ]] || continue
  printf '==> %s\n' "$(basename "$test")"
  if ! bash "$test"; then
    status=1
  fi
done
exit "$status"
