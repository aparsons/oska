/-!
# Oska Core: syntax

The syntax of Oska Core, version 3 (ADR 0018): exactly the fragment that
core IR version 3 exports (`crates/oska-check/src/core_ir.rs`). It has
functions, `let`, `if`, `Int` and `Bool` with their operators, `()`,
structs and enums without generics, field access, and `match`; and
capabilities: text, `Path`, the capabilities `Stdout`, `Fs`, `FsRead`,
`FsWrite`, `Dir`, `DirRead` and `DirWrite` as opaque values only `main`
receives, the built-in calls that write a line to stdout, read and write
a file and create a directory under a directory capability, and
attenuate one capability to a narrower one, and the `Result` of an
effect with `is_ok` and `unwrap_or`. Each Lean constructor is one core IR
node, with the same fields; the values a run makes (a capability, a
path, a result) have no core IR node, so a program as written holds
none.

- A local is a *slot*: the function's locals are numbered from 0 in the
  order they are bound, the parameters that bind first, then each `let`
  that binds, and each binding of a `match` arm's pattern, in source
  order. A parameter or `let` with `binds = false` takes no slot.
- `Int` is a signed 64-bit integer: an `Int` value lies in
  `intMin ..= intMax`, and arithmetic outside that range panics.
- A struct's fields and an enum variant's fields are positional: core IR
  numbers them in declaration order.
-/

namespace OskaFormal.Core

/-- A built-in capability kind of Oska Core: core IR's `"Stdout"`, `"Fs"`,
`"FsRead"`, `"FsWrite"`, `"Dir"`, `"DirRead"` and `"DirWrite"`. -/
inductive CapKind where
  | stdout
  | fs | fsRead | fsWrite
  | dir | dirRead | dirWrite
  deriving DecidableEq, Repr

/-- The kinds a capability of kind `k` can be attenuated to, `k` first:
ADR 0008's order, `Fs` > `FsRead` > `DirRead` and likewise for writes,
with `Fs` > `Dir` > `DirRead`, `DirWrite`. What a capability of kind `k`
reaches is what one of these reaches. -/
def CapKind.below : CapKind → List CapKind
  | .stdout => [.stdout]
  | .fs => [.fs, .fsRead, .fsWrite, .dir, .dirRead, .dirWrite]
  | .fsRead => [.fsRead, .dirRead]
  | .fsWrite => [.fsWrite, .dirWrite]
  | .dir => [.dir, .dirRead, .dirWrite]
  | .dirRead => [.dirRead]
  | .dirWrite => [.dirWrite]

/-- The capability kinds `main` may declare, which the runtime grants:
`Stdout`, `Fs`, `FsRead` and `FsWrite`. A directory capability comes only
from attenuation. -/
def CapKind.granted : CapKind → Bool
  | .stdout | .fs | .fsRead | .fsWrite => true
  | _ => false

/-- The types of Oska Core: core IR's `"Int"`, `"Bool"`, `"()"`,
`"String"` (text, trusted or not: trust is checked statically and has no
runtime effect), `"Path"`, a capability kind, `"Result<(), IoError>"` and
`"Result<String, IoError>"` (`result unit` and `result string`), and a
struct or enum the program declares, by name. -/
inductive Ty where
  | int
  | bool
  | unit
  | string
  | path
  | cap (kind : CapKind)
  | result (ok : Ty)
  | named (name : String)
  deriving DecidableEq, Repr

/-- The types `==` and `!=` compare: `Int`, `Bool` and `()`. The checker
compares a struct or enum only through an `Eq` implementation, which is
outside Oska Core, and the export refuses `==` on text. -/
def Ty.isBase : Ty → Bool
  | .int | .bool | .unit => true
  | _ => false

/-- The capability kind of a capability type. -/
def Ty.capKind? : Ty → Option CapKind
  | .cap k => some k
  | _ => none

/-- A built-in call of Oska Core: core IR's `Builtin` names, the checker's
qualified names of the prelude's body-less functions. A method's receiver
is its first argument.

- `println`: `Stdout.println(line)`.
- `restrictTo k`: `K.restrict_to(dir)` on the capability kind `k`, `Fs`,
  `FsRead`, `FsWrite` (with a `String`), `Dir`, `DirRead` or `DirWrite`
  (with a `Path`): attenuation to a directory capability.
- `readOnly k`, `writeOnly k`: `K.read_only()`, `K.write_only()` on `Fs`
  and `Dir`.
- `readToString`: `DirRead.read_to_string(path)`.
- `write`: `DirWrite.write(path, contents)`.
- `createDir`: `DirWrite.create_dir(path)`.
- `pathFrom`, `pathNew`: `Path.from(text)`, `Path.new(text)`.
- `isOk`, `unwrapOr`: `Result.is_ok()`, `Result.unwrap_or(default)`. -/
inductive Builtin where
  | println
  | restrictTo (k : CapKind)
  | readOnly (k : CapKind)
  | writeOnly (k : CapKind)
  | readToString
  | write
  | createDir
  | pathFrom
  | pathNew
  | isOk
  | unwrapOr
  deriving DecidableEq, Repr

