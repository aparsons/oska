# Capabilities and authority

Oska has no **ambient authority**. Reading a file, opening a connection, reading the clock, and generating a random number are all effects, and code can perform one only if it holds the **capability** for it. A capability is an unforgeable value. Holding it is the only way to perform its effect, and a function's capability parameters are its effects. Oska has no separate effect annotations ([ADR 0002](../adr/0002-capabilities-are-the-effect-system.md)).

## The built-in capabilities

| Capability | Grants | Runs in this release |
| --- | --- | --- |
| `Stdin`, `Stdout`, `Stderr` | The standard streams | Yes |
| `Fs` | The filesystem. Attenuates to `FsRead`, `FsWrite`, or a directory capability: `Dir`, `DirRead`, or `DirWrite`. | Yes |
| `Env` | Environment variables, which can hold secrets | Yes |
| `Clock` | The time: `now_millis()` | Yes |
| `Random` | Pseudo-random numbers: `next_int()` | Yes |
| `Net` | The network. Attenuates to `NetConnect` or `NetListen`. | No: `E0370` |
| `Sched` | Channels and `select` | No: `E0370` |
| `Spawn` | Starting OS processes | No: `E0370` |

`Args`, the program's command-line arguments, is not a capability. It is a **program input**: plain data that grants no authority.

## `main` is the root of authority

`main` is the **root of authority**. The runtime grants `main` exactly the capabilities its signature declares, and every other capability in the program comes from those. To learn what a program can do, read one signature.

```oska
fn main(io: Stdout, clock: Clock) -> Result<(), IoError> {
    let now = clock.now_millis()
    io.println("milliseconds since the epoch: {now}")?
    Ok(())
}
```

```console cmd=check
fn main: Stdout (io), Clock (clock)
```

`main` may take only built-in capabilities and program inputs (`E0211` otherwise), and it must return `()` or `Result<(), E>` where `E` implements `Display` (`E0212`).

## Passing authority down

A function can reach only the capabilities its parameters supply. That is its **authority**, and `oskac check` prints it for every function and method. A function with no capability-bearing parameter is a **pure function**: it cannot perform effects and it is deterministic.

```oska
fn word_count(fs: FsRead, path: Path) -> Result<Int, IoError> {
    let text = fs.read_to_string(path)?
    Ok(text.split(" ").len())
}

fn double(n: Int) -> Int {
    n * 2
}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let reader = fs.read_only()
    let words = word_count(reader, Path.new("notes.txt"))?
    io.println("{words}")?
    Ok(())
}
```

```console cmd=check
fn word_count: FsRead (fs)
fn double: pure
fn main: Stdout (io), Fs (fs)
```

Reading the report: `word_count` can only read files, `double` can do nothing, and `main` can write to standard output and use the whole filesystem. Reviewing a change means reading the report, not the bodies.

