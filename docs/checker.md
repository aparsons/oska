# Oska checker report

`oskac check <file.osk> [--format=json] [--release]` type-checks one file against the embedded prelude. The rules it enforces are in the "Type checking" section of [`design.md`](design.md). This page describes what it prints.

The exit code is non-zero when there are diagnostics. Without `--format=json`, diagnostics go to stderr and the authority report to stdout, one function per line, such as `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`.

Type and trait names in signatures and declarations are resolved, declarations are checked (`cap`, `main`, capitalization, recursive types), and under `--release` every body is scanned for `dbg(...)`. Function, method, and constant bodies are type-checked with local, bidirectional inference: `let` types come from initializers, the expected type flows into literals, generic constructors, and closures, and an expression that fails gets an error type so it causes only one diagnostic. Test bodies and contract clauses are not checked yet.

## Report

```json
{
  "diagnostics": [
    {"code":"E0200","message":"unknown type `Strng`; did you mean `String`?","span":{"start":8,"end":13},"fix":{"message":"replace with `String`","edits":[{"span":{"start":8,"end":13},"replacement":"String"}]}}
  ],
  "functions": [
    {"name":"count_lines","authority":["FsRead"],"via":[{"param":"fs","capabilities":["FsRead"]}],"pure":false},
    {"name":"Logger.log","authority":["Stdout"],"via":[{"param":"self","capabilities":["Stdout"]}],"pure":false},
    {"name":"add","authority":[],"via":[],"pure":true}
  ]
}
```

- **`diagnostics`**, in source order. Each uses the same object as parser diagnostics (see [`ast.md`](ast.md)): `code`, `message`, `span` (byte offsets), and `fix` when the repair is unambiguous. A parse error is reported alone, with an empty `functions`.
- **`functions`**: one entry per function and `impl` method, in source order. The report is produced even when there are diagnostics.
  - `name`: the function's name, or `Type.method` for a method.
  - `authority`: the built-in capabilities the function can reach, in order of first appearance, without repeats. `cap` types are expanded through their fields, and type arguments are expanded, so `List<Stdout>` reaches `Stdout`. Attenuated capabilities keep their names, such as `FsRead`.
  - `via`: each capability-bearing parameter and the capabilities it supplies, in parameter order. A method's receiver appears as `self`. A `cap fn` parameter is listed with no capabilities, because a closure's capabilities are not written in its type; it still makes the function impure.
  - `pure`: `true` when no parameter, the receiver included, is capability-bearing.

## The prelude

The prelude is Oska source, `crates/oska-check/src/prelude.osk`, parsed with `parse_prelude`. It declares the built-in capabilities as `#[capability]` structs with private fields, the core types (`Option`, `Result`, `List`, `Range`, `Path`, `Args`, `IoError`) with the few methods the examples and tests use, structured concurrency (`Scope`, whose `spawn(task: cap fn() -> T)` returns a `Task<T>`, whose `join()` returns `T`; `scope |s| { ... }` binds `s: Scope`), the `dbg` function, and the core traits (`Eq`, `Hash`, `Display`, `From`). The primitive types (`Int`, `I8` to `I64`, `U8` to `U64`, `Bool`, `String`) and `()` are built into the checker. The prelude implements `Display` for `String`, `Int`, and `IoError`. User code cannot redeclare any of these names.

## Diagnostic codes

| Code | Meaning | Fix |
| --- | --- | --- |
| `E0200` | Unknown type or trait name. A qualified path such as `sql.Query` is unknown until `std.*` modules exist. | Replace the name with the single closest declared name, when one is close enough. |
| `E0201` | A trait where a type is expected (`x: Display`), or a type where a trait is expected (`T: Int`). | None. |
| `E0202` | A type or trait name declared twice in the module, or one that redeclares a prelude name. | None. |
| `E0210` | A struct or enum whose own fields name a capability-bearing type (a built-in capability, a `cap` type, or a `cap fn`, including inside written type arguments such as `List<Stdout>`) is not declared `cap`. A field typed by a type parameter, as in `struct Box<T> { value: T }`, does not count. Spans the type's name. | Insert `cap` before `struct` or `enum`. |
| `E0211` | A `main` parameter whose type is not a built-in capability or `Args`. Spans the parameter's type. | None. |
| `E0212` | `main` returns something other than `()` or `Result<(), E>` (spans the return type), or `E` has no `impl Display` (spans `E`). | None. |
| `E0213` | A type, enum variant, trait, or type parameter name that does not start with an uppercase letter (ADR 0005). | None. |
| `E0214` | A function, method, field, or parameter name that starts with an uppercase letter (ADR 0005). A leading `_` is allowed. | None. |
| `E0215` | A struct or enum with no base case: every way to build it contains itself, as in `struct Loop { me: Loop }`. Recursion through `Option`, `List`, or an enum variant without the type needs no indirection. Each type in a cycle is reported. | None. |
| `E0216` | A call to `dbg(...)` when checking with `--release`. Spans the call. | None. |
| `E0220` | Type mismatch: an expression's type differs from the type expected there. There are no implicit conversions. | None. |
| `E0221` | Unknown value name, or `self` outside a method with a receiver. | Replace the name with the single closest local, function, or prelude variant, when one is close enough. |
| `E0222` | Integer literal out of range for its type, such as `300` as a `U8`. | None. |
| `E0223` | Operator not defined for its operand's type: arithmetic and ordering need numbers (`+` is not for strings), unary `-` needs a signed number, `..` needs integers. | None. |
| `E0224` | Wrong number of arguments in a call or construction. | None. |
| `E0225` | Argument label that does not match its parameter or field, or a label on a positional field. | Replace the label with the parameter's name, or remove it. |
| `E0226` | Named field constructed without its label. | Insert the field's label. |
| `E0227` | Unknown field, or a method used without calling it. | Replace the field with the single closest field, when one is close enough. |
| `E0228` | Unknown method, variant, or associated function, or an associated function called on a value. | Replace the name with the single closest member, when one is close enough. |
| `E0229` | Assignment to something that is not a `var` binding: a `let` binding, a parameter, or a field of `self` in a method without `var self`. | Change `let` to `var`, or `self` to `var self`. None for parameters. |
| `E0230` | Mutating method (`var self`) called on something that is not a `var` binding. | Change `let` to `var`, or `self` to `var self`. |
| `E0231` | Call of a value that is not a function. | None. |
| `E0232` | `break` or `continue` outside a loop. | None. |
| `E0233` | A type that inference cannot work out, such as `let xs = List.new()` with no later use. | None. |
| `E0234` | A type name used as a value, or a type that cannot be constructed. | None. |
| `E0235` | `for` over a value that is not a `List` or a range. | None. |
| `E0270` | A `cap fn` used where a plain `fn` is expected: a closure that captures a capability-bearing value (a built-in capability, a `cap` type, a `cap fn`, or a container of one, including a capability-bearing `self`), or a `cap fn` value passed on. | None. |
| `E0271` | Construction of a prelude struct with private fields, such as a built-in capability (`Stdout(handle: 1)`) or `Args`. Capabilities cannot be forged; they come from `main`. | None. |
| `E0272` | Reading a private field of a prelude type, such as `out.handle`. | None. |
