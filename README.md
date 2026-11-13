# Oska

**An AI-first, secure-by-default programming language.**

**Install, docs and guides:** [oskalang.dev](https://oskalang.dev)

> Status: pre-alpha. Nothing here is stable; the language design is being worked out in [`docs/design.md`](docs/design.md), with key decisions recorded in [`docs/adr/`](docs/adr/).

## Why Oska?

Most code is increasingly written, reviewed, and refactored with the help of AI. Oska is designed for that world:

- **Secure by default.** No ambient authority: a function can only touch the filesystem, network, environment, or clock if it is explicitly handed a capability to do so. Memory safety, no nulls, no implicit conversions, and checked arithmetic are the baseline, not opt-ins.
- **AI-first.** The syntax is familiar, regular, and unambiguous, so it is easy for models to generate and for humans to verify. A function's capability parameters are its effects. Its `requires`/`ensures` contracts are part of the signature. A reviewer, human or model, can tell what a function *can* do without reading its body. There are no macros and no hidden control flow. Compiler diagnostics are available as structured JSON with suggested fixes.
- **Injection-proof by construction.** Untrusted input is tracked by type. The standard library's SQL, HTML, process, and logging APIs cannot accept untrusted input where it would change what code or command runs.
- **Auditable.** Every capability a program uses is declared at its entry point, so "what can this program do?" has a short, checkable answer.

## A taste

```oska
// examples/hello.osk
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello from Oska!")
}
```

`main` only receives the capabilities it asks for. This program can write to stdout and nothing else.

## Running it

```sh
oskac check hello.osk       # type-check and print each function's authority
oskac run hello.osk         # check, then run
oskac test hello.osk        # run the file's test blocks
oskac build hello.osk       # compile to a static aarch64 Linux executable, ./hello
```

`oskac build` works on every host: it writes the machine code and the executable itself, with no assembler, linker or C library. On aarch64 Linux (a Raspberry Pi, an arm64 server), `oskac run` and `oskac test` compile the program natively and run it, and `--interpret` selects the interpreter. Every other host runs programs on the interpreter until it has a native backend of its own. Both backends print the same output, reports and exit codes. See [platform support](docs/guide/cli-reference.md#platform-support).

## A package

A program can be one file, or a package of modules:

```text
shopping/
  oska.toml          # [package] name = "shopping", version = "0.1.0"
  src/
    main.osk         # the entry module: use pkg.receipt
    receipt.osk      # pub(pkg) fn line(...)
    pricing/
      tax.osk        # use pkg.pricing.tax
```

Run `oskac check`, `oskac run` and `oskac test` in the package directory. Items are private to their module unless marked `pub(pkg)` or `pub`. See [packages and modules](docs/guide/packages.md) and [`examples/shopping`](examples/shopping).

## Repository layout

| Path | Contents |
|---|---|
| `crates/oskac` | The `oskac` compiler CLI |
| `crates/oska-syntax` | Lexer (and, soon, parser and AST) |
| `crates/oska-check` | Type checker and authority report (`oskac check`) |
| `crates/oska-run` | Interpreter (`oskac run --interpret`, and `oskac run` on hosts with no native backend) and its test runner |
| `crates/oska-native` | Native backend: aarch64 Linux executables (`oskac build`, and `oskac run` and `oskac test` on aarch64 Linux), with the runtime `std.rt` in Oska |
| `crates/oska-sys` | The only crate with `unsafe`: the interpreter's confined filesystem calls |
| `examples/` | Example `.osk` programs and a multi-module package |
| `docs/guide/` | Task guides: [quickstart](docs/guide/quickstart.md), [language tour](docs/guide/language-tour.md), [CLI reference](docs/guide/cli-reference.md), [agent guide](docs/guide/agent-guide.md), and more |
| `docs/design.md` | Language design notes |
| `docs/adr/` | Architecture decision records |
| `docs/research/` | Background research |
| `GLOSSARY.md` | Oska's domain vocabulary |

## Building

The compiler is written in Rust (stable toolchain).

```sh
cargo build
cargo run -p oskac -- lex examples/hello.osk
cargo test
```

## License

Apache-2.0. See [LICENSE](LICENSE).
