# Call-style constructors instead of brace literals

Structs and enum variants are constructed with call syntax, and every named field must be named: `Point(x: 1, y: 2)`, `Shape.Circle(radius: 1.0)`. Tuple-style declarations are constructed positionally: `Some(x)`, `Meters(3)`. Patterns use the same syntax.

We chose this over Rust-style `Point { x: 1, y: 2 }`. In Rust, a brace literal is ambiguous after `if`, `while`, and `match`, because the brace could also start the block. Rust resolves this with a rule that bans struct literals in those positions, which makes the syntax context-sensitive. Principle 3 forbids context-sensitive syntax. With call style, construction looks the same for structs and variants, and the parser has no special case.

## Consequences

- A construction is indistinguishable from a function call until name resolution runs. Tooling must resolve names before it can tell them apart.
- Developers coming from Rust will expect brace literals. Do not add them without revisiting this ADR.
