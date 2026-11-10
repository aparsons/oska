# Quickstart

In ten minutes you will write a program, check it, see what authority it has, run it, and test it. You need `oskac`; see [install](install.md).

## 1. Write a program

Save this as `hello.osk`:

```oska file=hello.osk
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello from Oska!")
}
```

`main` is the program's **root of authority**. The runtime grants it exactly the capabilities in its signature, and here that is `Stdout`, the ability to write to standard output. Nothing else is available to this program.

## 2. Check it

```sh
oskac check hello.osk
```

```console cmd=check
fn main: Stdout (io)
```

`oskac check` type-checks the file without running it. It exits with 0 when there is nothing to report. It also prints the **authority report**: one line per function, naming the capabilities the function can reach and the parameter each one comes through. This line says that `main` can write to standard output, and that is all it can do.

## 3. Run it

```sh
oskac run hello.osk
```

```console cmd=run
Hello from Oska!
```

`oskac run` checks the program first and refuses to run it if the check reports anything.

## 4. See the sandbox work

Oska has no ambient authority, so code cannot reach a file, the network, or the clock without being handed the capability. Try to read a file without asking for one:

```oska file=hello.osk error=E0221
fn main(io: Stdout) -> Result<(), IoError> {
    let text = fs.read_to_string(Path.new("notes.txt"))?
    io.println(text)
}
```

```console cmd=check stream=stderr
hello.osk: error[E0221]: unknown name `fs` (bytes 60..62)
```

`fs` does not exist, because nothing passed it in. The fix is to ask for it in `main`'s signature, which makes it visible to every reader:

```oska file=hello.osk
fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let text = fs.read_to_string(Path.new("notes.txt"))?
    io.println(text)
}
```

```console cmd=check
fn main: Stdout (io), FsRead (fs)
```

The report now says `main` can also read files, and nothing more: it still cannot write files or open connections.

## 5. Take arguments

Command-line arguments arrive as `Args`, a **program input**. It is plain data and grants no authority. Everything after `--` belongs to the program, and `args.get(0)` is the program's own path.

```oska file=hello.osk
fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    let name = args.get(1).unwrap_or("world")
    io.println("Hello, {name}!")
}
```

```console cmd=run -- Ada
Hello, Ada!
```

Text from arguments is an **untrusted value**, so `name` cannot be passed to a function that wants a trusted `String` (`E0300`). Printing it is fine. See [untrusted data and secrets](untrusted-data.md).

## 6. Test it

Put the logic in a function, and test it in the same file. Inside a `test`, `t` gives you stand-in capabilities, so a test needs no setup:

```oska file=hello.osk tests
fn greeting(name: String) -> String {
    "Hello, {name}!"
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(greeting("world"))
}

test "greets by name" {
    t.assert_eq(greeting("Ada"), "Hello, Ada!")
}

test "main prints the greeting" {
    let _ = main(t.stdout)
    t.assert_eq(t.printed().len(), 1)
}
```

```console cmd=test
test greets by name ... ok
test main prints the greeting ... ok

2 tests: 2 passed, 0 failed, 0 skipped
```

## 7. Read the output as a machine

Every command accepts `--format=json`. Diagnostics get a stable code, a byte span, and a machine-applicable fix where one is unambiguous:

```oska file=hello.osk error=E0210
struct Logger { out: Stdout }
```

```console cmd=check json
{
  "diagnostics": [
    {"code":"E0210","message":"`Logger` holds the capability-bearing type `Stdout`, so it must be declared `cap struct`","span":{"start":7,"end":13},"fix":{"message":"declare it `cap struct`","edits":[{"span":{"start":0,"end":0},"replacement":"cap "}]}}
  ],
  "functions": [],
  "audit": []
}
```

That is the whole loop. The [agent guide](agent-guide.md) shows how to drive it from a script or an AI agent.

## Where next

- [Language tour](language-tour.md): the language in an afternoon.
- [Capabilities and authority](capabilities.md): the idea that makes Oska different.
- [Examples](examples/capabilities.md): complete programs with their authority reports.
- [Troubleshooting](troubleshooting.md): symptom and code lookup.