For a larger example, this site is built by an Oska program, `site/build.osk`. Its [authority report](https://oskalang.dev/authority.html) is printed by `oskac check` on every build and published as it was printed.

## Attenuation

You cannot forge a capability. You get one by being passed it, or by **attenuation**: producing a narrower capability from one you hold. Attenuation never widens authority, and there is no way back from a narrow capability to a wider one.

- `fs.read_only()` gives an `FsRead`, and `fs.write_only()` gives an `FsWrite`.
- `fs.restrict_to("dir")` gives a `Dir`, a **directory capability** that reaches only `dir` and what is beneath it. `read.restrict_to("dir")` on an `FsRead` gives a `DirRead`, and `write.restrict_to("dir")` on an `FsWrite` gives a `DirWrite`. The directory must be trusted text, because it decides where the program may reach (a **structural position**).
- A `Dir` gives a `DirRead` with `read_only()` and a `DirWrite` with `write_only()`. Each directory capability narrows to a subdirectory with `restrict_to(path)`, which takes a `Path` and returns its own type.
- `net.connect_only()` and `net.listen_only()` narrow `Net`. They check, but a program that declares `Net` does not run yet.

```oska
fn save(fs: DirWrite, text: String) -> Result<(), IoError> {
    fs.write(Path.new("note.txt"), text)
}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let notes = fs.restrict_to("notes")
    save(notes.write_only(), "remember the milk")?
    io.println("saved")?
    Ok(())
}
```

```console cmd=check
fn save: DirWrite (fs)
fn main: Stdout (io), Fs (fs)
```

A directory capability is a type of its own, so `save`'s signature says it can write only beneath one directory. The runtime does the confining. A directory capability resolves every path against its root and refuses an absolute or empty path, one that climbs out with `..`, and one that goes through a symlink to somewhere else. A refusal is an `Err(IoError)`, not a panic. A root that cannot be opened, because it is missing or a symlink, gives a directory capability whose every operation returns an `IoError` naming the root.

Narrowing an existing directory capability takes a `Path`, which may come from untrusted text through `Path.from`, because the result stays beneath the same root. Per-user confinement needs no trust assertion:

```oska
fn avatar(uploads: DirRead, user: Untrusted<String>) -> Result<Untrusted<String>, IoError> {
    uploads.restrict_to(Path.from(user)).read_to_string(Path.new("avatar.txt"))
}
```

The in-memory filesystem in a test confines the same way:

```oska tests
test "a restricted filesystem refuses to climb out" {
    let fs = t.fs.restrict_to("safe").read_only()
    let outcome = fs.read_to_string(Path.new("../secret.txt"))
    t.assert(!outcome.is_ok())
}
```

One limit is documented in the [runtime notes](../runtime.md#capabilities): checking a path and opening it are separate steps, so a concurrent process that swaps a directory for a symlink at the right moment can still win a race.

## Bundling capabilities: `cap struct`

A type that holds a capability in its own fields must be declared `cap`. Leaving it out is `E0210`, and the diagnostic carries a fix that inserts `cap`.

```oska error=E0210
struct Logger { out: Stdout }
```

```oska
cap struct Logger { out: Stdout }

impl Logger {
    fn log(self, message: String) -> Result<(), IoError> {
        self.out.println(message)
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let logger = Logger(out: io)
    logger.log("started")?
    Ok(())
}
```

A method's receiver counts towards its authority, so a method on a `cap struct` is never pure. A generic container is capability-bearing when it holds a capability, so `List<Stdout>` needs no `cap`. A **capability-bearing type** is any type that is a capability or contains one, directly or through type arguments.

## Closures: `fn` and `cap fn`

A function type `fn(T) -> U` accepts only pure closures. A closure that captures a capability has the type `cap fn`, and passing it where a plain `fn` is expected is `E0270`.

```oska error=E0270
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    f(x)
}

fn noisy(out: Stdout) -> Int {
    apply(|n| {
        let _ = out.println("called")
        n + 1
    }, 1)
}
```

To accept such a closure, declare the parameter `cap fn(Int) -> Int`:

```oska
fn apply(f: cap fn(Int) -> Int, x: Int) -> Int {
    f(x)
}

fn noisy(out: Stdout) -> Int {
    apply(|n| {
        let _ = out.println("called")
        n + 1
    }, 1)
}
```

```console cmd=check
fn apply: cap fn (f)
fn noisy: Stdout (out)
```

A plain `fn` may always be used where a `cap fn` is expected. A `cap fn` parameter appears in the authority report as `cap fn (name)` with no capabilities, because a closure's capabilities are not written in its type, but it still makes the function impure.

## You cannot forge one

Constructing a built-in capability yourself is `E0271`, and reading its private fields is `E0272`.

```oska error=E0271
fn main() {
    let io = Stdout(handle: 1)
}
```

## What the runtime refuses

A program that declares `Net`, `NetConnect`, `NetListen`, `Spawn`, or `Sched` in `main` passes `oskac check` but is refused by `oskac run` with `E0370` and exit code 2, before anything runs.

```oska
fn main(net: Net) {
}
```

```console cmd=run stream=stderr
main.osk: error[E0370]: `oskac run` cannot grant `Net` yet: the capability has no runtime (bytes 18..21)
```

## Two exceptions

Two things are deliberately not capabilities: `dbg(expr)`, which writes to standard error in debug builds and tests and is a compile error under `--release`, and panics, which stop the program but have no effect on the outside world and cannot be caught. See [debugging](debugging.md).
