# Example: capabilities

A program that passes narrower and narrower authority down, in `examples/capabilities.osk`. It counts the lines of a file named on the command line.

```oska file=capabilities.osk
// Capabilities are ordinary values. `count_lines` can read files under the
// path it is given and nothing else: no network, no writes, no env access.

fn count_lines(fs: DirRead, path: Path) -> Result<Int, IoError> {
    // File contents are untrusted: `text` is Untrusted<String>.
    let text = fs.read_to_string(path)?
    // Counting converts to Int, a non-text type, so the result is trusted.
    Ok(text.lines().count())
}

// The program's own error type. `?` converts an IoError into it through
// the explicit `From` implementation.
enum AppError {
    Usage(String),
    Io(IoError),
}

impl AppError {
    fn usage(message: String) -> AppError {
        AppError.Usage(message)
    }
}

impl From<IoError> for AppError {
    fn from(err: IoError) -> AppError {
        AppError.Io(err)
    }
}

// `main` may return `Result<(), E>` only when `E: Display`, so the runtime
// can report the failure.
impl Display for AppError {
    fn to_string(self) -> String {
        match self {
            AppError.Usage(message) => message
            AppError.Io(_) => "cannot read the file"
        }
    }
}

fn main(io: Stdout, fs: FsRead, args: Args) -> Result<(), AppError> {
    // `args` is a program input (plain data), not a capability.
    // Attenuate: hand out a capability restricted to the current directory.
    let local = fs.restrict_to(".")
    let path = args.get(1).ok_or(AppError.usage("usage: capabilities <file>"))?
    let n = count_lines(local, Path.from(path))?
    io.println("lines: {n}")?
    Ok(())
}
```

## What it can do

```console cmd=check
fn count_lines: DirRead (fs)
fn AppError.usage: pure
fn AppError.from: pure
fn AppError.to_string: pure
fn main: Stdout (io), FsRead (fs)
  capabilities.osk:44:17: FsRead -> DirRead: restrict_to "." (bytes 1342..1361)
```

Read the report as a reviewer would:

- `count_lines` can only read files beneath one directory. It cannot write, use the network, or read the environment, whatever its body does.
- The three `AppError` methods are **pure functions**: no capability-bearing parameter, so no effects, and they are deterministic.
- `main` can write to standard output and read files, and the line under it shows how far: it narrows `FsRead` to a `DirRead` rooted at `.`, the current directory. `args` does not appear because `Args` is a **program input**, plain data that grants no authority.

## How the authority narrows

1. `main` asks for `fs: FsRead`, so it can read files but not write them.
2. `fs.restrict_to(".")` is an **attenuation**: `local` is a `DirRead`, a **directory capability** that can read only under the current directory. It never widens, and `count_lines` receives `local`, not `fs`.
3. The file contents are an **untrusted value**. `text.lines().count()` is a number, so the result is trusted without any trust assertion, and the report has no audit sites.
4. `?` turns an `IoError` into an `AppError` through the explicit `impl From<IoError> for AppError`. There are no implicit conversions.
5. `main` may return `Result<(), AppError>` because `AppError` implements `Display`; the runtime prints that text and exits with code 1 when `main` returns `Err`.

## Run it

```sh
oskac run examples/capabilities.osk -- some-file.txt
```

Everything after `--` belongs to the program. `args.get(0)` is the program's own path, so the first argument is `args.get(1)`. Run on its own source from its own directory:

```console cmd=run -- capabilities.osk
lines: 49
```

With no argument the program reports its usage error and exits with 1:

```console cmd=run stream=stderr
usage: capabilities <file>
```

A path outside the current directory, or a file that does not exist, is refused by the attenuated filesystem and reported as `cannot read the file`.

Back to the [hello example](hello.md), or on to [capabilities and authority](../capabilities.md).
