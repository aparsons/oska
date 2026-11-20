# Linux `oskac` ships as a glibc build and a static musl build

- **Status:** Accepted (2026-11-19). The user settled each decision below in a grilling session on 2026-11-19. The research behind it is [docs/research/2026-11-oskac-without-glibc.md](../research/2026-11-oskac-without-glibc.md).

## Decision

For each Linux processor, a release has two `oskac` builds:

- The **glibc build**, `oskac-linux-<arch>-gnu`, linked dynamically against glibc 2.35 or newer, as releases are built today.
- The **musl build**, `oskac-linux-<arch>-musl`, from Rust's `<arch>-unknown-linux-musl` target. It is a static executable with no C library, no dynamic loader and no shared-library dependency at run time, so it runs on any Linux, a musl distribution and an empty (`FROM scratch`) container included.

Both names replace today's `oskac-linux-<arch>`.

The two builds are variants of one supported platform, not two platforms. The `--target` names do not change, and an executable that `oskac` builds is the same bytes from either build.

The installer installs the glibc build when it finds glibc 2.35 or newer, else the musl build. `OSKA_LIBC=glibc` or `OSKA_LIBC=musl` overrides that choice, and the install receipt records the override (`libc_forced`). The installer itself does not read that record: each run chooses again, from `OSKA_LIBC` when it is set and from the system when it is not. The update checker (#468, not built yet) makes the same choice again at each update, but only by reading a few fixed files (the dynamic loader paths and the version banner in `libc.so.6`), so it needs no authority to start processes. For the update checker, a recorded override wins.

Each release also publishes `ghcr.io/aparsons/oskac`: an image that holds only the musl build at `/oskac` and an empty `/tmp`, runs as user 65532, and has one entry for each Linux processor. A script in the repo builds it as reproducible OCI bytes. Its index digest goes into a release file that `SHA256SUMS` covers, so the existing release signature covers the image. The release pushes those exact bytes after it signs the release.

## Why

The musl build runs with no C library. On x86_64 it passed the confinement tests and the native parity tests, and it ran in an empty root. But `oskac` is slower with musl's allocator: on the site generator, `oskac build` took 0.32 s against 0.20 s, and the interpreter took 5.8 s against 3.8 s. Executables that `oskac` builds run at the same speed from either build. The glibc build keeps the faster toolchain for the users who have glibc. The musl build serves everyone else.

## Considered options

- **The musl build only.** One build for each processor, but every Linux user gets a slower `oskac`.
- **A custom allocator first, then the musl build only.** This needs new `unsafe` code in `oska-sys` (ADR 0009) and takes weeks before anything ships.
- **`+crt-static` on the glibc target.** Rejected: glibc still loads its NSS modules at run time, and the unsafe guard refuses `RUSTFLAGS` in workflows.
- **An `oskac` with no C library at all (`no_std`).** Rejected: weeks of new `unsafe` code that works only on Linux.
- **Keeping the old asset names for the glibc build.** Rejected for clearer names. The cost: an installer that a user saved before the rename cannot install a release after it.
- **Docker buildx, or Sigstore signatures, for the image.** Rejected: the digest is known only after the push, and Sigstore adds a second trust root.

## Consequences

- The release workflow builds four Linux assets in place of two, gives the musl rows a `--target`, and checks that a musl build has no `PT_INTERP` and no `DT_NEEDED`. The glibc check (no symbol newer than `GLIBC_2.35`) remains for the glibc build. The release smoke-tests the musl build on both processors, in `alpine` and in the image.
- Every pull request builds the musl build on x86_64 and runs the `oska-sys` and `oska-run` tests and an empty-root smoke test with it.
- The signed index has one entry for each build. The installer stops with `unsupported_libc` in two cases only: when `OSKA_LIBC=glibc` forces the glibc build on a system without glibc 2.35 or newer, and when a release from before the musl build (v0.5.0 and earlier, which has only the glibc build) is asked for where only the musl build would run, or with `OSKA_LIBC=musl`. Otherwise it installs the musl build where the glibc build cannot run.
- The release's `publish` job gets `packages: write` to push the image by digest, and its `tag-image` job to tag it once the GitHub Release exists.
