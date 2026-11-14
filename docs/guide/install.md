# Install

Oska is pre-alpha. The latest release is **v0.3.0**. You can install `oskac`, the compiler and runner, from a signed binary with a one-line installer, or build it from source.

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

It prints `oskac` and the version on one line. A build from this checkout prints `oskac 0.3.0`. Then continue with the [quickstart](quickstart.md).

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
- **They verify before they install.** The installer checks the SSH signature on `SHA256SUMS` (`ssh-keygen -Y verify`, so it needs OpenSSH 8.1 or newer) against the release key built into the installer. The signed file's first line, `# oska-release v0.3.0`, names the release, and it must be the one the installer asked for, so an old release cannot be served as a newer one. Then the installer checks the download's SHA-256 checksum against `SHA256SUMS`, and checks that the binary reports the expected version. If any check fails, nothing is installed and the installer exits with a non-zero status.
- **They take the latest release from a signed index.** Without `--version`, the installer reads `release-index.json`, which names the latest release and is signed by the release key in its own namespace, `oska-index`. It refuses an index that is not signed by that key (`bad_index`), one that has expired (`stale_index`), and one older than the last index this install accepted (`rollback_refused`). It warns when the index expires within 30 days. `--version` does not read the index. Until the site publishes its first index, the installer reads the unsigned `latest.json` instead and reports `index_verified: false`.
- **They download over HTTPS only.** They refuse a redirect to a plain `http://` URL.
- **They never downgrade by accident.** If the release they would install is older than the installed `oskac`, they stop with `downgrade_refused` and install nothing. To go back to an older release, name it with `--version` and add `--allow-downgrade`.
- **They record what they installed.** See [the install receipt](#the-install-receipt).
- **They clean up after themselves.** If an install is interrupted, for example with Ctrl-C or because a CI job is cancelled, the installer removes its temporary and staging files before it exits.
- **They take `--version`, `--allow-downgrade`, `--check`, `--uninstall` and `--json`.** `--version` installs that release instead of the latest. `--allow-downgrade`, which needs `--version`, lets that release be older than the installed one. `--check` reports the installed and the latest version and installs nothing (see [Check for an update](#check-for-an-update)). `--uninstall` removes what the installer installed (see [Uninstall](#uninstall)). `--json` prints one line of JSON on stdout and nothing else.

To pass options through the pipe:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.3.0 --json
```

`iex` cannot pass options, so in PowerShell run the script as a script block:

```powershell
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --version 0.3.0 --json
```

A successful `--json` run prints:

```json
{"ok": true, "action": "installed", "version": "0.3.0", "tag": "v0.3.0", "previous_version": null, "platform": "linux-x86_64", "path": "/home/you/.oska/bin/oskac", "bin_dir": "/home/you/.oska/bin", "on_path": false, "checksum_verified": true, "signature_verified": true, "receipt": "/home/you/.oska/receipt.json", "index_verified": true, "index_serial": 4}
```

- `action` is one of:
  - `installed`: there was no `oskac` installed;
  - `updated`: the old version, in `previous_version`, was lower;
  - `reinstalled`: the same version was installed;
  - `downgraded`: the old version, in `previous_version`, was higher, and the run had `--version` and `--allow-downgrade`.

  Versions are compared by [semver](https://semver.org) precedence, so `0.4.0-pre.1` is lower than `0.4.0` and higher than `0.3.0`.
- `receipt` is the path of the [install receipt](#the-install-receipt).
- `index_verified` is `true` when the release came from the verified, signed index, and `index_serial` is that index's serial. With `--version`, or before the site publishes an index, they are `false` and `null`.
- `platform` is one of `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, or `windows-x86_64`.
- On Windows, `path` ends in `oskac.exe`.

A failed run prints `{"ok": false, "error": "<code>", "message": "..."}` and exits non-zero: 2 for a usage error, 1 for anything else. The codes are:

| `error` | Meaning |
| --- | --- |
| `usage` | An unknown option, a `--version` that is not a version, `--allow-downgrade` without `--version`, or options that cannot be used together, such as `--uninstall` with `--version`. |
| `no_signing_key` | The installer has no release key built in. Download it again from oskalang.dev. |
| `insecure_url` | `OSKA_SITE_URL` is not an `https://` URL, or a download was redirected to one that is not. |
| `unsupported_platform` | There is no build for this OS or CPU. |
| `unsupported_libc` | Linux only: the C library is not glibc 2.35 or newer (for example musl, as on Alpine). |
| `missing_tool` | `curl`, `ssh-keygen`, or a SHA-256 tool is missing. |
| `download_failed` | A file could not be downloaded, for example because that version does not exist. |
| `bad_signature` | `SHA256SUMS` is not signed by the release key. |
| `checksum_mismatch` | The binary does not match `SHA256SUMS`. |
| `version_mismatch` | The signed `SHA256SUMS` is for another release than the one asked for, or the binary reports another version. |
| `downgrade_refused` | The release to install is older than the installed `oskac`, and the run did not ask for a downgrade with both `--version` and `--allow-downgrade`. Nothing was installed. |
| `bad_index` | The release index is not signed by the release key in the `oska-index` namespace, has no signature, or is not in the index format. |
| `stale_index` | The release index has expired. A named release still installs with `--version`. |
| `rollback_refused` | The release index's serial is lower than the one this install already accepted, recorded in the receipt. |
| `install_failed` | The binary or the receipt could not be written into place. |
| `not_installed` | `--uninstall` found no install receipt in the Oska home. Nothing was removed. |
| `unsafe_path` | `--uninstall` found a receipt entry outside the places the installer writes, reached through a symlink, or containing `..`, or a receipt it cannot read. Nothing was removed. |

The binaries are published on oskalang.dev, together with `SHA256SUMS` and its signature, for Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64. The Linux binaries are built on Ubuntu 22.04 and need glibc 2.35 or newer. They do not run on an older glibc or on musl systems such as Alpine; there the installer stops with `unsupported_libc`, and you can build from source instead.

### The install receipt

Each install writes `receipt.json` into the Oska home (`~/.oska`, or `OSKA_HOME`). It records what the installer put there:

```json
{
  "schema": 1,
  "version": "0.3.0",
  "tag": "v0.3.0",
  "platform": "linux-x86_64",
  "installed_at": "2026-11-12T09:30:00Z",
  "site": "https://oskalang.dev",
  "key_fingerprint": "SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o",
  "sha256": "<SHA-256 of the installed oskac>",
  "installer": "sh",
  "index_serial": 0,
  "files": [
    {"path": "/home/you/.oska/bin/oskac", "kind": "binary", "sha256": "<SHA-256>"},
    {"path": "/home/you/.oska/version", "kind": "record", "sha256": "<SHA-256>"},
    {"path": "/home/you/.oska/receipt.json", "kind": "record"}
  ]
}
```

- `key_fingerprint` is the fingerprint of the release key that vouched for the binary, and `sha256` is the binary's SHA-256, so you can tell whether it has changed since.
- `installer` is `sh` or `ps1`.
- `files` lists every file the installer wrote, with its SHA-256 where the content is the installer's own.
- The installer also still writes the one-line `version` file, for one more release, so that tools that read it keep working.

The receipt is advisory. The installer never trusts it to verify anything: every install checks the signature, the signed tag line, the checksum and the binary's version, whatever the receipt says. It only decides which version counts as installed, for the downgrade guard and `previous_version`, what `--uninstall` may remove, and records in `index_serial` the highest release index serial accepted, so that an older index is refused. When a receipt and a `version` file disagree, the receipt wins.

### Verify the installation

```sh
oskac --version
```

If `oskac` is not found, add `~/.oska/bin` to your `PATH`. The [quickstart](quickstart.md) runs a first program.

### Update

Re-run the installer. It installs the latest release over the old one, and with `--version` it installs a specific one. It refuses to install a release older than the installed one with `downgrade_refused`. So if you installed a pre-release with `--version`, a plain re-run keeps it until a newer release comes out. To go back to an older release:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.2.0 --allow-downgrade
```

There is no self-update command.

### Check for an update

`--check` reports the installed and the latest version, and installs and writes nothing:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --check --json
```

```json
{"ok": true, "action": "checked", "installed": "0.3.0", "latest": "0.4.0", "tag": "v0.4.0", "update_available": true, "index_verified": true, "index_serial": 4, "index_expires": "2027-02-11T10:00:00Z"}
```

- `installed` is `null` when no `oskac` is installed.
- `update_available` is `true` when the latest release is newer than the installed one, or nothing is installed. It exits 0 either way.
- It takes the latest release from the signed index, with the same checks as an install, so it fails with `bad_index`, `stale_index` or `rollback_refused` as an install would. Before the site publishes an index, it reads `latest.json`, and `index_verified`, `index_serial` and `index_expires` are `false`, `null` and `null`.

### Uninstall

Run the installer with `--uninstall`. It removes what the [install receipt](#the-install-receipt) lists, and nothing else:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --uninstall
```

```powershell
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --uninstall
```

It downloads nothing, so a saved copy of the installer works offline. Set `OSKA_HOME` as you did to install, and it uninstalls from there.

- **It removes only what the receipt lists**, and only files directly in the Oska home or its `bin` directory. Then it removes `bin` and the Oska home if they are left empty. It never removes a directory with anything in it.
- **It checks everything before it removes anything.** An entry outside those places, a path through a symlink, or a path with `..` stops it with `unsafe_path`, and nothing is removed. With no receipt it stops with `not_installed`.
- **It keeps a file that changed** since it was installed: one whose SHA-256 is no longer the one the receipt recorded.
- **It never touches `~/.config/oska`,** which holds settings and credentials.
- **It does not edit your `PATH`.** If the `bin` directory is on it, the installer says so, and you remove the line that adds it from your shell profile (on Windows, the entry from your user `PATH`).

With `--json` it prints:

```json
{"ok": true, "action": "uninstalled", "removed": ["/home/you/.oska/bin/oskac", "/home/you/.oska/version", "/home/you/.oska/receipt.json", "/home/you/.oska/bin", "/home/you/.oska"], "kept": [], "path_hint": null}
```

- `removed` lists every file and directory it removed.
- `kept` lists what it left in place: a file that changed, the `bin` directory or the Oska home if something else is in it, and `~/.config/oska` if it exists.
- `path_hint` says what to remove from your `PATH`, or is `null` when the `bin` directory is not on it.

An install made before the installers wrote a receipt has none, so `--uninstall` stops with `not_installed`. Delete `~/.oska` by hand: it holds `bin/oskac` (`bin\oskac.exe` on Windows) and a `version` file.

## Upgrading to v0.3.0

v0.3.0 adds the native backend. Programs keep their meaning; these are the changes you can see:

- **`run` and `test` compile natively on aarch64 Linux.** Elsewhere, and with `--interpret`, they run on the interpreter as before. A program the native backend cannot compile yet runs on the interpreter after a one-line note (`E0390`). `oskac build` writes the executable. See [platform support](cli-reference.md#platform-support).
- **A closed stdout pipe ends the program quietly with exit code 0** (`oskac run x.osk | head -1`), on both backends. Before, `oskac` panicked (#86).
- **`String.to_lowercase` maps each character on its own**, so a Greek capital sigma at the end of a word becomes `σ`, not `ς`, on both backends.
- **`for` over a range of anything but integers is refused by the checker** (`E0235`); the interpreter used to fail at run time.

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
- **Contracts are now enforced.** In v0.1.0 a `requires` or `ensures` clause was type-checked but never evaluated, so a contract that was silently false did nothing. In v0.2.0 every clause runs on every call, in every build, including `--release`. A false `requires` panics with `P0006` at the call, and a false `ensures` panics with `P0007` at the clause, both with exit code 101. A program that relied on a wrong contract being ignored now panics: fix the call or the function, or correct the contract. Two placements that v0.1.0 accepted are now errors: a contract on a method of `impl Trait for Type` (`E0218`, move it to the trait's declaration of the method, where it binds every impl) and a contract on `main` (`E0219`). A `?` or `return` directly in a contract is `E0282`. A contract's `==`, `!=` and interpolation now count as calls of the `Eq` and `Display` impls they reach, so using them on a capability-bearing value is `E0280`.
- **Capabilities can no longer hide in generic code or in closures.** v0.1.0 let some programs pass a capability where only pure code was allowed, and these are now errors:
  - A type argument that breaks what a generic assumes of its type parameter is the new `E0274`: a capability-bearing type for a parameter that a contract uses, or that a closure used as a plain `fn` captures, and a type holding a `Scope` for a parameter that an unscoped closure captures. That includes a trait's `Self`: a capability-bearing type cannot implement a trait whose contract uses `self`. Pass the capability as its own parameter, or take a `cap fn`.
  - A closure is a `cap fn` when something it captures turns out capability-bearing later in the body, as in `let g = || xs` before `xs.push(out)`. If it was already used as a plain `fn`, that is `E0270` at the closure. A value already used as a plain `fn` also rejects a `cap fn` assigned to it later (`E0270`).
  - A mutating method writes its receiver back, so `push` of untrusted text into a `List<String>` is now `E0300`, and `push` of a `cap fn` into a `List<fn()>` is `E0270`. Declare the list with `Untrusted<String>` or `cap fn`.
  - A closure that captures a value whose type turns out to hold a `Scope` only later in the body, as in `var xs = []` and `let g = || xs.len()` before `xs.push(s)`, is `E0292`. Declare the value with its type first.

  Where values meet, a plain `fn` and a `cap fn` now join to `cap fn` instead of being a type error, so some programs that v0.1.0 rejected now check.

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
