# Oska

Oska is a secure-by-default programming language designed for AI agents to write and humans to review. These terms describe its security model.

## Authority

**Capability**:
An unforgeable value that grants authority to perform one kind of effect, such as reading files, opening network connections, reading the clock, or generating random numbers. Holding a capability is the only way to perform its effect.
_Avoid_: permission, effect handler, token

**Ambient authority**:
Authority available to code without being explicitly handed to it, such as global state or unrestricted system calls. Oska has none.

**Root of authority**:
The program's entry point, `main`. The runtime grants `main` exactly the capabilities its signature declares, and every other capability in the program comes from those.
_Avoid_: root token, `Root`

**Attenuation**:
Producing a narrower capability from one you already hold, for example restricting `Fs` to a single directory. Attenuation never widens authority.
_Avoid_: derive, narrow, restrict (as nouns for the concept)

**Capability-bearing type**:
A type that is a capability or that contains one, directly or transitively. This includes containment through type arguments, as in `List<Stdout>`, and closures that capture a capability.

**Authority**:
For a function, the set of capabilities it can reach through its capability-bearing parameters. For a package, the union of the authority of its public functions.

**Pure function**:
A function with no capability-bearing parameters. It cannot perform effects, and it is deterministic.

**Program input**:
Data given to `main` that grants no authority, such as `Args`.
_Avoid_: calling it a capability

**Mutating method**:
A method that declares `var self`. It can change its receiver in place, and can only be called on a `var` binding.
_Avoid_: inout, mut ref

**Prelude**:
The built-in declarations, written in Oska, that every program can use without importing them, such as `Int`, `Option`, `Result`, and the built-in capabilities.
_Avoid_: stdlib (for this subset), builtins

## Untrusted data

**Untrusted value**:
Text or bytes that came from outside the program, through a capability or a program input. Text operations keep a value untrusted. Converting it to a non-text or validated type makes it trusted.
_Avoid_: tainted, dirty, unsafe input

**Secret value**:
A value whose contents are never displayed, logged, or interpolated. The only way to read it is an explicit, auditable exposure.
_Avoid_: sensitive, private

**Sink**:
An API where data leaves the program or affects how it behaves, such as SQL queries, process execution, HTML output, filesystem paths, network hosts, or logs.

**Sink literal**:
A string literal tagged with the name of a sink, such as `sql"..."`. It provides the text for a structural position and can never interpolate values.
_Avoid_: tagged string, raw query

**Data position**:
An argument to a sink that is treated only as data. It accepts untrusted values. Examples are SQL parameters, escaped HTML text, and process arguments.

**Structural position**:
An argument to a sink that decides code, commands, paths, or destinations. It rejects untrusted values. Examples are SQL query text, a program name, `TrustedHtml`, an attenuation directory, and a network host.
