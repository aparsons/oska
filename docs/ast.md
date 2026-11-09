# Oska AST export

`oskac parse <file.osk> --format=json` writes the syntax tree as JSON. Agents and tools can build on this format.

## Envelope

```json
{
  "ast_version": 1,
  "file": "examples/hello.osk",
  "items": [ ... ]
}
```

## Stability

- Within a major `ast_version`, changes are **additive only**. New node kinds and new fields can appear. Existing fields are never renamed, removed, or given a new meaning.
- Consumers must ignore fields and node kinds they don't recognize.
- Breaking changes increase `ast_version`. The previous version stays available through `--ast-version=N` for at least one release.

## Nodes

Every node is a JSON object with:

- `"kind"`: the node type, as a string, for example `"FnItem"` or `"CallExpr"`.
- `"span"`: `{"start": <byte>, "end": <byte>}`, a half-open byte range into the source file. These are the same spans that diagnostics use.

## Node schema

Conventions:

- Every node's first two fields are `kind` and `span`. The tables below list the other fields.
- Field names are `snake_case`. `kind` values are `PascalCase`, and end in `Item`, `Stmt`, `Expr`, or `Type` for those node families.
- An optional field is omitted when absent. It is never written as `null`.
- An array field is always present, even when empty.
- "Expression node", "statement node", and "type node" mean any node of that family.

### Items

Every item node starts with the same two fields, before its own fields:

| Field | Type | Notes |
| --- | --- | --- |
| `public` | bool | declared `pub` |
| `attributes` | array of `Attribute` | in source order; empty when there are none |

An item's span starts at its first attribute, if it has one, and otherwise at `pub` or its keyword. The tables below list only each item's own fields.

#### `FnItem`

A function declaration: `fn name(params) -> T { body }`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `Ident` |  |
| `params` | array of `Param` | in source order |
| `return_type` | type node | optional; absent means `()` |
| `body` | `Block` |  |

#### `UseItem`

An import: `use a.b` or `use a.b.{X, y}`. Glob imports (`use a.*`, error `E0130`) and renaming imports (`use a.b as c`, error `E0131`) are rejected, so every imported name is written out.

| Field | Type | Notes |
| --- | --- | --- |
| `path` | array of `Ident` | the dotted path, without the brace group |
| `names` | array of `Ident` | optional; the names in the brace group, absent without one |

#### `ConstItem`

A module-level constant: `const NAME: T = expr`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `Ident` |  |
| `type` | type node |  |
| `value` | expression node |  |

#### `TestItem`

A test: `test "a descriptive name" { body }`. Tests that need real capabilities carry `#[integration]`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `StrExpr` | the name as written |
| `body` | `Block` |  |

### Shared nodes

#### `Ident`

A name as written in source, such as a function, parameter, or field name.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string |  |

#### `Param`

A function parameter, `name: Type`. Capability parameters such as `fs: FsRead` are ordinary parameters, so a function's authority can be read from its `params`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `Ident` |  |
| `type` | type node |  |

#### `Block`

A `{ ... }` block. Its value is the value of its last statement if that statement is an `ExprStmt`.

| Field | Type | Notes |
| --- | --- | --- |
| `stmts` | array of statement nodes |  |

#### `Arg`

A call argument, optionally labelled: `copy(src: a)`. The span covers the label and the value.

| Field | Type | Notes |
| --- | --- | --- |
| `label` | `Ident` | optional |
| `value` | expression node |  |

#### `Attribute`

An attribute on an item: `#[name]` or `#[name(args)]`. Names come from a fixed set, currently only `integration`; any other name is error `E0132`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `Ident` |  |
| `args` | array of `Arg` | empty when there are no parentheses |

#### `ClosureParam`

A closure parameter with an optional type: `|x: Int, y|`.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `Ident` |  |
| `type` | type node | optional |

#### `StrText`

Literal text inside a string. The span covers the raw source text, escapes included.

| Field | Type | Notes |
| --- | --- | --- |
| `value` | string | escapes resolved |

#### `StrHole`

An interpolation hole such as `{user.name}`. The span includes the braces.

| Field | Type | Notes |
| --- | --- | --- |
| `path` | array of `Ident` | the name, then any field names |

### Statements

#### `LetStmt`

`let name: T = value`, or `var` for a mutable binding. The initializer is required.

| Field | Type | Notes |
| --- | --- | --- |
| `mutable` | bool | `true` for `var` |
| `name` | `Ident` |  |
| `type` | type node | optional |
| `value` | expression node |  |

#### `AssignStmt`

`target = value`.

| Field | Type | Notes |
| --- | --- | --- |
| `target` | expression node | a `NameExpr`, `SelfExpr`, or `FieldExpr` |
| `value` | expression node |  |

#### `ReturnStmt`

