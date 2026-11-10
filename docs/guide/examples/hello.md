# Example: hello

The smallest Oska program, in `examples/hello.osk`. It prints a greeting.

```oska file=hello.osk
// The smallest Oska program.
// `main` receives only the capabilities it declares, so this program
// can write to stdout and nothing else.

fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello from Oska!")
}
```

## What it can do

`main` receives only the capabilities it declares, so this program can write to standard output and nothing else. `oskac check` prints that as the **authority report**: one line per function, naming the capabilities it can reach and the parameter that supplies each.

```console cmd=check
fn main: Stdout (io)
```

There is no file, network, environment, or clock access to audit. A reviewer can read the whole program's authority from that one line.

## Run it

```sh
oskac run examples/hello.osk
```

```console cmd=run
Hello from Oska!
```

`io.println` returns a `Result`, so `main` returns `Result<(), IoError>` and hands the result back as its last expression.

Next: the [capabilities example](capabilities.md), or [the quickstart](../quickstart.md).
