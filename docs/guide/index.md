# Oska guides

Task-oriented guides for the `oskac` toolchain. Oska is pre-alpha: these pages describe what the current build does, which is v0.1.0 or later. The programs and outputs on these pages are checked against the real toolchain by `cargo test`.

## Start here

1. [Install](install.md): one-line installers for the v0.1.0 release, or build from source.
2. [Quickstart](quickstart.md): write, check, run, and test a program.
3. [Language tour](language-tour.md): the syntax and types in one sitting.

## Concepts

- [Capabilities and authority](capabilities.md): how a program's effects are visible in its signatures.
- [Untrusted data and secrets](untrusted-data.md): how Oska keeps injection and leaks out of programs.

## Working with the toolchain

- [Testing with `oskac test`](testing.md)
- [Debugging: `dbg`, panics, and `--format=json`](debugging.md)
- [Troubleshooting](troubleshooting.md): by symptom and by diagnostic code.
- [CLI reference](cli-reference.md)
- [Agent guide](agent-guide.md): the check, fix, run, test, and authority loop, with machine-readable output.

## Examples

Each example page shows the program's source and its `oskac check` authority report.

- [hello](examples/hello.md)
- [capabilities](examples/capabilities.md)

## Reference

- [Design notes](../design.md), [checker notes](../checker.md), [runtime notes](../runtime.md), and the [AST reference](../ast.md).
- [Glossary](../../GLOSSARY.md) and the [decision records](../adr/).
