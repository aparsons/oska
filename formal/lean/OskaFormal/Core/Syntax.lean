/-!
# Oska Core: syntax

The syntax of Oska Core, version 6 (ADR 0018): exactly the fragment that
core IR version 6 exports (`crates/oska-check/src/core_ir.rs`). It has
functions, `let`, `if`, `Int` and `Bool` with their operators, `()`,
structs and enums without generics, field access, and `match`; and
capabilities: text, `Path`, the capabilities `Stdout`, `Fs`, `FsRead`,
`FsWrite`, `Dir`, `DirRead` and `DirWrite` as opaque values only `main`
receives, the built-in calls that write a line to stdout, read and write
a file and create a directory under a directory capability, and
attenuate one capability to a narrower one; closures, which capture by
value the slots they read, with function types and calls of a function
value; and `Result`: `Ok` and `Err`, `match` on them, `is_ok` and
`unwrap_or`, and `?`, which returns an `Err` from the function (or
closure) it is in, through the `From` conversion the checker chose, if
any; and `var` bindings, assignment and **mutating methods** (`var
self`), with value semantics (ADR 0001): a change through a `var` is
never seen through another binding. Each Lean constructor is one core IR
node, with the same fields; the values a run makes (a capability, a path,
an `IoError`), the frame a call runs its body in, the cell that holds a
`var` while its block runs and the running mutating call have no core IR
node, so a program as written holds none.

- A local is a *slot*: the function's locals are numbered from 0 in the
  order they are bound, the parameters that bind first, then each `let`
  that binds, and each binding of a `match` arm's pattern, in source
  order. A parameter or `let` with `binds = false` takes no slot.
- A closure's body numbers slots as the enclosing function does where
  the closure is written: the `height` slots then in scope, then the
  closure's parameters from `height` on, then its own locals.
- A `var` takes no slot: it is a *var*, numbered by how many `var`s of
  the same function or closure are bound inside it, so the innermost var
  in scope is var 0. A mutating method's receiver (`var self`) is the
  outermost var of its body; a method that takes `self` has it as its
  first parameter, slot 0. A closure's body sees no var of the function
  it is written in (the export refuses a closure that captures one).
