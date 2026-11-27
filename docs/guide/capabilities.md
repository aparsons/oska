# Capabilities and authority

Oska has no **ambient authority**. Reading a file, opening a connection, reading the clock, and generating a random number are all effects, and code can perform one only if it holds the **capability** for it. A capability is an unforgeable value. Holding it is the only way to perform its effect, and a function's capability parameters are its effects. Oska has no separate effect annotations ([ADR 0002](../adr/0002-capabilities-are-the-effect-system.md)).

## The built-in capabilities

| Capability | Grants | Runs in this release |
| --- | --- | --- |
| `Stdin`, `Stdout`, `Stderr` | The standard streams | Yes |
| `Fs` | The filesystem, but only to attenuate: it reads and writes nothing itself. Attenuates to `FsRead`, `FsWrite`, a directory capability (`Dir`, `DirRead`, `DirWrite`), or a file capability (`FileRead`, `FileWrite`). | Yes |
| `Env` | Environment variables, which can hold secrets | Yes |
| `Clock` | The time: `now_millis()` | Yes |
| `Random` | Pseudo-random numbers: `next_int()`, and secure bytes for keys: `secure_bytes(n)` | Yes |
| `Budget` | Spending model tokens and calls. Unlimited in `main`; attenuates to a smaller `Budget` with `limit(tokens, calls)`. See [Budgets](#budgets). | Yes |
| `Net` | The network, but only to attenuate: it connects to nothing itself. Attenuates to `NetConnect`, `NetListen`, or a host capability (`HostConnect`). | Every host; native on Linux, macOS and Windows |
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
fn word_count(docs: DirRead, path: Path) -> Result<Int, IoError> {
    let text = docs.read_to_string(path)?
    Ok(text.split(" ").len())
}

fn double(n: Int) -> Int {
    n * 2
}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {
    let docs = fs.restrict_to("notes").read_only()
    let words = word_count(docs, Path.new("today.txt"))?
    io.println("{words}")?
    Ok(())
}
```

```console cmd=check
fn word_count: DirRead (docs)
fn double: pure
fn main: Stdout (io), Fs (fs)
  main.osk:11:16: Fs -> Dir: restrict_to "notes" (bytes 250..273)
  main.osk:11:16: Dir -> DirRead: read_only (bytes 250..285)
```

Reading the report: `word_count` can only read beneath one directory, `double` can do nothing, and `main` can write to standard output and attenuate the filesystem. The indented lines under `main` are **attenuations**: they narrow `Fs` to a `Dir` for `notes`, then to a `DirRead`. Every attenuation is listed under the function it is in. Reviewing a change means reading the report, not the bodies.

For a larger example, this site is built by an Oska program, `site/build.osk`. Its [authority report](https://oskalang.dev/authority.html) is printed by `oskac check` on every build and published as it was printed.

### Unused authority

A report is only worth reviewing if it is honest, so a function must use every capability it takes. A capability-bearing parameter that the function never uses is **unused authority**, and the release check, `oskac check --release`, rejects it with `E0296`. Any use counts: calling a method on it, passing it on, attenuating it, storing it, returning it, or capturing it in a closure. Discarding it with `let _ = net` is not a use, and neither is reading a local that shadows it. An ordinary `oskac check` says nothing, so a function you are still writing can take a capability before its body uses it.

```oska
fn greet(io: Stdout, net: Net) -> Result<(), IoError> {
    io.println("hello")
}

fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    greet(io, net)
}
```

```console cmd=check release stream=stderr
main.osk:1:22: error[E0296]: unused authority: `greet` never uses its parameter `net`, so it declares authority it does not exercise; remove the parameter (bytes 21..29)
    fix (maybe): remove the parameter `net`
```

`main` passes `net` on, so it uses it; `greet` never does. Remove the parameter, and then `main`'s `net` is unused too. On `main` the fix is `machine`, since nothing calls `main`, and `oskac fix --release` applies it; on any other function it is `maybe`, because its callers change too. A method whose signature a trait fixes is exempt. Naming a parameter `_net` does not silence it: a capability a function does not use is removed, not hidden ([ADR 0016](../adr/0016-unused-authority-is-a-release-check-error.md)).

A `cap struct` can hold unused authority too. A capability-bearing field that no code in the program reads is rejected by the release check with `E0297`. Any read counts, anywhere in the program: a field access, which covers calling a method on the field and passing it on, or a `match` pattern that binds the field to a name that is then read. Discarding it, with `let _ = tools.net` or with `_` in a pattern, is not a read.

```oska
cap struct Tools {
    io: Stdout,
    net: Net,
}

fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let tools = Tools(io: io, net: net)
    tools.io.println("hello")
}
```

```console cmd=check release stream=stderr
main.osk:3:5: error[E0297]: unused authority: no code reads the field `net` of `Tools`, so it holds authority the program does not exercise; remove the field (bytes 39..47)
    fix (maybe): remove the field `net`
```

The fix is `maybe`, because every construction of `Tools` changes too. Remove the field, and then `main`'s `net` is unused.

## Attenuation

You cannot forge a capability. You get one by being passed it, or by **attenuation**: producing a narrower capability from one you hold. Attenuation never widens authority, and there is no way back from a narrow capability to a wider one.

- `fs.read_only()` gives an `FsRead`, and `fs.write_only()` gives an `FsWrite`.
- `fs.restrict_to("dir")` gives a `Dir`, a **directory capability** that reaches only `dir` and what is beneath it. `read.restrict_to("dir")` on an `FsRead` gives a `DirRead`, and `write.restrict_to("dir")` on an `FsWrite` gives a `DirWrite`. The directory must be trusted text, because it decides where the program may reach (a **structural position**).
- A `Dir` gives a `DirRead` with `read_only()` and a `DirWrite` with `write_only()`. Each directory capability narrows to a subdirectory with `restrict_to(path)`, which takes a `Path` and returns its own type.
- `dir.with_lock(|d: DirWrite| ...)` on a `DirWrite` gives one writer at a time. It takes an exclusive lock on the directory, calls the closure with the same capability, and releases the lock when the closure returns or panics (or the program ends or crashes). It gives `Ok` of what the closure returned. A second lock on the same directory, from this program or another, fails at once with an `IoError` that begins `L0001`. It narrows nothing and widens nothing: the authority report lists it as an attenuation of `DirWrite` to `DirWrite`. See [the runtime reference](../runtime.md).
- `read.restrict_to_file("name")` on an `FsRead` gives a `FileRead`, and `write.restrict_to_file("name")` on an `FsWrite` gives a `FileWrite`: a **file capability** for exactly one file. A `DirRead` or `DirWrite` hands out one for a file beneath it, with a `Path`.
- `net.connect_only()` and `net.listen_only()` narrow `Net` to `NetConnect` and `NetListen`. `net.restrict_to("host", 443)` on `Net` or `NetConnect` gives a `HostConnect`, a **host capability** for one host and port, and only that can `connect()`. The host must be trusted text, because it decides where the program connects (a structural position, so untrusted text there is `E0300`). `listen.restrict_to_loopback(port)` on `NetListen` gives a `LoopbackListen`, which accepts connections from this machine only. `Net`, `NetConnect` and `NetListen` have no `connect` or `accept` of their own, and every narrowing is in the authority report. See [networking](networking.md) and ADR 0025.

`Fs`, `FsRead`, and `FsWrite` only attenuate. They have no `read_to_string`, `list_dir`, `write`, or `create_dir`, so every read and write goes through a directory or file capability whose root the code names. Calling one of those methods on them is `E0273`, which shows the shape of the fix:

```oska error=E0273
fn load(fs: FsRead, path: Path) -> Result<Untrusted<String>, IoError> {
    fs.read_to_string(path)
}
```

### Four patterns

**A directory.** Narrow to the directory the code needs, and pass the directory capability down:

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
  main.osk:6:17: Fs -> Dir: restrict_to "notes" (bytes 174..197)
  main.osk:7:10: Dir -> DirWrite: write_only (bytes 207..225)
```

The report shows `main` restricting the filesystem to `notes`, so a reviewer sees where the program can reach without reading its code. A root that is a string literal or a module constant is printed; any other root, such as one built from a variable, is printed as `computed at runtime`. `read_only` and `write_only` take no root.

**A subdirectory from untrusted input.** Narrowing an existing directory capability takes a `Path`, which may come from untrusted text through `Path.from`, because the result stays beneath the same root. Per-user confinement needs no trust assertion:

```oska
fn avatar(uploads: DirRead, user: Untrusted<String>) -> Result<Untrusted<String>, IoError> {
    uploads.restrict_to(Path.from(user)).read_to_string(Path.new("avatar.txt"))
}
```

**A single file.** When a function needs one file, give it a file capability. Its `read_to_string()` and `write(text)` take no path, so the signature says exactly which file it can touch:

```oska
fn show(readme: FileRead, io: Stdout) -> Result<(), IoError> {
    io.println(readme.read_to_string()?)
}

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    show(fs.restrict_to_file("README.md"), io)
}
```

A file capability reads or writes, never both, and it cannot narrow further.

**The whole filesystem.** Code that really needs to reach anywhere, such as a file viewer, writes `restrict_to("/")`. Paths are then relative to `/`, and the authority report shows the root, so a reviewer cannot miss it:

```oska
fn view(everything: DirRead, path: Path) -> Result<Untrusted<String>, IoError> {
    everything.read_to_string(path)
}

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let text = view(fs.restrict_to("/"), Path.new("etc/hostname"))?
    io.println(text)
}
```

```console cmd=check
fn view: DirRead (everything)
fn main: Stdout (io), FsRead (fs)
  main.osk:6:21: FsRead -> DirRead: restrict_to "/" (bytes 197..216)
```

### What the runtime refuses

A directory or file capability resolves every path against its root and refuses an absolute or empty path, one that climbs out with `..` (`a/../b`, which stays inside, still works), and any symlink beneath the root, whether it points outside, inside, or nowhere; on Windows, junctions, mount points and other reparse points are refused as symlinks are. It also refuses to read or write an existing file with more than one hard link, since a hard link planted beneath the root could be a file outside it; a write that creates a new file is unaffected. A refusal is an `Err(IoError)`, not a panic. A root that cannot be opened, because it is missing, a symlink, or the wrong kind, gives a capability whose every operation returns an `IoError` naming the root, so create a directory before you narrow to it.