/-- What `restrict_to` on a capability of kind `k` takes, and the kind it
gives. -/
def CapKind.restrictTo : CapKind → Option (Ty × CapKind)
  | .fs => some (.string, .dir)
  | .fsRead => some (.string, .dirRead)
  | .fsWrite => some (.string, .dirWrite)
  | .dir => some (.path, .dir)
  | .dirRead => some (.path, .dirRead)
  | .dirWrite => some (.path, .dirWrite)
  | .stdout => none

/-- The kind `read_only()` gives on a capability of kind `k`. -/
def CapKind.readOnly : CapKind → Option CapKind
  | .fs => some .fsRead
  | .dir => some .dirRead
  | _ => none

/-- The kind `write_only()` gives on a capability of kind `k`. -/
def CapKind.writeOnly : CapKind → Option CapKind
  | .fs => some .fsWrite
  | .dir => some .dirWrite
  | _ => none

/-- The kind of capability an **effect** takes as its receiver: the
built-in calls that ask the host for something. `read_only`,
`write_only`, `Path` and `Result` are pure. -/
def Builtin.receiver : Builtin → Option CapKind
  | .println => some .stdout
  | .restrictTo k => (k.restrictTo).map fun _ => k
  | .readToString => some .dirRead
  | .write => some .dirWrite
  | .createDir => some .dirWrite
  | _ => none

