# The interpreter runs the checker's checked program

`oska-check` produces a **checked program** as well as its report. A checked program is a lowered tree in which every name, method call, trait dispatch, and `?` conversion has already been resolved. The interpreter (`oska-run`) executes only checked programs. `oskac run` refuses to start if the checker reports any diagnostic.

## Why

The checker decides what every call means, and it is the security boundary for capabilities and trust. An interpreter that walked the syntax tree would have to re-implement name resolution, method lookup, trait dispatch, and `From` conversions. That second copy could drift away from the first, so the program that runs would not be the program that was checked. With one checked program, there is a single source of truth for what a call means.

## Considered options

- **Interpret the syntax tree directly.** Rejected because it duplicates the checker's resolution logic, and the two copies could disagree on what a call means.
- **Monomorphize generics in the checked program.** Rejected for now. The checked program keeps generic information. The interpreter dispatches generic trait calls by looking up the impl from the runtime type tag that every value carries. A compiled backend, such as WASM, can monomorphize from the same checked program later.

## Consequences

- Every backend consumes the checked program. The checked program's shape is internal to the toolchain, unlike the AST JSON export, so it may change freely.
- A body-less prelude function needs a runtime implementation in every backend. A test enforces that each one has one.
