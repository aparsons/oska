# Install

Oska is pre-alpha. **v0.1.0** is the first release. You can install `oskac`, the compiler and runner, from a signed binary with a one-line installer, or build it from source.

## Build from source

You need a stable Rust toolchain, version 1.77 or newer, and a checkout of the repository:

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

It prints `oskac` and the version on one line. A build from this checkout prints `oskac 0.1.0`. Then continue with the [quickstart](quickstart.md).

## One-line installers

On Linux and macOS:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://oskalang.dev/install.ps1 | iex
```

What the installers do:

- **They never prompt.** They are safe to run from a script, a CI job, or an AI agent.
- **They install to `~/.oska/bin`.** Set `OSKA_HOME` to install into `$OSKA_HOME/bin` instead. They don't change your `PATH`; if `~/.oska/bin` is not on it, they print the line to add.
- **They verify before they install.** The installer checks the SSH signature on `SHA256SUMS` (`ssh-keygen -Y verify`, so it needs OpenSSH 8.1 or newer) against the release key built into the installer. The signed file's first line, `# oska-release v0.1.0`, names the release, and it must be the one the installer asked for, so an old release cannot be served as a newer one. Then the installer checks the download's SHA-256 checksum against `SHA256SUMS`, and checks that the binary reports the expected version. If any check fails, nothing is installed and the installer exits with a non-zero status.
- **They download over HTTPS only.** They refuse a redirect to a plain `http://` URL.
- **They take `--version` and `--json`.** `--version` installs that release instead of the latest. `--json` prints one line of JSON on stdout and nothing else.

To pass options through the pipe:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.1.0 --json
```

`iex` cannot pass options, so in PowerShell run the script as a script block:

```powershell
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
| `insecure_url` | `OSKA_SITE_URL` is not an `https://` URL, or a download was redirected to one that is not. |
| `unsupported_platform` | There is no build for this OS or CPU. |
| `unsupported_libc` | Linux only: the C library is not glibc 2.35 or newer (for example musl, as on Alpine). |
| `missing_tool` | `curl`, `ssh-keygen`, or a SHA-256 tool is missing. |
| `download_failed` | A file could not be downloaded, for example because that version does not exist. |
| `bad_signature` | `SHA256SUMS` is not signed by the release key. |
| `checksum_mismatch` | The binary does not match `SHA256SUMS`. |
| `version_mismatch` | The signed `SHA256SUMS` is for another release than the one asked for, or the binary reports another version. |
| `install_failed` | The binary could not be written into place. |

The binaries are published on oskalang.dev, together with `SHA256SUMS` and its signature, for Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64. The Linux binaries are built on Ubuntu 22.04 and need glibc 2.35 or newer. They do not run on an older glibc or on musl systems such as Alpine; there the installer stops with `unsupported_libc`, and you can build from source instead.

### Verify the installation

```sh
oskac --version
```

If `oskac` is not found, add `~/.oska/bin` to your `PATH`. The [quickstart](quickstart.md) runs a first program.

### Update

Re-run the installer. It installs the latest release over the old one, and with `--version` it installs a specific one. There is no self-update command.

### Uninstall

Delete `~/.oska`, which holds `bin/oskac` and a `version` file recording what the installer put there.

## Upgrading to v0.2.0

v0.2.0 is a **breaking change** to the filesystem capabilities, with no deprecation period ([ADR 0008](../adr/0008-file-access-goes-through-confined-capabilities.md)). `Fs`, `FsRead`, and `FsWrite` no longer read or write anything. They only attenuate, and every read and write goes through a directory capability (`DirRead`, `DirWrite`) or a file capability (`FileRead`, `FileWrite`). A program that calls a removed method fails `oskac check` with `E0273`, whose message shows the fix. The check cannot apply it for you, because only you know the root.

Each removed method has a one-line migration. Replace `<root>` with the directory the program should reach:

| Before (v0.1.0) | After (v0.2.0) |
| --- | --- |
| `fs.read_to_string(path)` on an `FsRead` | `fs.restrict_to("<root>").read_to_string(path)` |
| `fs.list_dir(path)` on an `FsRead` | `fs.restrict_to("<root>").list_dir(path)` |
| `fs.write(path, text)` on an `FsWrite` | `fs.restrict_to("<root>").write(path, text)` |
| `fs.create_dir(path)` on an `FsWrite` | `fs.restrict_to("<root>").create_dir(path)` |
| `fs.restrict_to("file")` naming a single file | `fs.restrict_to_file("file")`, then `.read_to_string()` or `.write(text)` with no path |
| Reading anywhere through an `FsRead` | `fs.restrict_to("/")`, which the authority report shows |

Other things change with it:

- A function that took an `FsRead` or an `FsWrite` to read or write files now takes a `DirRead` or a `DirWrite`, or a `FileRead` or a `FileWrite`.
- `main` may not declare a directory or file capability (`E0217`). Declare `Fs`, `FsRead`, or `FsWrite`, and attenuate.
- `restrict_to` on a file no longer works. The capability it returns is broken, and every operation on it fails with a "not a directory" `IoError`. Use `restrict_to_file`.
- Every symlink beneath a root is refused, including one that points inside the root.
- `oskac check` now lists each attenuation under its function, in the report and in the JSON `attenuations` array.

An example of the migration:

```oska error=E0273
fn count(fs: FsRead, path: Path) -> Result<Int, IoError> {
    Ok(fs.read_to_string(path)?.lines().count())
}
```

```oska
fn count(fs: FsRead, path: Path) -> Result<Int, IoError> {
    Ok(fs.restrict_to("/srv/app").read_to_string(path)?.lines().count())
}
```

## Set up an AI agent

The site publishes a copy-and-paste prompt that has an AI coding agent install Oska, verify the toolchain, and configure your project. It is at the top of this page and at `https://oskalang.dev/agent-setup.md` as plain Markdown. The [agent guide](agent-guide.md) describes the loop the prompt sets up.
