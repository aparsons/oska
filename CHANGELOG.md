# Changelog

Every release's notes, newest first. A change users can see adds its line under "Unreleased" in the pull request that makes it.

## Unreleased

v0.7.0 ships two Linux builds of `oskac`, so it runs on any Linux, and publishes a container image ([ADR 0022](docs/adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). These are the changes you can see:

- **The Linux release files are renamed.** `oskac-linux-x86_64` and `oskac-linux-aarch64` are gone. In their place each release has the glibc build, `oskac-linux-x86_64-gnu` and `oskac-linux-aarch64-gnu`, and the musl build, `oskac-linux-x86_64-musl` and `oskac-linux-aarch64-musl`. `latest.json` lists them under `linux-x86_64-gnu`, `linux-x86_64-musl`, `linux-aarch64-gnu` and `linux-aarch64-musl`, with no `linux-x86_64` or `linux-aarch64` key. The macOS and Windows files keep their names. A script that downloads a Linux file by name, or reads those keys, must change.
- **Fetch the installer again.** A copy of `install.sh` saved before v0.7.0 asks for the old names, so it fails with `download_failed` on v0.7.0 and later. The one-line installer always fetches the current one. The installer from v0.7.0 on still installs v0.6.0 and earlier with `--version`, but on Linux only as the glibc build, the one build those releases have: where it does not find glibc 2.35 or newer, or with `OSKA_LIBC=musl`, it refuses them.
- **The installer no longer refuses a Linux system for its C library.** It installs the glibc build where it finds glibc 2.35 or newer, and the musl build everywhere else, such as Alpine or an older glibc, where it used to stop with `unsupported_libc`. `OSKA_LIBC` chooses the build yourself. `unsupported_libc` now means only that `OSKA_LIBC=glibc` was set where the glibc build cannot run, or that a release up to v0.6.0, which has only the glibc build, was asked for where only the musl build would run or with `OSKA_LIBC=musl`. See [the two Linux builds](docs/guide/install.md#the-two-linux-builds).
- **The receipt and `oskac self info` name the Linux build.** The receipt gains `linux_build` and `libc_forced`, and `oskac self info` gains `linux_build`.
- **The image `ghcr.io/aparsons/oskac`** holds only `oskac`, runs as a non-root user, and is covered by the release signature. See [Run oskac in a container](docs/guide/install.md#run-oskac-in-a-container).
- **Bitwise operators, shifts and rotations on `U8`, `U32` and `U64`.** `&`, `|`, `^`, `~`, `<<`, `>>`, and the methods `rotl(n)` and `rotr(n)`. A shift or rotation by the type's width or more, or by a negative amount, panics with `P0008`. Mixing one of these operators with another operator, or chaining shifts, without parentheses is `E0108`, with a machine fix that adds them. The operators on any other type are `E0223`.
- **Wrapping arithmetic and integer conversions.** `wrapping_add`, `wrapping_sub` and `wrapping_mul` on `U8`, `U32` and `U64`. `to_int`, `to_u8`, `to_u16`, `to_u32` and `to_u64` convert between `Int` and the unsigned types and panic with `P0001` when the value does not fit; `wrapping_to_u8`, `wrapping_to_u16` and `wrapping_to_u32` keep the low bits of a wider unsigned value.
- **`Bytes`, a packed buffer of bytes.** `Bytes.new()`, `Bytes.zeroed(len)`, `Bytes.from_hex(text)` and `"text".to_bytes()` make one; `len`, `get`, `at`, `slice`, the endian reads `read_u16_be` to `read_u64_le`, `to_hex` and `to_text` read it; `push`, `set`, `append` and the endian pushes `push_u16_be` to `push_u64_le` change it through a `var`, in place when nothing else holds it. `Bytes` has `Eq` and `Hash`, and `dbg` shows its hexadecimal digits. An index out of bounds in `at` or `set` panics with `P0002`, a slice or read out of bounds with the new `P0042`, and a buffer that would grow past 256 MiB with the new `P0043` (not yet checked for `String.to_bytes` or the capabilities' `read_bytes`, which give a buffer of any length).
- **Secret integers: compute on keys without exposing them.** `Secret.new(value)` makes a `Secret<U8>`, `Secret<U32>`, `Secret<U64>`, `Secret<Bool>` or `Secret<Bytes>`; it is not an audit site, and `expose()` stays the only way out. On secret integers `+ - *` wrap, the bitwise and shift operators, `rotl` and `rotr` work, and a public operand of the same type gives a secret result; `ct_eq` gives a `Secret<Bool>`, which combines with `and`, `or`, `xor` and `not`, and `c.ct_select(a, b)` picks without a branch. `Secret<Bytes>` reads secret integers at public offsets, and compares with `constant_time_eq`. The checker refuses a secret that would decide a branch (`E0353`), an index, offset, length or hash key (`E0354`), a division, remainder or shift amount (`E0355`), or an ordering (`E0356`); `==` on a secret integer is `E0351`, whose fix now rewrites it to `ct_eq`. `ct_select` on two buffers of different lengths panics with `P0042`.
- **Bytes from outside are untrusted, and files can be read and written as bytes.** `DirRead.read_bytes(path)` and `FileRead.read_bytes()` give a file's bytes as they are, UTF-8 or not, as `Untrusted<Bytes>`; `DirWrite.write_bytes(path, contents)` and `FileWrite.write_bytes(contents)` write exactly the bytes given, confined as the text methods are. `Connection.send_bytes` and `receive_bytes` are declared. Untrusted bytes where trusted `Bytes` is required are `E0300`, every `Bytes` operation keeps the trust of its inputs (`to_text()` of untrusted bytes is `Option<Untrusted<String>>`), and `.assume_trusted()` on bytes vouches for them and is an audit site. `Untrusted<Bytes>` is allowed; `E0320` still rejects `Untrusted<...>` of any other type except `String`.
- **The native backend checks that secret operations compile to constant-time code.** Operations on secret integers, `Secret<Bool>` and `Secret<Bytes>`, `ct_eq`, `ct_select` and the constant-time comparisons are lowered with no branch. After optimizing, on every target, the backend follows each secret value through registers and calls and refuses to build, with the new internal error `E0398` and nothing written, if one would decide a conditional branch or the address of a load, store or call. A value stored to memory and loaded back is not followed; secrets are marked again where they are read out of their `Secret`. `E0398` is a bug in `oskac` to report; nothing in your programs changes, and the interpreter computes the same values with no timing promise.
- **`oskac doc` prints the API reference** of the prelude, `std.html`, `std.json`, `std.mcp`, `std.sql` and `std.proc`: each public item's signature, authority, description, and whether it runs or only checks. Give it a module (`std.sql`) or an item (`List.push`) to see just that, and `--format=json` for tools. The same reference is on oskalang.dev at `/api/`, with `/api.json` for agents. See [`oskac doc`](docs/guide/cli-reference.md#oskac-doc).
- **Guides for bytes and secrets.** [Bytes and binary data](docs/guide/bytes.md) and [Secrets and constant-time code](docs/guide/secrets.md) are new, and the language tour has a section on both. The `AGENTS.md` that `oskac new` writes, and the plugin's `oska` skill, now say to keep binary data in `Bytes` and key material in a `Secret`.
- **`Approval` and `Approver`: a person's yes to one action.** `Approver.terminal(stdin, stderr)` asks on standard error, never standard output, and reads the answer from standard input; `approver.confirm(summary, details)` shows the summary and each detail as `dbg` shows text, so control and bidirectional characters appear as escapes, and returns `Some(Approval)` only on `y` or `yes`. An `Approval` cannot be made by hand. A tool calls `approval.consume(summary, details)`, which succeeds once: a second use, of the approval or any copy of it, is refused with `ApprovalError.Used`, and an approval for a different action with `ApprovalError.Mismatch`. In a test, `t.approver` answers from `t.answer_approval(yes)` and `t.approvals_asked()` gives the summary and details of each question it was asked. `TestContext` gains the field `approver`. See [Approvals](docs/guide/capabilities.md#approvals).
- **`std.json`: read and write JSON safely.** `use std.json`, then `json.parse(text)` gives a `json.Value` or a `json.Error` with a byte offset and a reason. Every string it reads, keys included, is untrusted text, while numbers and booleans are trusted; `Number.to_int` gives `None` for a fraction, an exponent or an integer too large for an `Int`. Input over `json.MAX_LENGTH` (8 MiB), nesting deeper than `json.MAX_DEPTH` (128) and a key repeated in one object are errors, never a panic. `json.encode(value)` writes canonical JSON on one line, escaping every control character, U+2028 and U+2029. `json.Schema` builds closed object, string, integer, number, boolean, enumeration and list schemas, prints them as JSON Schema, and `validate`s a value, giving the first violation's path; `json.decode(text, schema, read)` does all three. The module is written in Oska and runs on both backends.
- **`Budget`, authority to spend model tokens and calls.** `main` may declare a `Budget`, which is unlimited; `budget.limit(tokens, calls)` gives a child budget clamped to what its parent has left, listed as an attenuation in the authority report, and `t.budget` in a test has 100000 tokens and 100 calls. `reserve(tokens)` holds tokens and one call before a request, refusing with `OverBudget` when the budget or any budget it was limited from has less left, or has no tokens left at all; the `Reservation` it gives is settled once, with `debit(input, output, cache_creation, cache_read)` or `release()`. `remaining()` and `spent()` report the counters, which every copy of a budget shares. `std.llm` will spend from it. See [Budgets](docs/guide/capabilities.md#budgets).
- **`std.mcp`: tools an agent can call, each with its own authority.** `use std.mcp`, then implement `mcp.Toolset` for a `cap struct` holding the narrowed capabilities your tools need: `specs` lists each tool's `mcp.ToolSpec` (a name, a description, an input `json.Schema` and an optional title), and `call(name, args)` runs one and returns an `mcp.ToolResult` (`text`, `json` or `error`; its text is untrusted). Write `call` as a `match` on the name with one string-literal arm per tool, and `oskac check` lists each tool under `call` with the capabilities its arm reaches, with the default arm, and flags a tool that writes to standard output, a spec with no arm and an arm with no spec, or says the names were not checked when `specs` does not name them with literals; JSON reports carry the same under a new `toolset` key on `call`'s entry. A `call` of any other shape is reported once for the whole toolset, marked as not split per tool. `mcp.serve(stdin, stdout, info, tools)` then makes the program an MCP server over standard input and output (see the next entry).
- **`std.mcp`: serve a `Toolset` over MCP.** `mcp.serve(stdin, stdout, mcp.ServerInfo.new(name, version), tools)` reads one JSON-RPC message per line and answers each, until standard input closes, for any MCP client, in protocol 2026-07-28 (`server/discover`, `_meta` on every request, `-32602` for missing fields and `-32022` for another version) and 2025-11-25 (`initialize`, `notifications/initialized`, `ping`), with `tools/list` and `tools/call`. It first returns an `mcp.McpError`, before it reads anything, for two tools with one name, a name that is not 1 to 128 ASCII letters, digits, `_` and `-`, or an input schema that is not an object. It answers an unknown tool itself and checks the arguments of every call against the tool's schema, so `Toolset.call` only sees valid arguments and a name from its own specs; failed arguments are a tool error (`isError`) naming the field. Each tool's `readOnlyHint`, `destructiveHint` and `openWorldHint` are derived from the capabilities the checker computed it reaches, and cannot be written by hand. `mcp.handle(session, tools, line)`, with a session from `mcp.Session.new(info, tools)`, is the per-message function, public so a program's tests can drive its server with recorded lines and no process. `examples/notes_server.osk` is a worked server with its authority report and tests. A tool that reaches `Stdout` still writes into the protocol stream: the report flags it. A message line over `mcp.MAX_LINE` (1 MiB) gets `-32600`, but standard input still buffers the whole line first, so this keeps the framing and does not bound memory. A message with no `id` never gets a reply. `serve` lists the tools once and serves that list.
- **The authority report names a trait of a `std` module written in Oska by its `std` path**, as in `<Notes as std.mcp.Toolset>.call`. Building a struct of such a module whose fields are private, such as `json.Number`, is `E0360`, with no fix, instead of `E0206` with a fix that would edit the module.
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
