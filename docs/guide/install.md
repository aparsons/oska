# Install

Oska is pre-alpha. Today you build `oskac`, the compiler and runner, from source. The one-line installers and signed binaries arrive with **v0.1.0**, the first release, and are described below as the decided design.

## Build from source

This works today. You need a stable Rust toolchain, version 1.75 or newer, and a checkout of the repository:

```sh
cargo build --release -p oskac
```

The binary is `target/release/oskac`. Copy it somewhere on your `PATH`, or let Cargo install it into `~/.cargo/bin`:

```sh
cargo install --path crates/oskac
```

Confirm that it works:

```sh
oskac --version
```

It prints `oskac` and the version on one line. A build from this checkout prints `oskac 0.0.1`. Then continue with the [quickstart](quickstart.md).

## One-line installers

> **Available from v0.1.0.** Everything in this section is the decided design for the first release. It is not published yet, so the commands below will not work until v0.1.0 is cut. When it ships, delete this notice and the `planned` marks on the code blocks.

On Linux and macOS:

```sh planned
curl -fsSL https://oskalang.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell planned
irm https://oskalang.dev/install.ps1 | iex
```

What the installers do:

- **They never prompt.** They are safe to run from a script, a CI job, or an AI agent.
- **They install to `~/.oska/bin`.** Make sure that directory is on your `PATH`.
- **They verify before they install.** The installer checks the download's SHA-256 checksum against `SHA256SUMS` and checks the SSH signature (the one `ssh-keygen -Y verify` understands). If either check fails, nothing is installed.
- **They take `--version` and `--json`.** `--version` installs that release instead of the latest, and `--json` prints the result as JSON so a tool can read it. The result reports the installed version and path and whether the checksum and signature verified. The exact field names are fixed when the installer ships.

To pass options through the pipe:

```sh planned
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.1.0 --json
```

The binaries are published on oskalang.dev, together with `SHA256SUMS` and its signature, for Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64.

### Verify the installation

```sh
oskac --version
```

If `oskac` is not found, add `~/.oska/bin` to your `PATH`. The [quickstart](quickstart.md) runs a first program.

### Update

Re-run the installer. It installs the latest release over the old one, and with `--version` it installs a specific one. There is no self-update command.

### Uninstall

Delete the files the installer put in `~/.oska/bin`.

## Set up an AI agent

> **Available from v0.1.0.** The site is planned to publish a copy-and-paste prompt that has an AI coding agent install Oska, verify the toolchain, and configure your project. Until then, the [agent guide](agent-guide.md) describes the loop the prompt sets up.
