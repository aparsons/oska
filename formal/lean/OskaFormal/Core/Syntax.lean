/-!
# Oska Core: syntax

The syntax of Oska Core, version 1 (ADR 0018): exactly the fragment that
core IR version 1 exports (`crates/oska-check/src/core_ir.rs`). It has
functions, `let`, `if`, `Int` and `Bool` with their operators, and `()`.
Each Lean constructor is one core IR node, with the same fields.

- A local is a *slot*: the function's locals are numbered from 0 in the
  order they are bound, the parameters that bind first, then each `let`
  that binds. A parameter or `let` with `binds = false` takes no slot.
- `Int` is a signed 64-bit integer: an `Int` value lies in
  `intMin ..= intMax`, and arithmetic outside that range panics.
-/

namespace OskaFormal.Core

/-- The types of Oska Core: core IR's `"Int"`, `"Bool"` and `"()"`. -/
inductive Ty where
  | int
  | bool
  | unit
  deriving DecidableEq, Repr

/-- A binary operator, as core IR writes it in `op`. -/
inductive BinOp where
  | add | sub | mul | div | rem
  | lt | le | gt | ge
  | eq | ne
  | and | or | implies
  deriving DecidableEq, Repr

mutual
  /-- An expression of Oska Core: one constructor per core IR `expr`
  kind. An `if` with no `else`, and a block with no value, hold `none`. -/
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

  /-- A statement of Oska Core: core IR's `Let` and `Expr`. -/
  inductive Stmt where
    | let_ (binds : Bool) (type : Ty) (value : Expr)
    | expr (value : Expr)
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

/-- A program of Oska Core: its functions, in source order. -/
abbrev Program := List Function

/-- The function a `Call` of `name` reaches: the first of that name. -/
def Program.find? : Program → String → Option Function
  | [], _ => none
  | f :: fs, name => if f.name = name then some f else Program.find? fs name

/-- The smallest `Int`, `-2^63`. -/
def intMin : Int := -(2 ^ 63)

/-- The largest `Int`, `2^63 - 1`. -/
def intMax : Int := 2 ^ 63 - 1

/-- Whether `n` is an `Int` of Oska: whether it fits in 64 signed bits. -/
def fits (n : Int) : Bool := decide (intMin ≤ n) && decide (n ≤ intMax)

/-- The values of Oska Core: `()`, an integer and a boolean. -/
inductive IsValue : Expr → Prop where
  | unit : IsValue .unit
  | int (n : Int) : IsValue (.int n)
  | bool (b : Bool) : IsValue (.bool b)

/-- Every expression of a list is a value. -/
inductive AllValues : List Expr → Prop where
  | nil : AllValues []
  | cons {v : Expr} {vs : List Expr} : IsValue v → AllValues vs → AllValues (v :: vs)

end OskaFormal.Core
