# Coverage is counted on every backend from one table of lines

## Decision

`oskac test` will report **coverage** of an Oska program: which of its lines a test run executed, per function. The interpreter and the native backend count it the same way. Both read one table of coverable lines, which the checker builds from the checked program, and both report the same lines as run, with the same hit counts, for the same tests. The report is line coverage only, as `GLOSSARY.md` defines it.

Coverage of the Rust code of `oskac` by its own tests is a separate matter. It uses the Rust toolchain's own tools in a CI job, and this decision does not touch it.

## Why

The interpreter is the reference for what every program does, and the differential suite fails on any difference between it and the native backend ([ADR 0007](0007-interpreter-runs-the-checked-program.md), [ADR 0014](0014-every-supported-platform-builds-and-runs-natively.md)). A coverage number that depends on the backend would break that rule, and a reader could not trust it: the default `oskac test` path runs native code, and a line that the interpreter reaches may not be reached there. One table, built from the checked program, gives every backend the same lines to count.

## Considered options

- **Count in the interpreter only.** Rejected: it is the cheapest change and works on every platform, but the default test path is native, so the report would describe a different run from the one the test used.
- **Count in native code only.** Rejected: it matches what ships, but it needs work in all three native backends before anyone gets a first number, and `--interpret` would report nothing.
- **Reuse LLVM source-based coverage.** Rejected: `oska-native` has its own backend and no LLVM ([ADR 0011](0011-oska-generates-native-code-itself.md)).

## Consequences

The native backend emits a counter for each coverable line when coverage is on, which costs time in a test run. A test run without `--coverage` carries no counter, and the build cache key includes the flag. The native test process runs once per test, so each process hands its counts to the parent, which merges them. The differential suite gains a check that the two backends report the same coverage for the corpus. The spec for Oska program coverage decides the report formats and the flags.
