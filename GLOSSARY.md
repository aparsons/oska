# Oska

Oska is a secure-by-default programming language designed for AI agents to write and humans to review. These terms describe its security model.

## Authority

**Capability**:
An unforgeable value that grants authority to perform one kind of effect, such as reading files, opening network connections, reading the clock, or generating random numbers. Holding a capability is the only way to perform its effect.
_Avoid_: permission, effect handler, token

**Ambient authority**:
Authority available to code without being explicitly handed to it, such as global state or unrestricted system calls. Oska has none.

**Root of authority**:
The program's entry point, `main`. The runtime grants `main` exactly the capabilities its signature declares, and every other capability in the program comes from those.
_Avoid_: root token, `Root`

**Attenuation**:
Producing a narrower capability from one you already hold, for example a directory capability from `Fs`. Attenuation never widens authority.
_Avoid_: derive, narrow, restrict (as nouns for the concept)

**Directory capability**:
A capability confined to one directory and everything beneath it, such as `DirRead`. All access to a directory goes through one.
_Avoid_: restricted Fs, sandboxed filesystem

**File capability**:
A capability confined to a single file, such as `FileRead`. Using it takes no path.

**Capability-bearing type**:
A type that is a capability or that contains one, directly or transitively. This includes containment through type arguments, as in `List<Stdout>`, and closures that capture a capability.

**Authority**:
For a function, the set of capabilities it can reach through its capability-bearing parameters. For a package, the union of the authority of its public functions, counted in built-in capability kinds. Which directory or file a capability is confined to is the caller's choice and is not part of a package's authority.

**Widening**:
A change after which a dependency's authority includes a capability kind it did not have, or a broader kind in place of a narrower one. A widening must be approved before the build accepts it.
_Avoid_: escalation, upgrade (for this concept)

**Pure function**:
A function with no capability-bearing parameters. It cannot perform effects, and it is deterministic.

**Pre-state value**:
A value in a postcondition captured with `old(...)` when the function is entered, before a mutating method changes its receiver.
_Avoid_: snapshot, previous value

**Program input**:
Data given to `main` that grants no authority, such as `Args`.
_Avoid_: calling it a capability

**Mutating method**:
A method that declares `var self`. It can change its receiver in place, and can only be called on a `var` binding.
_Avoid_: inout, mut ref

**Prelude**:
The built-in declarations, written in Oska, that every program can use without importing them, such as `Int`, `Option`, `Result`, and the built-in capabilities.
_Avoid_: stdlib (for this subset), builtins

**Checked program**:
A program the checker has accepted, with every name, method call, and conversion already resolved, and every expression and slot typed. Both the interpreter and the native backend run only checked programs.
_Avoid_: IR, compiled program

**Panic**:
A runtime failure that stops the current task and cannot be caught, such as integer overflow or an out-of-bounds index. A panic means the program has a bug; an `Err` returned from `main` does not.
_Avoid_: exception, crash

## Untrusted data

**Untrusted value**:
Text or bytes that came from outside the program, through a capability or a program input. Text operations keep a value untrusted. Converting it to a non-text or validated type makes it trusted.
_Avoid_: tainted, dirty, unsafe input

**Validator**:
A function that inspects an untrusted value and, only if the value passes its checks, returns a trusted value of a validated type, such as an `Email`.
_Avoid_: sanitizer

**Trust assertion**:
An explicit, audited point where code declares untrusted text trusted, normally inside a validator.
_Avoid_: untaint, sanitize, cast

**Audit site**:
A place in the source where code deliberately weakens a guarantee, such as a trust assertion or a secret exposure. Tooling lists every audit site for review.
_Avoid_: escape hatch (informal)

**Secret value**:
A value whose contents are never displayed, logged, or interpolated. The only way to read it is an explicit, auditable exposure.
_Avoid_: sensitive, private

**Secret integer**:
A secret value of a fixed-width unsigned integer type, such as `Secret<U32>`. Code may compute with it but never branch on it or index with it, so its value cannot affect timing.
_Avoid_: constant-time int, CtU32

**Sink**:
An API where data leaves the program or affects how it behaves, such as SQL queries, process execution, HTML output, filesystem paths, network hosts, or logs.

**Sink literal**:
A string literal tagged with the name of a sink, such as `sql"..."`. It provides the text for a structural position and can never interpolate values.
_Avoid_: tagged string, raw query

**Data position**:
An argument to a sink that is treated only as data. It accepts untrusted values. Examples are SQL parameters, escaped HTML text, and process arguments.

**Structural position**:
An argument to a sink that decides code, commands, paths, or destinations. It rejects untrusted values. Examples are SQL query text, a program name, `TrustedHtml`, an attenuation directory, and a network host.

## Packages

**Module**:
One source file. Its items, methods and struct fields are private to it unless marked: an item or method with `pub(pkg)` or `pub`, a field with `pub`. The methods of an `impl` belong to the module the `impl` is in.

**Package**:
A set of modules published together from one git repository and named by its URL. Its `pub` items are its API, and its authority is counted over that API alone.
_Avoid_: crate, library (for this concept)

**Manifest**:
The file in which a package states its identity, its direct dependencies, and the authority approved for each one.
_Avoid_: package file, config

**Lockfile**:
The record of the exact version, commit, content hash and computed authority of every package in the dependency graph.

**Direct dependency**:
A package named in the manifest. Only direct dependencies need authority approval.

## Execution

**Interpreter**:
The toolchain's way of running a checked program directly, with no executable. It is the reference for what every program does.
_Avoid_: VM, evaluator

**Native backend**:
The part of the toolchain that compiles a checked program into a standalone executable for a platform, with no outside assembler, linker or C library. A program it cannot compile yet is refused, never compiled wrongly.
_Avoid_: codegen, JIT

**Supported platform**:
An operating system and processor that `oskac` is released for: Linux on aarch64 and x86_64, macOS on aarch64 and x86_64, and Windows on x86_64. Every feature works the same on each, natively, with the same security guarantees.
_Avoid_: OS

**Parity gate**:
The rule that a feature is done only when it works natively, with the same output and the same security guarantees, on every supported platform. Where operating systems differ, Oska defines the result: its own error wording, `/` in paths, no newline translation and its own exit codes.

**Host**:
The supported platform `oskac` itself is running on.

**Target platform**:
The supported platform an executable is built for. Any host can build for any target platform, and the same program built by the same `oskac` gives the same bytes on every host.
_Avoid_: triple, arch

**Runtime**:
What runs alongside a program to provide what the language promises it: memory, text, panics, the capabilities `main` is granted, filesystem confinement and the test host. For native executables it is `std.rt`, an Oska module owned by the toolchain and compiled into every executable, which no program can import; the interpreter provides the same itself.
_Avoid_: libc, runtime library

**System-call intrinsic**:
A body-less function in the runtime that performs one operating-system call. It is the only way native code reaches the operating system, and naming one anywhere outside the runtime is an error.
_Avoid_: syscall wrapper

**Differential suite**:
The tests that run every program of a corpus on both the interpreter and the native backend and fail on any difference in output, exit code, panic or files written.
_Avoid_: parity tests, golden tests (for this suite)

## Installation

**Managed install**:
An `oskac` that the installer put in place and recorded in its install receipt. Only a managed install updates or uninstalls itself; any other copy is left alone.
_Avoid_: system install, global install

**Update checker**:
The toolchain's own Oska program that fetches the signed release index and verifies releases for `oskac self check` and `oskac self update`. It is compiled when `oskac` is built and carried inside it, and it runs as a separate process holding only the network and file authority it needs.
_Avoid_: updater daemon, auto-updater
