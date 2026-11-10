# Troubleshooting

Look up a problem by what you see, or by the diagnostic code. Every program below is checked against the real toolchain when the documentation is tested, so the code each one produces is the code shown. The complete code tables are in the [checker notes](../checker.md#diagnostic-codes), the [runtime notes](../runtime.md), and the [AST reference](../ast.md).

Tools should match on the `code`, never on the message. Read the first diagnostic first: a later one is often a consequence of it. A syntax error is reported alone.

## By symptom

### `oskac run` prints diagnostics and exits with 1

`oskac run` checks the program first and refuses to run it if the check reports anything. Run `oskac check file.osk` to see the same diagnostics with the authority report. Exit code 1 also means `main` returned `Err`; in that case the message is the error's `Display` text and no `error[...]` line appears.

### `oskac run` exits with 2 and runs nothing

The program uses something the interpreter cannot run yet, and `oskac` says so before running any of it: `E0370` for the capabilities `Net`, `NetConnect`, `NetListen`, `Spawn`, and `Sched`, `E0371` for the modules `std.sql` and `std.proc`, and `E0380` for a few constructs. The program may still pass `oskac check`. See [what the runtime refuses](capabilities.md#what-the-runtime-refuses).

### `oskac run` exits with 101

The program panicked, which means it has a bug. The last line on standard error has a `P` code, a message, and the byte range of the expression that panicked. See [debugging](debugging.md#panics).

### My `dbg` output is missing from a pipe or a file

`dbg` writes to standard error, not standard output. Redirect with `2>&1`, or read standard error.

### The program cannot read a file that exists

A restricted filesystem refuses an absolute path, an empty path, a path that climbs out with `..`, and one that goes through a symlink to outside the root. The refusal is an `Err(IoError)`, not a panic, so the program sees `Err`. Check the directory you passed to `restrict_to`, and that the path is relative to it. See [attenuation](capabilities.md#attenuation).

### `env.secret` panics with `P0020`

The variable is not set. A secret has no stand-in value, so that a missing password can never become an empty one. Set the variable, or use `env.get`, which returns an `Option`.

### A constant cannot be used

A module-level `const NAME: T = expr` declares and checks, but referring to `NAME` reports `E0221` in this release. Pass the value as a parameter, or write it in a function body.

### `{self.name}` inside a string is an unknown name

An interpolation hole that starts with `self` reports `E0221` in this release. Copy the field into a local with `let`, and interpolate the local.

### A type misspelled in a `let` annotation is not reported

In a signature, an unknown type is `E0200` with a "did you mean" fix. In a `let` annotation inside a body, this release does not report it. Do not rely on a clean check to confirm a type name in a body.

### A contract that is violated does not stop the program

`requires` and `ensures` are type-checked but not evaluated yet. Validate with an explicit `if` and return a `Result` until they are.

### `oskac` is not found

After a [source build](install.md#build-from-source), put `target/release/oskac` on your `PATH`. After an installer, put `~/.oska/bin` on it.

### The documentation says a command exists, but it does not

Anything marked **Available from v0.1.0** is the decided design for the first release and is not in the current build.

## By code

### Syntax: `E00xx` and `E01xx`

A syntax error is reported alone. Newlines end statements, and there is no `;`.

`E0005`, a `;` at the end of a line. The fix removes it.

```oska error=E0005
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hi");
    Ok(())
}
```

`E0102`, a string interpolation that holds more than a name or a field path. Compute it with `let` first.

```oska error=E0102
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("total {1 + 2}")
}
```

`E0104`, a `let` or `var` without an initializer. Write `let x: Int = 0`.

```oska error=E0104
fn main() {
    let x: Int
}
```

`E0120`, fields or variants not separated by commas, even on separate lines. The fix inserts the comma.

```oska error=E0120
struct Point {
    x: Int
    y: Int
}
```

`E0143`, a comma after a `match` arm. Arms are one per line, with no commas. The fix removes it.

```oska error=E0143
fn flip(b: Bool) -> Int {
    match b {
        true => 1,
        false => 0
    }
}
```

`E0140`, an or-pattern such as `A | B`. This release has no or-patterns, range patterns, or `@` bindings (`E0141`, `E0142`). Write one arm for each.

```oska error=E0140
fn small(n: Int) -> Bool {
    match n {
        1 | 2 => true
        _ => false
    }
}
```

`E0001`, a character that is not part of the language, such as `$`. `E0002`, an unterminated string.

### Names and types: `E020x` and `E022x`

`E0200`, an unknown type. The fix suggests the closest declared name.

```oska error=E0200
fn shout(text: Strng) -> Int {
    1
}
```

`E0220`, a type mismatch. There are no implicit conversions, so convert explicitly.

```oska error=E0220
fn main() {
    let n: Int = "four"
}
```

`E0221`, an unknown name. The fix suggests the closest local, function, or variant. A capability you did not declare is this error, and so is a `const`.

```oska error=E0221
fn main(io: Stdout) -> Result<(), IoError> {
    io.println(greeting)
}
```

`E0223`, an operator that does not apply to its operands. `+` works on numbers only, so build text by interpolation.

```oska error=E0223
fn join(a: String, b: String) -> String {
    a + b
}
```

`E0224`, the wrong number of arguments. `E0228`, an unknown method, variant, or associated function, with a "did you mean" fix. `E0227`, an unknown field.

```oska error=E0228
fn main() {
    let xs: List<Int> = List.new()
    let n = xs.length()
}
```

`E0233`, a type the checker cannot infer. Add an annotation.

```oska error=E0233
fn main() {
    let xs = List.new()
}
```

`E0265`, a type that does not implement a needed trait. A common case is interpolating a `Bool`, which has no `Display`. Use `if` and print a message for each case.

```oska error=E0265
fn main(io: Stdout) -> Result<(), IoError> {
    let ready = true
    io.println("ready: {ready}")
}
```

### Mutation and results: `E0229`, `E023x`, `E024x`

`E0229`, assigning to something that is not a `var`. The fix changes `let` to `var`.

```oska error=E0229
fn main() {
    let count = 0
    count = count + 1
}
```

`E0230`, calling a mutating method on a `let` binding.

```oska error=E0230
fn main() {
    let items: List<Int> = List.new()
    items.push(1)
}
```

`E0240`, a dropped `Result`. Handle it, propagate it with `?`, or discard it on purpose with `let _ = f()`.

```oska error=E0240
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("hi")
    Ok(())
}
```

`E0242`, `?` on a `Result` whose error type is not the function's, with no `impl From` to convert it. `E0243`, `?` in a function that does not return the same kind, such as an `Option` in a function returning `Result`. Convert with `.ok_or(err)?`.

```oska error=E0243
fn first(xs: List<Int>) -> Result<Int, String> {
    let head = xs.get(0)?
    Ok(head)
}
```

### Patterns: `E025x`

`E0253`, a `match` that is not exhaustive. The message lists the missing patterns, and the fix adds an arm for each, with `todo()` as its body.

```oska error=E0253
fn name(b: Option<Int>) -> String {
    match b {
        Some(n) => "some"
    }
}
```

`E0254`, an arm that can never match because an earlier one covers it. `E0250`, a pattern that names no variant, such as a misspelled `Nnoe`: an uppercase name in a pattern is always a variant.

### Capabilities: `E021x`, `E027x`, `E028x`

`E0210`, a struct that holds a capability but is not declared `cap`. The fix inserts `cap`.

```oska error=E0210
struct Logger { out: Stdout }
```

`E0211` and `E0212`, a `main` with a parameter that is not a built-in capability or `Args`, or a return type other than `()` or `Result<(), E>` with `E` implementing `Display`.

```oska error=E0211
fn main(name: String) {
}
```

`E0270`, a closure that captures a capability passed where a plain `fn` is expected. Declare the parameter `cap fn`. `E0271`, forging a capability. `E0280`, a contract that calls a function with a capability. See [capabilities](capabilities.md).

### Trust, sinks, and secrets: `E03xx`

`E0300`, untrusted text where trusted `String` is required. Check it with a validator, or vouch for it with `.assume_trusted()`, which is an audit site.

```oska error=E0300
fn open(name: String) -> Int {
    name.len()
}

fn main(args: Args) {
    let who = args.get(1).unwrap_or("x")
    let n = open(who)
}
```

`E0310`, a sink literal whose module is not imported. The fix inserts the `use`.

```oska error=E0310
fn query() {
    let q = sql"SELECT 1"
}
```

`E0341`, text where a sink literal is required. SQL cannot be built from text. `E0350`, `E0351`, and `E0352`, a `Secret` interpolated, compared with `==`, or printed. See [untrusted data and secrets](untrusted-data.md).

### Release builds: `E0216` and `E0293`

`oskac check --release` rejects `dbg(...)` and `todo()`:

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    let n = dbg(1)
    let m: Int = todo()
    io.println("done")
}
```

```console cmd=check release stream=stderr
main.osk: error[E0216]: `dbg(...)` is not allowed in a release build (bytes 57..63)
    fix: remove `dbg`, keeping its argument
main.osk: error[E0293]: `todo()` is not allowed in a release build; finish this code first (bytes 81..87)
```

### Runtime: `E0370`, `E0371`, `E0380`, and `P` codes

These come from `oskac run` and `oskac test`, not from `oskac check`. The refusals have exit code 2. The panics have exit code 101, and under `oskac test` they fail only their own test. See [debugging](debugging.md#panics) for the panic table.

## Getting the explanation from the toolchain

`oskac explain <code>` prints the catalog entry for a diagnostic code offline: what it means, a program that fails with it, and the same program fixed. The documentation site has the same entry as one page per code at `/codes/E0300`, and all of them as data in `/codes.json`. See [the CLI reference](cli-reference.md#oskac-explain).

```sh
oskac explain E0300
```