The capability holds an OS handle for its root, and each path is opened one component at a time beneath it without following a symlink, or on Windows any other reparse point. There is no separate check to race against: a process that swaps a directory for a symlink or a junction while the program runs cannot steer an open outside the root. This holds on Linux, macOS and Windows. See the [runtime notes](../runtime.md#capabilities).

The in-memory filesystem in a test confines the same way:

```oska tests
test "a directory capability refuses to climb out" {
    let docs = t.fs.restrict_to("safe").read_only()
    let outcome = docs.read_to_string(Path.new("../secret.txt"))
    t.assert(!outcome.is_ok())
}
```

## Budgets

A `Budget` is the authority to spend model tokens and calls. `std.llm` takes one for every request, so a function that can call a model says so in its signature. `main`'s budget is unlimited. `budget.limit(tokens, calls)` gives a child budget that draws from its parent and is clamped to what the parent has left, so attenuating a budget never widens it, and the authority report lists each `limit` call as an attenuation. Tokens are counted, not money, because prices differ by model and change.

A budget's counters live in the runtime, not in its value, so every copy of a budget draws from the same counters. Before a request, `reserve(tokens)` holds that many tokens and one call. It returns `OverBudget` when the budget, or any budget it was limited from, has less left or no tokens left at all, and then nothing is spent. The `Reservation` it returns is settled once: `debit(input, output, cache_creation, cache_read)` spends the call and every token the reply used, cache tokens included, and returns the rest, and `release()` returns everything for a request that was never sent. `remaining()` gives the tokens and calls left (`None` where there is no limit), and `spent()` what was spent.

```oska tests
fn ask(budget: Budget, tokens: Int) -> Result<(), OverBudget> {
    let reservation = budget.reserve(tokens)?
    reservation.debit(tokens, 0, 0, 0)
    Ok(())
}

test "a child budget stops at its limit" {
    let child = t.budget.limit(1000, 1)
    t.assert(ask(child, 400).is_ok())
    t.assert(!ask(child, 100).is_ok())
    t.assert_eq(t.budget.spent().tokens, 400)
}
```

Input tokens are known only after a reply, so a reply can use more tokens than were reserved. They are spent all the same: a budget can end over its limit by at most what its last reply used beyond its reservation, and a budget with no tokens left refuses every further reservation, even one of no tokens.

`reserve`, `debit` and `release` are for model clients such as `std.llm`. A program that debits less than a reply used cheats only itself: it can never spend more than the budget it was given allows.

Natively, the counters of a budget that `limit` makes stay allocated until the program ends, so a program that calls `limit` in a long loop keeps 64 bytes for each call. Under `--max-memory` the native backend counts those bytes and the interpreter does not.

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

## Approvals

A tool that deletes a file or sends a message can require a person's yes. An `Approval` is that yes, for one action, and only an `Approver` makes one: its fields are private and it has no other constructor, so a function that takes an `Approval` cannot be called until a person has said yes. `Approval` and `Approver` live in the prelude, not in `std.llm`: approval is not specific to models, and any program that acts on someone's behalf can use it.

`Approver.terminal(stdin, stderr)` asks the person at the terminal. It writes the question to standard error, never to standard output, which a pipeline or an MCP server may own, so a function that takes an `Approver` has `Stdin` and `Stderr` in its authority. `approver.confirm(summary, details)` shows the trusted summary and then each untrusted detail, such as an argument a model chose, as `dbg` shows text: in double quotes, with control, bidirectional and other invisible characters as escapes such as `\n` and `\u{202e}`, so neither can disguise itself. It returns `Some(Approval)` only when the answer is `y` or `yes`.

A terminal approver trusts whoever writes to its standard input. When that is a pipe, a script or another program, the "yes" need not come from a person, so build one only when a person is at the terminal, and never over the standard input of an MCP server, which the client writes.

An approval is single use. The tool it is given to calls `approval.consume(summary, details)`, which succeeds once. A second use, of the approval or of any copy of it, is refused with `ApprovalError.Used`, and an approval given for a different summary or different details with `ApprovalError.Mismatch`, which leaves it unused. `matches(summary, details)` says whether `consume` would succeed, without using it, so it is false once the approval is used; a tool that acts calls `consume`. `summary()` and `details()` say what was approved.

```oska tests
fn delete(approval: Approval, path: Untrusted<String>) -> Result<(), ApprovalError> {
    approval.consume("delete a file", [path])?
    Ok(())
}

test "one yes covers one action" {
    t.answer_approval(true)
    match t.approver.confirm("delete a file", ["notes.txt"]) {
        Some(approval) => {
            t.assert(delete(approval, "notes.txt").is_ok())
            t.assert(!delete(approval, "notes.txt").is_ok())
        }
        None => t.fail("the script said yes")
    }
    t.assert_eq(t.approvals_asked().at(0).details, ["notes.txt"])
}
```

In a test, `t.approver` answers from the test's script instead of a terminal: see [Testing](testing.md).

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

A plain `fn` may always be used where a `cap fn` is expected. When a plain `fn` and a `cap fn` meet in one list, in the branches of an `if` or `match`, or as arguments of a generic for one type parameter, the result is a `cap fn`: `[pure_fn, capturing_closure]` is a `List<cap fn() -> Int>`. Writing `List<fn() -> Int>` as the type still rejects the capturing closure. A `cap fn` parameter appears in the authority report as `cap fn (name)` with no capabilities, because a closure's capabilities are not written in its type, but it still makes the function impure.

A generic function can perform effects through a bounded type parameter: `run` below calls `tools.call`, and `T` may be a `cap struct` whose `call` reads files. The report lists such a parameter as `generic T (name)`, so the function is not pure. An unbounded type parameter has no methods to call, so it leaves a function pure.

```oska
trait Toolset {
    fn call(self, name: String) -> Result<Untrusted<String>, IoError>
}

fn run<T: Toolset>(tools: T) -> Result<Untrusted<String>, IoError> {
    tools.call("a.txt")
}
```

```console cmd=check
fn run: generic T (tools)
```

## You cannot forge one

Constructing a built-in capability yourself is `E0271`, and reading its private fields is `E0272`.

```oska error=E0271
fn main() {
    let io = Stdout(handle: 1)
}
```

## What the runtime refuses

A program that declares `Spawn` or `Sched` in `main` passes `oskac check` but is refused by `oskac run` with `E0370` and exit code 2, before anything runs.

```oska
fn main(spawn: Spawn) -> Result<(), IoError> {
    Ok(())
}
```

```console cmd=run stream=stderr
main.osk:1:46: error[E0370]: `oskac run` cannot grant `Spawn` yet: the capability has no runtime (bytes 45..59)
```

`Net` has a runtime, on both backends. A program that connects goes through a host capability. The host is an IP address or a DNS name, which both backends resolve with `std.dns`:

```oska fragment
fn main(io: Stdout, net: Net) -> Result<(), IoError> {
    let connection = net.restrict_to("example.com", 443).connect()?
    connection.send("hello")?
    io.println("sent")
}
```

## Two exceptions

Two things are deliberately not capabilities: `dbg(expr)`, which writes to standard error in debug builds and tests and is a compile error under `--release`, and panics, which stop the program but have no effect on the outside world and cannot be caught. See [debugging](debugging.md).
