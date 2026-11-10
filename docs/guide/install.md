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
- **They install to `~/.oska/bin`.** Set `OSKA_HOME` to install into `$OSKA_HOME/bin` instead. They don't change your `PATH`; if `~/.oska/bin` is not on it, they print the line to add.
- **They verify before they install.** The installer checks the SSH signature on `SHA256SUMS` (`ssh-keygen -Y verify`, so it needs OpenSSH 8.1 or newer) against the release key built into the installer, then checks the download's SHA-256 checksum against `SHA256SUMS`. If either check fails, nothing is installed and the installer exits with a non-zero status.
- **They download over HTTPS only.**
- **They take `--version` and `--json`.** `--version` installs that release instead of the latest. `--json` prints one line of JSON on stdout and nothing else.

To pass options through the pipe:

```sh planned
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.1.0 --json
```

`iex` cannot pass options, so in PowerShell run the script as a script block:

```powershell planned
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --version 0.1.0 --json
```

A successful `--json` run prints:

```json
{"ok": true, "action": "installed", "version": "0.1.0", "tag": "v0.1.0", "previous_version": null, "platform": "linux-x86_64", "path": "/home/you/.oska/bin/oskac", "bin_dir": "/home/you/.oska/bin", "on_path": false, "checksum_verified": true, "signature_verified": true}
```

- `action` is `installed`, `updated` (with the old version in `previous_version`), or `reinstalled`.
- `platform` is one of `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, or `windows-x86_64`.
- On Windows, `path` ends in `oskac.exe`.

A failed run prints `{"ok": false, "error": "<code>", "message": "..."}` and exits non-zero: 2 for a usage error, 1 for anything else. The codes are:

| `error` | Meaning |
| --- | --- |
| `usage` | An unknown option, or a `--version` that is not a version. |
| `no_signing_key` | The installer has no release key built in. Download it again from oskalang.dev. |
| `insecure_url` | `OSKA_SITE_URL` is not an `https://` URL. |
| `unsupported_platform` | There is no build for this OS or CPU. |
| `missing_tool` | `curl`, `ssh-keygen`, or a SHA-256 tool is missing. |
| `download_failed` | A file could not be downloaded, for example because that version does not exist. |
| `bad_signature` | `SHA256SUMS` is not signed by the release key. |
| `checksum_mismatch` | The binary does not match `SHA256SUMS`. |
| `install_failed` | The binary could not be written into place. |

The binaries are published on oskalang.dev, together with `SHA256SUMS` and its signature, for Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64.

### Verify the installation

```sh
oskac --version
```

If `oskac` is not found, add `~/.oska/bin` to your `PATH`. The [quickstart](quickstart.md) runs a first program.

### Update

Re-run the installer. It installs the latest release over the old one, and with `--version` it installs a specific one. There is no self-update command.

### Uninstall

Delete `~/.oska`, which holds `bin/oskac` and a `version` file recording what the installer put there.

## Set up an AI agent

> **Available from v0.1.0.** The site is planned to publish a copy-and-paste prompt that has an AI coding agent install Oska, verify the toolchain, and configure your project. Until then, the [agent guide](agent-guide.md) describes the loop the prompt sets up.
