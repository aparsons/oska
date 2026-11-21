# Changelog

Every release's notes, newest first. A change users can see adds its line under "Unreleased" in the pull request that makes it.

## Unreleased

v0.7.0 ships two Linux builds of `oskac`, so it runs on any Linux, and publishes a container image ([ADR 0022](docs/adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). These are the changes you can see:

- **The Linux release files are renamed.** `oskac-linux-x86_64` and `oskac-linux-aarch64` are gone. In their place each release has the glibc build, `oskac-linux-x86_64-gnu` and `oskac-linux-aarch64-gnu`, and the musl build, `oskac-linux-x86_64-musl` and `oskac-linux-aarch64-musl`. `latest.json` lists them under `linux-x86_64-gnu`, `linux-x86_64-musl`, `linux-aarch64-gnu` and `linux-aarch64-musl`, with no `linux-x86_64` or `linux-aarch64` key. The macOS and Windows files keep their names. A script that downloads a Linux file by name, or reads those keys, must change.
- **Fetch the installer again.** A copy of `install.sh` saved before v0.7.0 asks for the old names, so it fails with `download_failed` on v0.7.0 and later. The one-line installer always fetches the current one. The installer from v0.7.0 on still installs v0.6.0 and earlier with `--version`, but on Linux only as the glibc build, the one build those releases have: where it does not find glibc 2.35 or newer, or with `OSKA_LIBC=musl`, it refuses them.
- **The installer no longer refuses a Linux system for its C library.** It installs the glibc build where it finds glibc 2.35 or newer, and the musl build everywhere else, such as Alpine or an older glibc, where it used to stop with `unsupported_libc`. `OSKA_LIBC` chooses the build yourself. `unsupported_libc` now means only that `OSKA_LIBC=glibc` was set where the glibc build cannot run, or that a release up to v0.6.0, which has only the glibc build, was asked for where only the musl build would run or with `OSKA_LIBC=musl`. See [the two Linux builds](docs/guide/install.md#the-two-linux-builds).
- **The receipt and `oskac self info` name the Linux build.** The receipt gains `linux_build` and `libc_forced`, and `oskac self info` gains `linux_build`.
- **The image `ghcr.io/aparsons/oskac`** holds only `oskac`, runs as a non-root user, and is covered by the release signature. See [Run oskac in a container](docs/guide/install.md#run-oskac-in-a-container).
- **A Claude Code plugin.** `/plugin marketplace add aparsons/oska`, then `/plugin install oska@oska`, turns on the post-edit check and the Stop gate in every project and adds the loop as the `oska` skill, in place of copying the hook settings. See [the Claude Code plugin](docs/guide/agent-guide.md#the-claude-code-plugin).

## v0.6.0

Released 2026-11-20.

v0.6.0 adds the native backend for Windows, so all five supported platforms are native. It also turns the parity gate on and adds `oskac release`. These are the changes you can see:

- **`run`, `test` and `build` are native on x86_64 Windows**, as on Linux and macOS. Before, `run` and `test` used the interpreter there, tens of times slower on program logic. `--interpret` still selects the interpreter. See [platform support](docs/guide/cli-reference.md#platform-support).
- **`oskac build` on Windows writes a Windows executable, `<name>.exe`**, where v0.5.0 wrote an aarch64 Linux one. To get the old output, build with `--target linux-aarch64`. `--target windows-x86_64` builds a Windows executable from any host, and `--target all` writes five executables. The same program gives the same bytes on every host.
- **The parity gate: a program the native backend cannot compile yet is an error.** `oskac run` and `oskac test` refuse it with `E0390`, the same diagnostic `oskac build` prints, and exit code 2. Before, they ran it on the interpreter after a one-line note. If a program stops running after the upgrade, run it with `--interpret` and report the construct the message names. From this release on, a language change lands on all five platforms at once.
- **Windows file access is confined race-free.** On both backends, each path is opened one component at a time beneath the capability's open root handle. A symlink, a junction or any other reparse point, a hard-linked file, or a reserved device name such as `CON` is refused, as on Linux and macOS. Before, Windows checked a path and then opened it, so a junction swapped in between could escape the root.
- **`oskac release` builds an application for all five platforms** into one directory. It writes a `SHA256SUMS` that names the release and signs it with your SSH key, in the format `oskac`'s own releases use. The version, key and name refusals are `E0395`, `E0396` and `E0397`. See [Releasing an application](docs/guide/releasing.md).

## v0.5.0

Released 2026-11-19.

v0.5.0 adds native backends for macOS, on Apple silicon and Intel, and words every `IoError` in Oska's own text. Programs keep their meaning; these are the changes you can see:

- **`run`, `test` and `build` are native on macOS**, on both processors, as on Linux. Before, `run` and `test` used the interpreter on a Mac, tens of times slower on program logic. A program the native backend cannot compile yet still runs on the interpreter after the one-line `E0390` note, and `--interpret` still selects the interpreter. See [platform support](docs/guide/cli-reference.md#platform-support).
- **`oskac build` on a Mac writes a macOS executable for that Mac**, where v0.4.0 wrote an aarch64 Linux one. To get the old output from a Mac, build with `--target linux-aarch64`.
- **`--target macos-aarch64` and `--target macos-x86_64`** build macOS executables from any host, and `--target all` writes four executables, where it wrote two. Neither is refused with `E0394` any more. The same program gives the same bytes on every host.
- **macOS executables are ad-hoc signed**, so they run on a Mac with no Apple tools needed to build them, but Gatekeeper refuses them after a download. To distribute one, see [Signing and notarizing for macOS](docs/guide/macos-signing.md).
- **`IoError` messages no longer carry the operating system's text or error number.** The same failure now prints the same bytes on every platform and both backends. A path the failure concerns still comes first. If a test compares an error message, update its expected text from this table (the full table is in the [runtime reference](docs/runtime.md#errors-and-traits)):

  | Before (Linux) | After |
  | --- | --- |
  | `` `x`: No such file or directory (os error 2) `` | `` `x`: not found `` |
  | `` `x`: Permission denied (os error 13) `` | `` `x`: permission denied `` |
  | `` `x`: Not a directory (os error 20) `` | `` `x`: not a directory `` |
  | `` `x`: Is a directory (os error 21) `` | `` `x`: is a directory `` |
  | `` `x`: File exists (os error 17) `` | `` `x`: already exists `` |
  | `` `x`: Directory not empty (os error 39) `` | `` `x`: directory not empty `` |
  | `No space left on device (os error 28)` | `no space left on device` |
  | `Broken pipe (os error 32)` | `broken pipe` |
  | any other system error, such as `` `x`: Input/output error (os error 5) `` | `` `x`: I/O error `` |

  The messages that refuse confined access (a symlink, a hard link, a path that escapes its root) are unchanged.

## v0.4.0

Released 2026-11-15.

v0.4.0 makes x86_64 Linux the second platform with a native backend, and lets `oskac build` target either Linux platform from any host. Programs keep their meaning; these are the changes you can see:

- **`run`, `test` and `build` are native on x86_64 Linux**, as on aarch64 Linux. Before, `run` and `test` used the interpreter there, tens of times slower on program logic, and `build` could only write an aarch64 Linux executable. A program the native backend cannot compile yet still runs on the interpreter after the one-line `E0390` note, and `--interpret` still selects the interpreter. See [platform support](docs/guide/cli-reference.md#platform-support).
- **`oskac build` writes an executable for the host's platform**, so on x86_64 Linux it writes an x86_64 executable, where v0.3.0 wrote an aarch64 one. To get the old output from an x86_64 Linux host, build with `--target linux-aarch64`. On macOS and Windows, which have no backend yet, the default is still `linux-aarch64`.
- **`oskac build --target <platform>`** builds for `linux-aarch64` or `linux-x86_64` from any host, and `--target all` writes both, as `<name>-<platform>`, into the `-o` directory. An unknown platform is `E0393` and a platform with no backend yet (macOS until v0.5.0, Windows until v0.6.0) is `E0394`; both exit with 2 and write nothing. The same program built by the same `oskac` gives the same bytes on every host. See [`oskac build`](docs/guide/cli-reference.md#oskac-build).

## v0.3.0

Released 2026-11-13.

v0.3.0 adds the native backend. Programs keep their meaning; these are the changes you can see:

- **`run` and `test` compile natively on aarch64 Linux.** Elsewhere, and with `--interpret`, they run on the interpreter as before. A program the native backend cannot compile yet runs on the interpreter after a one-line note (`E0390`). `oskac build` writes the executable. See [platform support](docs/guide/cli-reference.md#platform-support).
- **A closed stdout pipe ends the program quietly with exit code 0** (`oskac run x.osk | head -1`), on both backends. Before, `oskac` panicked (#86).
- **`String.to_lowercase` maps each character on its own**, so a Greek capital sigma at the end of a word becomes `σ`, not `ς`, on both backends.
- **`for` over a range of anything but integers is refused by the checker** (`E0235`); the interpreter used to fail at run time.

## v0.2.0

Released 2026-11-10.

v0.2.0 is a **breaking change** to the filesystem capabilities, with no deprecation period ([ADR 0008](docs/adr/0008-file-access-goes-through-confined-capabilities.md)). `Fs`, `FsRead`, and `FsWrite` no longer read or write anything. They only attenuate, and every read and write goes through a directory capability (`DirRead`, `DirWrite`) or a file capability (`FileRead`, `FileWrite`). A program that calls a removed method fails `oskac check` with `E0273`, whose message shows the fix. The check cannot apply it for you, because only you know the root.

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

### Security fixes

v0.1.0 let a capability reach code that was meant to be pure. These are now errors (see the bullets above):

- A closure that captured a value whose type was not known yet could carry `Stdout` into a pure function (`E0270`).
- A generic function could hand a capability to a plain `fn` through a type parameter (`E0274`).
- A `Scope` could escape its block through a generic capture (`E0274`, `E0292`).
- `push` could add untrusted text to a `List<String>` (`E0300`).

## v0.1.0

Released 2026-11-10.

v0.1.0 is the first release of Oska, a pre-alpha. It is the first signed release, and the first with a one-line installer. These are the things you can use:

- **`oskac`, the compiler and runner.** `oskac check` type-checks a program and prints its authority report. `oskac run` runs it on the interpreter, and `oskac test` runs its `test` blocks. `oskac explain <code>` shows a diagnostic code's entry.
- **Signed binaries for five platforms:** Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64. Each release has a `SHA256SUMS` signed with the release key.
- **The one-line installers**, `install.sh` and `install.ps1`. They verify the checksum and the signature and install to `~/.oska/bin`. Run the installer again to update.
- **The site, oskalang.dev**, built by an Oska program from the repository's docs, with the guides, the error-code catalog, `llms.txt` and a Markdown copy of every page.
