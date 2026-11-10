# Oska

**An AI-first, secure-by-default programming language.**

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

## Repository layout

| Path | Contents |
|---|---|
| `crates/oskac` | The `oskac` compiler CLI |
| `crates/oska-syntax` | Lexer (and, soon, parser and AST) |
| `crates/oska-check` | Type checker and authority report (`oskac check`) |
| `crates/oska-run` | Interpreter (`oskac run`) and test runner (`oskac test`) |
| `examples/` | Example `.osk` programs |
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