/-- Where a filesystem capability reaches, as the interpreter's `Root`
(`crates/oska-run/src/value.rs`): anywhere (the `Fs`, `FsRead` or
`FsWrite` `main` receives; also `Stdout`'s, which has no root), beneath
the directory at `path` (components from `/`), or nowhere (a root that
could not be opened). -/
inductive CapRoot where
  | anywhere
  | beneath (path : List String)
  | nowhere
  deriving DecidableEq, Repr

/-- The paths a root reaches: every path, the paths beneath it (itself
included), or none. -/
def CapRoot.Reaches : CapRoot → List String → Prop
  | .anywhere, _ => True
  | .beneath p, q => p <+: q
  | .nowhere, _ => False

/-- `r'` reaches no path that `r` does not: it is no wider. -/
def CapRoot.Within (r' r : CapRoot) : Prop :=
  ∀ q, r'.Reaches q → r.Reaches q

/-- A binary operator, as core IR writes it in `op`. -/
inductive BinOp where
  | add | sub | mul | div | rem
  | lt | le | gt | ge
  | eq | ne
  | and | or | implies
  deriving DecidableEq, Repr

/-- A pattern of a `match` arm: core IR's `pattern`. A literal pattern is
an `Int`, a `Bool` or `()`; a constructor pattern names a struct, or an
enum and its variant, and has a pattern for each field. -/
inductive Pattern where
  | wildcard
  | bind (type : Ty)
  | unit
  | int (value : Int)
  | bool (value : Bool)
  | construct (type : String) (variant : Option String) (fields : List Pattern)

mutual
  /-- An expression of Oska Core: one constructor per core IR `expr`
  kind. An `if` with no `else`, and a block with no value, hold `none`.
  A `construct` with no variant builds a struct; with one, an enum
  variant. -/
  inductive Expr where
    | unit
    | str (value : String)
    | path (text : String)
    | cap (kind : CapKind) (root : CapRoot)
    | okUnit
    | okStr (value : String)
    | err
    | int (value : Int)
    | bool (value : Bool)
    | local_ (slot : Nat)
    | neg (operand : Expr)
    | not (operand : Expr)
    | binary (op : BinOp) (lhs rhs : Expr)
    | call (function : String) (args : List Expr)
    | if_ (cond thenBranch : Expr) (elseBranch : Option Expr)
    | block (stmts : List Stmt) (value : Option Expr)
    | construct (type : String) (variant : Option String) (fields : List Expr)
    | field (base : Expr) (index : Nat)
    | match_ (scrutinee : Expr) (arms : List Arm)
    | builtin (op : Builtin) (args : List Expr)

  /-- A statement of Oska Core: core IR's `Let` and `Expr`. -/
  inductive Stmt where
    | let_ (binds : Bool) (type : Ty) (value : Expr)
    | expr (value : Expr)

  /-- A `match` arm: its pattern, its guard if it has one, and its body. -/
  inductive Arm where
    | mk (pattern : Pattern) (guard : Option Expr) (body : Expr)
end

/-- A parameter: its type, and whether it binds a slot. -/
structure Param where
  type : Ty
  binds : Bool

/-- A function of Oska Core, as core IR's `function`. -/
structure Function where
  name : String
  params : List Param
  ret : Ty
  body : Expr

/-- A struct or enum declaration, as core IR's `typedecl`: a struct's
field types, or an enum's variants with their field types, in
declaration order. -/
inductive TypeDecl where
  | struct_ (name : String) (fields : List Ty)
  | enum_ (name : String) (variants : List (String × List Ty))

def TypeDecl.name : TypeDecl → String
  | .struct_ n _ => n
  | .enum_ n _ => n

/-- A program of Oska Core: its struct and enum declarations and its
functions, each in source order. -/
structure Program where
  types : List TypeDecl
  functions : List Function

/-- The first function of `name` in a list. -/
def findFunction : List Function → String → Option Function
  | [], _ => none
  | f :: fs, name => if f.name = name then some f else findFunction fs name

/-- The function a `Call` of `name` reaches: the first of that name. The
checker refuses two functions of one name, so in a checked program it is
the only one. -/
def Program.find? (P : Program) (name : String) : Option Function :=
  findFunction P.functions name

/-- The first declaration of `name` in a list. -/
def findType : List TypeDecl → String → Option TypeDecl
  | [], _ => none
  | d :: ds, name => if d.name = name then some d else findType ds name

/-- The field types of what a constructor of `type` builds: the struct
`type` when `variant` is `none`, and its variant `variant` when `type` is
an enum. `none` when the program declares no such struct or variant. A
name declares one type only, so a struct is never also an enum. -/
def Program.fieldsOf (P : Program) (type : String) (variant : Option String) :
    Option (List Ty) :=
  match findType P.types type, variant with
  | some (.struct_ _ fs), none => some fs
  | some (.enum_ _ vs), some v => vs.lookup v
  | _, _ => none

/-- The smallest `Int`, `-2^63`. -/
def intMin : Int := -(2 ^ 63)

/-- The largest `Int`, `2^63 - 1`. -/
def intMax : Int := 2 ^ 63 - 1

/-- Whether `n` is an `Int` of Oska: whether it fits in 64 signed bits. -/
def fits (n : Int) : Bool := decide (intMin ≤ n) && decide (n ≤ intMax)

/-- The values of Oska Core: `()`, an integer, a boolean, text, a path, a
capability, the `Ok` or `Err` of an effect's `Result`, and a struct or
enum variant whose fields are values. An `Err` carries no message: no
program of Oska Core can read one. -/
inductive IsValue : Expr → Prop where
  | unit : IsValue .unit
  | int (n : Int) : IsValue (.int n)
  | bool (b : Bool) : IsValue (.bool b)
  | str (s : String) : IsValue (.str s)
  | path (s : String) : IsValue (.path s)
  | cap (k : CapKind) (r : CapRoot) : IsValue (.cap k r)
  | okUnit : IsValue .okUnit
  | okStr (s : String) : IsValue (.okStr s)
  | err : IsValue .err
  | construct {type : String} {variant : Option String} {fields : List Expr} :
      (∀ e ∈ fields, IsValue e) → IsValue (.construct type variant fields)

/-- Every expression of a list is a value. -/
inductive AllValues : List Expr → Prop where
  | nil : AllValues []
  | cons {v : Expr} {vs : List Expr} : IsValue v → AllValues vs → AllValues (v :: vs)

theorem AllValues.of_forall : ∀ {vs : List Expr}, (∀ e ∈ vs, IsValue e) → AllValues vs
  | [], _ => .nil
  | _ :: _, h =>
    .cons (h _ List.mem_cons_self) (AllValues.of_forall fun e he => h e (List.mem_cons_of_mem _ he))

theorem AllValues.forall {vs : List Expr} (h : AllValues vs) : ∀ e ∈ vs, IsValue e := by
  induction h with
  | nil => intro e he; cases he
  | cons hv _ ih =>
    intro e he
    cases he with
    | head => exact hv
    | tail _ he => exact ih e he

mutual
  /-- What the value `v` binds when it matches the pattern `p`, in source
  order, or `none` when it does not match, as the interpreter matches
  (`crates/oska-run/src/interp.rs`): a wildcard matches anything, a name
  matches anything and binds it, a literal matches an equal value, and a
  constructor pattern matches a value of its variant whose fields match
  its patterns, from the first. -/
  def matchPat : Pattern → Expr → Option (List Expr)
    | .wildcard, _ => some []
    | .bind _, v => some [v]
    | .unit, .unit => some []
    | .int n, .int m => if n = m then some [] else none
    | .bool b, .bool c => if b = c then some [] else none
    | .construct _ variant ps, .construct _ variant' vs =>
      if variant = variant' then matchPats ps vs else none
    | _, _ => none

  /-- `matchPat` on each field: the bindings of all, in order. -/
  def matchPats : List Pattern → List Expr → Option (List Expr)
    | [], [] => some []
    | p :: ps, v :: vs =>
      match matchPat p v, matchPats ps vs with
      | some bs, some bss => some (bs ++ bss)
      | _, _ => none
    | _, _ => none
end

end OskaFormal.Core
