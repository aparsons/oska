# Rust toolchain

The Rust version is pinned in `rust-toolchain.toml`, so the Pi, CI and release builds all use one Rust.

## Upgrading Rust

A weekly workflow (`.github/workflows/rust-stable.yml`) opens a "Bump Rust to X.Y.Z" issue labelled `ready-for-agent` when a newer stable Rust is out, and moves that issue to the newest version if it is still open. To do the upgrade:

```sh
scripts/bump-rust.sh X.Y.Z
```

It sets `channel` in `rust-toolchain.toml`, regenerates the two tables that follow the Rust's Unicode in `crates/oska-check/src/std/rt.osk` (`escaped_table` for `dbg` and `lowercase_table`, built by `scripts/bump-rust/tables.rs`), and runs `scripts/check.sh`. Open a PR with the result.

If a newer Clippy fails the check, the script stops with its output; fix the code it names and run `scripts/check.sh` again. Running the script with the version already pinned leaves `rt.osk` unchanged.

## Windows target for check.sh

`scripts/check.sh` runs `cargo check -p oskac --tests --target x86_64-pc-windows-gnu`, so a type error in `cfg(windows)` code shows up locally instead of only on the Windows CI job. The target is not in `rust-toolchain.toml`, so the Windows, macOS and release jobs do not download it. Add it once per machine and per pinned Rust: `rustup target add x86_64-pc-windows-gnu` (about 139 MB). `scripts/bump-rust.sh` installs it with the new Rust, and the `ci.yml` test job adds it. Locally a missing target skips the step with a message; in CI (`CI` or `GITHUB_ACTIONS` set) it fails.
