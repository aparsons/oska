# Packages and modules

A program that outgrows one file becomes a **package**: a directory with an `oska.toml` and its modules under `src/`. One file is one module. `oskac check`, `run` and `test` work on the whole package, and every module is checked as one program.

This page describes milestone 1 of packages. There are no dependencies yet; see [what is not supported yet](#not-supported-yet).

## Starting a package

A package is this layout, which is also [`examples/shopping`](https://github.com/aparsons/oska/tree/main/examples/shopping):

```text
shopping/
  oska.toml
  src/
    main.osk
    receipt.osk
    pricing/
      tax.osk
```

`oska.toml` names the package. It has one table, `[package]`, with a `name` (snake_case) and a `version` (`MAJOR.MINOR.PATCH`):

```text
[package]
name = "shopping"
version = "0.1.0"
```

Any other key or table is an error (`E0402`), and a `[dependencies]` table is `E0403`. The manifest is a strict subset of TOML: tables, single-line strings, arrays of strings and comments.

`src/main.osk` is the entry module, and the only module that may declare `main` (`E0412`):

```oska fragment
use pkg.receipt
use pkg.pricing.tax

fn main(io: Stdout) -> Result<(), IoError> {
    let total = tax.with_tax(2000)
    io.println(receipt.line("Total", total))
}
```

Run the commands in the package directory, or give the directory as the path:

```sh
oskac check
oskac run
oskac test
```

On `examples/shopping`, `oskac run` prints `Total: 2400 cents`, and `oskac test` runs the tests of every module, each reported as `module::name`. A directory with no `oska.toml` or no `src/` is `E0413`. File and directory names under `src/` must be snake_case (`E0410`), and `oskac` follows no symlinks there (`E0411`).

## Modules and `use pkg.…`

Every `.osk` file under `src/` is a module. `use pkg.a.b` imports the file `src/a/b.osk`. The first segment of a `use` says where a name comes from: `pkg` is this package, and `std` is the standard library. A path that starts with anything else is `E0294`.

A plain `use` binds the module's last segment, and you reach its items through that qualifier, such as `tax.with_tax`. A brace group imports items by name, with no qualifier:

```oska fragment
use pkg.shapes.{Square, area}

fn describe(s: Square) -> Int {
    area(s)
}
```

There are no glob imports, and no renaming: a `use` that binds a name the module already has, such as two modules both called `util`, is `E0208`. Move one of them, or import by brace group. A `use pkg.…` of a module that does not exist is `E0204`, a name the module does not declare is `E0205`, and modules that import each other in a cycle are `E0207`. In a single file, `use pkg.…` is `E0203`.

## Visibility: private, `pub(pkg)` and `pub`

Everything is private to its module by default. Two markers widen it:

| Marker | Visible to |
| --- | --- |
| none | its own module |
| `pub(pkg)` | every module of the package |
| `pub` | every module, and it is part of the package's API |

```oska fragment
pub(pkg) fn line(label: String, cents: Int) -> String {
    "{label}: {cents} cents"
}

fn helper() -> Int {
    1
}

pub fn version() -> String {
    "1"
}
```

Using a private item from another module is `E0206`, and its machine fix is to insert `pub(pkg) ` at the declaration, in the other file. The same holds below the item: a struct field needs its own `pub` to be read or built from another module, and an inherent method needs `pub(pkg)` or `pub` to be called. The module of an `impl` decides its methods' visibility.

Use `pub(pkg)` for what the package's modules share, and `pub` for what other code is meant to use. The authority report marks each `pub` function, and names every function with its module path, so two modules can each have a `format`:

```text
fn main.main: Stdout (io)
fn pricing.tax.with_tax: pure
fn receipt.line: pure
```

A diagnostic's location is the file in the package, such as `src/other.osk:4:11: error[E0206]: ...`.

## Libraries and programs

A package with a `src/main.osk` is a program. A package without one is a **library**: `oskac check` and `oskac test` work on it, and `oskac run` refuses it (`E0414`). In a library, mark the modules' entry points `pub`.

## Not supported yet

- **Dependencies.** `oska.toml` has no `[dependencies]`, so a package can import only `pkg` and `std`. They arrive in a later milestone ([ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md)).
- **Re-exports.** `pub use` is `E0164`: a module's items can be imported only from the module that declares them.
- **Renaming.** There is no `use … as …`; see `E0208` above.
- **Glob imports.** `use` names modules and items explicitly.

Next: the [CLI reference](cli-reference.md) for how `check`, `run` and `test` take a package, and the [agent guide](agent-guide.md) for the machine-readable loop.
