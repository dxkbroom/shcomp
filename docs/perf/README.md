# Performance

`shcomp` measures itself with [tak](https://github.com/jdx/tak): three
instruction-counted subjects, run against the fixture in `benchmarks/fixture`,
with a 1% gate on every pull request.

| Subject | What it measures |
| --- | --- |
| `startup` | `shcomp --help`: process start, linking, and building the command tree. |
| `ls` | Registry and config resolution plus one cache stat per row. |
| `load` | The one command a shell forks on a cold Tab; `setup` prewarms the cache. |

`doctor` is deliberately absent: it probes PATH, mise, and the shim directory, so
its cost is the machine's rather than shcomp's.

## Where the numbers live

tak does not write its measurements to a directory, and it has no setting to
choose one. Each measured commit gets a note on the `refs/notes/tak` git ref
inside the repository, and `perf.yml` pushes that ref to the remote so
`perf-pr.yml` can compare a pull request against its merge base. That is the
whole store: history is read back with `tak log`, `tak history`, and
`tak explain`, and it travels with the clone.

Because the store is git notes, cloning does not bring it by default; `tak init`
teaches plain `git fetch` about the ref, and every tak command that reads
history refreshes it from `origin` first.

## What can be saved under this directory

tak's file outputs are opt-in, and this directory is where the durable ones
belong:

| Command | Writes | Committed? |
| --- | --- | --- |
| `mise run perf:docs` | `history.md`, the markdown history `tak log` prints | yes, refreshed deliberately |
| `mise run perf:docs` | `history.html`, the same history as self-contained charts | no, gitignored |
| `tak run --export-json <path>` | every sample and summary, hyperfine-compatible | only when attaching to an issue |
| `tak artifact export --output <path>` | a portable measurement file for CI plumbing | no |
| `tak run --profile-dir <dir>` | the cachegrind profile behind each count | no |

`history.md` is generated; do not edit it by hand. It is empty until the first
`perf.yml` run records `main`, and it is refreshed by running `mise run perf:docs`
from a checkout whose notes are current — after a release, or after a stack
merges. Regenerating it on every commit would produce a file nobody reads and a
bot commit on `main` that re-triggers the very workflow that wrote it.

## Reading a number

Instruction counts are the only gate-able metric: they vary by hundredths of a
percent on a quiet host, while wall clock moves by percent. Counts are only
comparable within one runner class, which names the runner image, CPU class, and
Rust toolchain; `perf.yml` and `perf-pr.yml` both pass
`gha-ubuntu-24.04-x64-rust1.99.0`, and changing it starts a new series.

A local `mise run perf` without valgrind reports wall clock only and says so. It
is useful for spotting a change in shape, and it is not a substitute for the CI
gate.
