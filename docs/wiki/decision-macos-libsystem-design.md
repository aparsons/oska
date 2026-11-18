# Decision: macOS libSystem calling path

On 2026-11-17 the user accepted all six recommendations of the design in PR #365 (`docs/research/2026-11-macos-libsystem-calls.md`, ADR 0015, proposed at the time):
1. The backend converts errno (`-1`/`NULL` → `-errno` via `__error()`).
2. The table reshape lands first as a Linux-only PR, before #169 (filed as #366; #169 marked blocked by #366).
3. Directory listing uses `fdopendir`/`readdir`.
4. Always import the full list of 24 libSystem functions.
5. Change four `std.rt` contracts on Linux too.
6. A new ADR 0015 rather than editing ADR 0013.

Risks noted on 2026-11-17: dyld's minimum load commands are unverified, with no run on a real Mac; a missing `$INODE64` suffix on x86_64 fails silently.

Related: [goal-drain-backlog](goal-drain-backlog.md), [research-oska-replacing-rust](research-oska-replacing-rust.md).

## Confirmed on a real Mac, 2026-11-18 (PR #381, first macOS CI run)
Programs built for `macos-aarch64` ran on macos-latest, passed `codesign --verify --strict`, and matched the interpreter. CI confirmed:
- dyld does not need `LC_SYMTAB`, `LC_DYSYMTAB` or `LC_DYLD_EXPORTS_TRIE` (this was the top unverified risk);
- GOT plus chained-fixup imports (format 6) work;
- variadic arguments go on the stack (`ioctl`, `openat`);
- `errno` is read through `___error`;
- `argc`/`argv` arrive in `x0`/`x1`;
- the ad-hoc signature is accepted.
Not yet confirmed: the Mac's cross-build byte comparison (the job failed earlier on a test race, not on the format).
