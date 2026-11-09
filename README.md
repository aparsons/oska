# Oska

**An AI-first, secure-by-default programming language.**

> Status: pre-alpha. Nothing here is stable; the language design is being worked out in [`docs/design.md`](docs/design.md).

## Why Oska?

Most code is increasingly written, reviewed, and refactored with the help of AI. Oska is designed for that world:

- **Secure by default.** No ambient authority: a function can only touch the filesystem, network, environment, or clock if it is explicitly handed a capability to do so. Memory safety, no nulls, no implicit conversions, and checked arithmetic are the baseline, not opt-ins.
- **AI-first.** The syntax is regular and unambiguous, so it is easy for models to generate and for humans to verify. Effects, capabilities, and contracts are visible in signatures, so a reviewer (human or model) can tell what a function *can* do without reading its body. Compiler diagnostics are available as structured JSON with suggested fixes.
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
| `examples/` | Example `.osk` programs |
| `docs/` | Language design notes |

## Building

The compiler is written in Rust (stable toolchain).

```sh
cargo build
cargo run -p oskac -- lex examples/hello.osk
cargo test
```

## License

Apache-2.0. See [LICENSE](LICENSE).
