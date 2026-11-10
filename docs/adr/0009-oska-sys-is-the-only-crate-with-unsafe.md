# `oska-sys` is the only crate allowed to use `unsafe`

## Decision

Every crate in the workspace forbids `unsafe` code, except one: `oska-sys`, an internal crate that confines filesystem access beneath a directory handle on Unix.

- **It adds no dependencies.** It declares the few C-library functions it needs itself, such as `openat`, and calls them through the C library that `std` already links.
- **It exposes only a safe API.** The runtime opens a root, and opens directories and files beneath it, through a directory handle type. Nothing `unsafe` is public.
- **Every `unsafe` block states its invariant.** Each block holds one unsafe operation and begins with a `// SAFETY:` comment. Clippy's `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` lints enforce both.
- **The exception cannot spread.** The workspace keeps `unsafe_code = "forbid"`, and `oska-sys` is the only crate that does not take the workspace's lints. `scripts/check.sh` runs `scripts/unsafe-guard/guard.sh`, which proves it by compiling: every other crate, every target, is built with `-F unsafe_code` last on rustc's command line, so no manifest, source attribute or flag can allow `unsafe` there, and `--cap-lints` is refused. The guard also refuses a checked-in cargo config and a workflow that sets rustc flags or a wrapper, and scans source the build skips, such as `#[cfg(windows)]` code, for the word `unsafe`. `scripts/unsafe-guard/test.sh` tries each known way around it.

## Why

A restricted filesystem must not be steerable outside its root, even by another process that swaps a directory for a symlink while the program runs (#69). The only race-free way to do that is to open each path component relative to a handle for the one before, never following a symlink. On Unix that is `openat` with `O_NOFOLLOW`, one component at a time.

`std` has no `openat`, and it has no way to open a path beneath a directory handle. The usual answer, the `libc` or `rustix` crate, is ruled out because the workspace takes no crates from outside the repo. Calling the C library directly needs `unsafe`. Putting all of it in one small crate keeps the code that needs review in one place, with every invariant written down.

## Considered options

- **Keep path-based checks.** Rejected: checking a path and then opening it are separate steps, and a symlink swapped in between them escapes the root. The checks narrow the window, but they cannot close it.
- **Use the `libc` or `rustix` crate.** Rejected: the workspace takes no external crates.
- **Allow `unsafe` in the runtime crate itself.** Rejected: the exception would cover a large crate, and nothing would keep `unsafe` from spreading through it.
- **Linux `openat2` with `RESOLVE_BENEATH`.** Not needed now. Walking with `openat` works on every Unix target in the release matrix, and `openat2` is Linux-only. It remains an optional speed-up for later.

## Consequences

- **Windows keeps path-based checks.** `oska-sys` is empty there, and a confined capability on Windows checks each component on the path instead. That narrows the race but does not close it, and the docs say so.
- **The platform values are hand-copied.** Flag and error numbers, such as `O_NOFOLLOW` and `ELOOP`, come from each platform's headers. They are declared only for the targets Oska releases for, and any other Unix target fails to compile. CI runs the escape tests on Linux and macOS.
- **Every symlink beneath a root is refused,** including one that points inside the root, because the walk never follows one.