`return`, with an optional value.

| Field | Type | Notes |
| --- | --- | --- |
| `value` | expression node | optional |

#### `BreakStmt`

`break`.

No fields beyond `kind` and `span`.

#### `ContinueStmt`

`continue`.

No fields beyond `kind` and `span`.

#### `ForStmt`

`for binding in iter { body }`.

| Field | Type | Notes |
| --- | --- | --- |
| `binding` | `Ident` |  |
| `iter` | expression node |  |
| `body` | `Block` |  |

#### `WhileStmt`

`while cond { body }`.

| Field | Type | Notes |
| --- | --- | --- |
| `cond` | expression node |  |
| `body` | `Block` |  |

#### `ExprStmt`

An expression used as a statement.

| Field | Type | Notes |
| --- | --- | --- |
| `expr` | expression node |  |

### Expressions

#### `IntExpr`

An integer literal.

| Field | Type | Notes |
| --- | --- | --- |
| `value` | string | the digits as written, including `_` separators |

#### `StrExpr`

A string literal, split into text and interpolation holes.

| Field | Type | Notes |
| --- | --- | --- |
| `parts` | array of `StrText` and `StrHole` | in source order; an empty string has one empty `StrText` |

#### `SinkStrExpr`

A sink literal such as `sql"..."`. It never contains holes.

| Field | Type | Notes |
| --- | --- | --- |
| `tag` | string | such as `sql` |
| `value` | string | escapes resolved |

#### `BoolExpr`

`true` or `false`.

| Field | Type | Notes |
| --- | --- | --- |
| `value` | bool |  |

#### `UnitExpr`

The unit value `()`.

No fields beyond `kind` and `span`.

#### `NameExpr`

A bare name. Its span is the name's span, so the name is a plain string.

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string |  |

#### `SelfExpr`

The receiver `self`.

No fields beyond `kind` and `span`.

#### `FieldExpr`

`base.name`: a field, method, module member, or enum variant. The parser does not resolve which.

| Field | Type | Notes |
| --- | --- | --- |
| `base` | expression node |  |
| `name` | `Ident` |  |

#### `CallExpr`

`callee(args)`. A method call is a call whose callee is a `FieldExpr`.

| Field | Type | Notes |
| --- | --- | --- |
| `callee` | expression node |  |
| `args` | array of `Arg` |  |

#### `UnaryExpr`

A prefix operator.

| Field | Type | Notes |
| --- | --- | --- |
| `op` | string | `-` or `!` |
| `operand` | expression node |  |

#### `BinaryExpr`

A binary operator.

| Field | Type | Notes |
| --- | --- | --- |
| `op` | string | as written: `implies`, `\|\|`, `&&`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `/`, `%` |
| `lhs` | expression node |  |
| `rhs` | expression node |  |

#### `TryExpr`

Postfix `?`.

| Field | Type | Notes |
| --- | --- | --- |
| `operand` | expression node |  |

#### `RangeExpr`

`start..end`, which excludes `end`.

| Field | Type | Notes |
| --- | --- | --- |
| `start` | expression node |  |
| `end` | expression node |  |

#### `ListExpr`

`[a, b, c]`.

| Field | Type | Notes |
| --- | --- | --- |
| `elements` | array of expression nodes |  |

#### `ClosureExpr`

`|params| body`.

| Field | Type | Notes |
| --- | --- | --- |
| `params` | array of `ClosureParam` |  |
| `body` | expression node |  |

#### `IfExpr`

`if cond { ... } else ...`.

| Field | Type | Notes |
| --- | --- | --- |
| `cond` | expression node |  |
| `then_block` | `Block` |  |
| `else_branch` | expression node | optional; an `IfExpr` or a `BlockExpr` |

#### `BlockExpr`

A block used as an expression. Its span is the block's span, so the statements sit directly on the node.

| Field | Type | Notes |
| --- | --- | --- |
| `stmts` | array of statement nodes |  |

#### `ScopeExpr`

`scope |handle| { body }`.

| Field | Type | Notes |
| --- | --- | --- |
| `handle` | `Ident` |  |
| `body` | `Block` |  |

### Types

#### `PathType`

A named type, possibly qualified and generic: `std.io.Error`, `Result<T, E>`.

| Field | Type | Notes |
| --- | --- | --- |
| `segments` | array of `Ident` | the dotted path |
| `args` | array of type nodes | generic arguments; empty when there are none |

#### `UnitType`

`()`.

No fields beyond `kind` and `span`.

#### `FnType`

`fn(T) -> U`, or `cap fn(T) -> U` when the function may capture capabilities.

| Field | Type | Notes |
| --- | --- | --- |
| `cap` | bool |  |
| `params` | array of type nodes |  |
| `return_type` | type node | optional; absent means `()` |
