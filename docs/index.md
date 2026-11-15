# Oska

Oska is an AI-first, secure-by-default programming language: a function can reach the filesystem, the network or the environment only through a capability it is handed, and the checker tracks untrusted text by type, so every program has a short, checkable answer to "what can this do?". It is pre-alpha.

## Install

On Linux and macOS:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://oskalang.dev/install.ps1 | iex
```

The installers check the release's signature and checksum before they install `oskac`, and install nothing if a check fails. The [install guide](guide/install.md) has the options, how to verify a release by hand, and how to build from source.

## A first program

```oska file=notes.osk
// Count the lines of notes.txt. `main` is handed the filesystem
// read-only, and passes on only the one file.
fn count(notes: FileRead) -> Result<Int, IoError> {
    Ok(notes.read_to_string()?.lines().count())
}

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let n = count(fs.restrict_to_file("notes.txt"))?
    io.println("notes.txt has {n} lines")
}
```

`oskac check notes.osk` checks the program and prints its authority report: what each function can reach, and where `main` narrows what it was given.

```console cmd=check
fn count: FileRead (notes)
fn main: Stdout (io), FsRead (fs)
  notes.osk:8:19: FsRead -> FileRead: restrict_to_file "notes.txt" (bytes 290..322)
```

`count` can read one file and nothing else: no network, no writes, no other file. A reviewer, a person or a model, can see that without reading its body.

Next, the [quickstart](guide/quickstart.md) writes, checks, runs and tests a program, and the [language tour](guide/language-tour.md) covers the syntax and types in one sitting.

## What works today

Not everything that checks runs yet. [What works today](guide/what-works-today.md) lists each feature: whether `oskac check` accepts it, whether it runs on the interpreter and natively, and on which platforms.

`std.sql`, `std.proc` and the network capabilities (`Net`) check but do not run yet: `oskac check` enforces their rules, and `oskac run`, `oskac test` and `oskac build` refuse a program that uses them. [The table](guide/what-works-today.md) lists them with the code `oskac` refuses each with.

## Security

To report a vulnerability, see the [security policy](../SECURITY.md). It also has the release key's fingerprint, to check the key the installers use.
