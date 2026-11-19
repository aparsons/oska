/-!
# Oska Core: syntax

The syntax of Oska Core, version 2 (ADR 0018): exactly the fragment that
core IR version 2 exports (`crates/oska-check/src/core_ir.rs`). It has
functions, `let`, `if`, `Int` and `Bool` with their operators, `()`,
structs and enums without generics, field access, and `match`. Each Lean
constructor is one core IR node, with the same fields.

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

/-- The types of Oska Core: core IR's `"Int"`, `"Bool"` and `"()"`, and a
struct or enum the program declares, by name. -/
inductive Ty where
  | int
  | bool
  | unit
  | named (name : String)
  deriving DecidableEq, Repr

/-- The types `==` and `!=` compare: `Int`, `Bool` and `()`. The checker
compares a struct or enum only through an `Eq` implementation, which is
outside Oska Core. -/
def Ty.isBase : Ty → Bool
  | .int | .bool | .unit => true
  | .named _ => false

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

/-- The values of Oska Core: `()`, an integer, a boolean, and a struct or
enum variant whose fields are values. -/
inductive IsValue : Expr → Prop where
  | unit : IsValue .unit
  | int (n : Int) : IsValue (.int n)
  | bool (b : Bool) : IsValue (.bool b)
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