- A *place*, what an assignment writes or a mutating call changes, is a
  var and a path of struct fields in it, outermost first.
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
runtime effect), `"Path"`, `"IoError"` (the error an effect gives, an
opaque value: no program of Oska Core reads its message), a capability
kind, `Result<ok, err>` (core IR's `{"kind": "Result", ...}`), a struct
or enum the program declares, by name, and a function type
`fn(params) -> ret`, or `cap fn(params) -> ret` when `cap`, core IR's
`{"kind": "Fn", ...}`: a closure's type. A plain `fn` captures no
capability-bearing value (`Typing.lean`); a `cap fn` may, and reaches
what it captured (`Authority.lean`). -/
inductive Ty where
  | int
  | bool
  | unit
  | string
  | path
  | ioError
  | cap (kind : CapKind)
  | result (ok err : Ty)
  | named (name : String)
  | fn (cap : Bool) (params : List Ty) (ret : Ty)
  deriving Repr

mutual
  /-- Equality of types is decidable. `deriving` does not reach a type
  nested in a `List`, so it is written out. -/
  def Ty.decEq : (a b : Ty) → Decidable (a = b)
    | .int, .int => isTrue rfl
    | .int, .bool => isFalse nofun
    | .int, .unit => isFalse nofun
    | .int, .string => isFalse nofun
    | .int, .path => isFalse nofun
    | .int, .ioError => isFalse nofun
    | .int, .cap _ => isFalse nofun
    | .int, .result _ _ => isFalse nofun
    | .int, .named _ => isFalse nofun
    | .int, .fn _ _ _ => isFalse nofun
    | .bool, .int => isFalse nofun
    | .bool, .bool => isTrue rfl
    | .bool, .unit => isFalse nofun
    | .bool, .string => isFalse nofun
    | .bool, .path => isFalse nofun
    | .bool, .ioError => isFalse nofun
    | .bool, .cap _ => isFalse nofun
    | .bool, .result _ _ => isFalse nofun
    | .bool, .named _ => isFalse nofun
    | .bool, .fn _ _ _ => isFalse nofun
    | .unit, .int => isFalse nofun
    | .unit, .bool => isFalse nofun
    | .unit, .unit => isTrue rfl
    | .unit, .string => isFalse nofun
    | .unit, .path => isFalse nofun
    | .unit, .ioError => isFalse nofun
    | .unit, .cap _ => isFalse nofun
    | .unit, .result _ _ => isFalse nofun
    | .unit, .named _ => isFalse nofun
    | .unit, .fn _ _ _ => isFalse nofun
    | .string, .int => isFalse nofun
    | .string, .bool => isFalse nofun
    | .string, .unit => isFalse nofun
    | .string, .string => isTrue rfl
    | .string, .path => isFalse nofun
    | .string, .ioError => isFalse nofun
    | .string, .cap _ => isFalse nofun
    | .string, .result _ _ => isFalse nofun
    | .string, .named _ => isFalse nofun
    | .string, .fn _ _ _ => isFalse nofun
    | .path, .int => isFalse nofun
    | .path, .bool => isFalse nofun
    | .path, .unit => isFalse nofun
    | .path, .string => isFalse nofun
    | .path, .path => isTrue rfl
    | .path, .ioError => isFalse nofun
    | .path, .cap _ => isFalse nofun
    | .path, .result _ _ => isFalse nofun
    | .path, .named _ => isFalse nofun
    | .path, .fn _ _ _ => isFalse nofun
    | .ioError, .int => isFalse nofun
    | .ioError, .bool => isFalse nofun
    | .ioError, .unit => isFalse nofun
    | .ioError, .string => isFalse nofun
    | .ioError, .path => isFalse nofun
    | .ioError, .ioError => isTrue rfl
    | .ioError, .cap _ => isFalse nofun
    | .ioError, .result _ _ => isFalse nofun
    | .ioError, .named _ => isFalse nofun
    | .ioError, .fn _ _ _ => isFalse nofun
    | .cap _, .int => isFalse nofun
    | .cap _, .bool => isFalse nofun
    | .cap _, .unit => isFalse nofun
    | .cap _, .string => isFalse nofun
    | .cap _, .path => isFalse nofun
    | .cap _, .ioError => isFalse nofun
    | .cap x, .cap y => if h : x = y then isTrue (h ▸ rfl) else isFalse fun e => by cases e; exact h rfl
    | .cap _, .result _ _ => isFalse nofun
    | .cap _, .named _ => isFalse nofun
    | .cap _, .fn _ _ _ => isFalse nofun
    | .result _ _, .int => isFalse nofun
    | .result _ _, .bool => isFalse nofun
    | .result _ _, .unit => isFalse nofun
    | .result _ _, .string => isFalse nofun
    | .result _ _, .path => isFalse nofun
    | .result _ _, .ioError => isFalse nofun
    | .result _ _, .cap _ => isFalse nofun
    | .result o e, .result o' e' =>
      match Ty.decEq o o', Ty.decEq e e' with
      | isTrue h₁, isTrue h₂ => isTrue (h₁ ▸ h₂ ▸ rfl)
      | isFalse h, _ => isFalse fun x => by cases x; exact h rfl
      | _, isFalse h => isFalse fun x => by cases x; exact h rfl
    | .result _ _, .named _ => isFalse nofun
    | .result _ _, .fn _ _ _ => isFalse nofun
    | .named _, .int => isFalse nofun
    | .named _, .bool => isFalse nofun
    | .named _, .unit => isFalse nofun
    | .named _, .string => isFalse nofun
    | .named _, .path => isFalse nofun
    | .named _, .ioError => isFalse nofun
    | .named _, .cap _ => isFalse nofun
    | .named _, .result _ _ => isFalse nofun
    | .named x, .named y => if h : x = y then isTrue (h ▸ rfl) else isFalse fun e => by cases e; exact h rfl
    | .named _, .fn _ _ _ => isFalse nofun
    | .fn _ _ _, .int => isFalse nofun
    | .fn _ _ _, .bool => isFalse nofun
    | .fn _ _ _, .unit => isFalse nofun
    | .fn _ _ _, .string => isFalse nofun
    | .fn _ _ _, .path => isFalse nofun
    | .fn _ _ _, .ioError => isFalse nofun
    | .fn _ _ _, .cap _ => isFalse nofun
    | .fn _ _ _, .result _ _ => isFalse nofun
    | .fn _ _ _, .named _ => isFalse nofun
    | .fn c ps r, .fn d qs u =>
      if hc : c = d then
        match Ty.decEqList ps qs, Ty.decEq r u with
        | isTrue h₁, isTrue h₂ => isTrue (hc ▸ h₁ ▸ h₂ ▸ rfl)
        | isFalse h, _ => isFalse fun e => by cases e; exact h rfl
        | _, isFalse h => isFalse fun e => by cases e; exact h rfl
      else isFalse fun e => by cases e; exact hc rfl


  def Ty.decEqList : (a b : List Ty) → Decidable (a = b)
    | [], [] => isTrue rfl
    | [], _ :: _ => isFalse nofun
    | _ :: _, [] => isFalse nofun
    | a :: as, b :: bs =>
      match Ty.decEq a b, Ty.decEqList as bs with
      | isTrue h₁, isTrue h₂ => isTrue (h₁ ▸ h₂ ▸ rfl)
      | isFalse h, _ => isFalse fun e => by cases e; exact h rfl
      | _, isFalse h => isFalse fun e => by cases e; exact h rfl
end

instance : DecidableEq Ty := Ty.decEq

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

/-- Whether a type is a function type. -/
def Ty.isFn : Ty → Bool
  | .fn _ _ _ => true
  | _ => false

/-- Whether a type is a `cap fn` type: a function value that may carry
authority. -/
def Ty.isCapFn : Ty → Bool
  | .fn c _ _ => c
  | _ => false

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
- `isOk`, `unwrapOr`: `Result.is_ok()`, `Result.unwrap_or(default)`, on
  any `Result`. -/
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
enum and its variant, and has a pattern for each field. `result true ok
err p` is core IR's `Ok` pattern on a `Result<ok, err>`, and `result
false ok err p` its `Err` pattern: `p` matches the value inside. -/
inductive Pattern where
  | wildcard
  | bind (type : Ty)
  | unit
  | int (value : Int)
  | bool (value : Bool)
  | construct (type : String) (variant : Option String) (fields : List Pattern)
  | result (isOk : Bool) (ok err : Ty) (value : Pattern)

/-- A parameter of a function or closure: its type, and whether it binds
a slot. -/
structure Param where
  type : Ty
  binds : Bool

/-- A place: the var `var` and the path of struct fields `fields` in it,
outermost first. Core IR's `{"var": N, "fields": [N...]}`. -/
structure Place where
  var : Nat
  fields : List Nat

mutual
  /-- An expression of Oska Core: one constructor per core IR `expr`
  kind. An `if` with no `else`, and a block with no value, hold `none`.
  A `construct` with no variant builds a struct; with one, an enum
  variant.

  A `closure` is written where `height` slots are in scope; `captures`
  lists those its body reads, `cap` whether its type is a `cap fn`, and
  `ret` its result type. A run puts a
  value for each slot below `height` in its body as the slot is bound
  (`Semantics.lean`), so a closure the run reaches has height 0 and its
  body holds the values it captured. `callValue` calls a function value
  with arguments.

  `result true e` is core IR's `Ok`, and `result false e` its `Err`.
  `try_ e convert` is `e?`: the value inside an `Ok`; on an `Err`, the
  function returns it, its error put through the function `convert`
  first when the checker chose a `From` conversion. A call runs the
  callee's body in a `frame`, which such a return ends; core IR has no
  node for it. -/
  inductive Expr where
    | unit
    | str (value : String)
    | path (text : String)
    | cap (kind : CapKind) (root : CapRoot)
    | ioError
    | result (isOk : Bool) (value : Expr)
    | try_ (operand : Expr) (convert : Option String)
    | frame (body : Expr)
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
    | closure (params : List Param) (height : Nat) (captures : List Nat) (cap : Bool) (ret : Ty)
        (body : Expr)
    | callValue (callee : Expr) (args : List Expr)
    | var_ (index : Nat)
    | mutCall (function : String) (receiver : Place) (args : List Expr)
    | cell (value body : Expr)
    | method (receiver : Place) (value body : Expr)

  /-- A statement of Oska Core: core IR's `Let`, `Expr`, `Var` (a `var`
  binding, which takes no slot but makes var 0 of the rest of its block)
  and `Assign`. -/
  inductive Stmt where
    | let_ (binds : Bool) (type : Ty) (value : Expr)
    | expr (value : Expr)
    | var_ (type : Ty) (value : Expr)
    | assign (place : Place) (value : Expr)

  /-- A `match` arm: its pattern, its guard if it has one, and its body. -/
  inductive Arm where
    | mk (pattern : Pattern) (guard : Option Expr) (body : Expr)
end

/-- A function of Oska Core, as core IR's `function`. A mutating method
has the type of its receiver (`var self`) as `receiver`, the outermost
var of its body; any other function has none. -/
structure Function where
  name : String
  receiver : Option Ty := none
  params : List Param
  ret : Ty
  body : Expr

/-- The types of the arguments a `Call` of `f` passes: the receiver's
first, for a mutating method, then one per parameter. -/
def Function.argTypes (f : Function) : List Ty :=
  f.receiver.toList ++ f.params.map Param.type

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

/-- `Ok(())`, the answer to an effect that succeeded. -/
abbrev Expr.okUnit : Expr := .result true .unit

/-- `Ok(text)`, the answer to a read that succeeded. -/
abbrev Expr.okStr (s : String) : Expr := .result true (.str s)

/-- `Err(e)` for an `IoError` `e`, the answer to an effect that failed. -/
abbrev Expr.ioErr : Expr := .result false .ioError

/-- The values of Oska Core: `()`, an integer, a boolean, text, a path, a
capability, an `IoError`, an `Ok` or `Err` of a value, a struct or enum
variant whose fields are values, and a closure of height 0, whose body
holds the values it captured. An `IoError` carries no message: no
program of Oska Core can read one. -/
inductive IsValue : Expr → Prop where
  | unit : IsValue .unit
  | int (n : Int) : IsValue (.int n)
  | bool (b : Bool) : IsValue (.bool b)
  | str (s : String) : IsValue (.str s)
  | path (s : String) : IsValue (.path s)
  | cap (k : CapKind) (r : CapRoot) : IsValue (.cap k r)
  | ioError : IsValue .ioError
  | result (b : Bool) {v : Expr} : IsValue v → IsValue (.result b v)
  | construct {type : String} {variant : Option String} {fields : List Expr} :
      (∀ e ∈ fields, IsValue e) → IsValue (.construct type variant fields)
  | closure (ps : List Param) (cs : List Nat) (cap : Bool) (ret : Ty) (body : Expr) :
      IsValue (.closure ps 0 cs cap ret body)

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
  its patterns, from the first, and an `Ok` or `Err` pattern an `Ok` or
  `Err` whose value matches its pattern. -/
  def matchPat : Pattern → Expr → Option (List Expr)
    | .wildcard, _ => some []
    | .bind _, v => some [v]
    | .unit, .unit => some []
    | .int n, .int m => if n = m then some [] else none
    | .bool b, .bool c => if b = c then some [] else none
    | .construct _ variant ps, .construct _ variant' vs =>
      if variant = variant' then matchPats ps vs else none
    | .result b _ _ p, .result b' v => if b = b' then matchPat p v else none
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

mutual
  /-- Whether `e` reads slot `i` of the context it is written in. Slots
  count from the first in scope, so a `.local_ i` anywhere in `e` is that
  slot, except inside a closure of height `h ≤ i`, where slot `i` is the
  closure's own (a parameter or a local of its body). What a closure of
  height `h` captures is the slots below `h` its body reads. -/
  def Expr.reads (i : Nat) : Expr → Bool
    | .local_ j => decide (i = j)
    | .unit | .int _ | .bool _ | .str _ | .path _ | .cap _ _ | .ioError => false
    | .neg e | .not e | .field e _ | .result _ e | .try_ e _ | .frame e => Expr.reads i e
    | .binary _ l r => Expr.reads i l || Expr.reads i r
    | .call _ args | .builtin _ args => Expr.readsList i args
    | .construct _ _ fs => Expr.readsList i fs
    | .if_ c th el => Expr.reads i c || Expr.reads i th || Expr.readsOpt i el
    | .block ss val => Expr.readsStmts i ss || Expr.readsOpt i val
    | .match_ sc arms => Expr.reads i sc || Expr.readsArms i arms
    | .closure _ h _ _ _ body => decide (i < h) && Expr.reads i body
    | .callValue c args => Expr.reads i c || Expr.readsList i args
    | .var_ _ => false
    | .mutCall _ _ args => Expr.readsList i args
    | .cell v body | .method _ v body => Expr.reads i v || Expr.reads i body

  def Expr.readsList (i : Nat) : List Expr → Bool
    | [] => false
    | e :: es => Expr.reads i e || Expr.readsList i es

  def Expr.readsOpt (i : Nat) : Option Expr → Bool
    | none => false
    | some e => Expr.reads i e

  def Expr.readsStmts (i : Nat) : List Stmt → Bool
    | [] => false
    | .let_ _ _ e :: ss => Expr.reads i e || Expr.readsStmts i ss
    | .expr e :: ss => Expr.reads i e || Expr.readsStmts i ss
    | .var_ _ e :: ss => Expr.reads i e || Expr.readsStmts i ss
    | .assign _ e :: ss => Expr.reads i e || Expr.readsStmts i ss

  def Expr.readsArms (i : Nat) : List Arm → Bool
    | [] => false
    | .mk _ g b :: arms => Expr.readsOpt i g || Expr.reads i b || Expr.readsArms i arms
end

end OskaFormal.Core
