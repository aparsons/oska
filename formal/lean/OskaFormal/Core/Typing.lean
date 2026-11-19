import OskaFormal.Core.Syntax

/-!
# Oska Core: typing

The typing rules of Oska Core, version 1. A context lists the types of the
slots in scope, slot 0 first; a `let` that binds adds its slot at the end,
and it leaves scope with its block.

The rules for expressions, argument lists and blocks form one inductive
judgment, `Typed`, indexed by a `Goal`, so that a proof about typing is one
induction.
-/

namespace OskaFormal.Core

/-- The type of slot `slot` in a context, if it is in scope. -/
def lookup : List Ty → Nat → Option Ty
  | [], _ => none
  | t :: _, 0 => some t
  | _ :: ts, n + 1 => lookup ts n

/-- The types of the slots a function's parameters bind, slot 0 first. -/
def bindingTypes : List Param → List Ty
  | [] => []
  | p :: ps => if p.binds then p.type :: bindingTypes ps else bindingTypes ps

/-- `op` takes two operands of type `arg` and gives a result of type `res`:
arithmetic on `Int`, comparison of `Int`s, `==` and `!=` on two values of
one type, and `&&`, `||` and `implies` on `Bool`. -/
inductive BinOpTyped : BinOp → Ty → Ty → Prop where
  | add : BinOpTyped .add .int .int
  | sub : BinOpTyped .sub .int .int
  | mul : BinOpTyped .mul .int .int
  | div : BinOpTyped .div .int .int
  | rem : BinOpTyped .rem .int .int
  | lt : BinOpTyped .lt .int .bool
  | le : BinOpTyped .le .int .bool
  | gt : BinOpTyped .gt .int .bool
  | ge : BinOpTyped .ge .int .bool
  | eq (t : Ty) : BinOpTyped .eq t .bool
  | ne (t : Ty) : BinOpTyped .ne t .bool
  | and : BinOpTyped .and .bool .bool
  | or : BinOpTyped .or .bool .bool
  | implies : BinOpTyped .implies .bool .bool

/-- What a typing judgment is about: an expression and its type, a list of
arguments and their types, or a block's statements and value and the
block's type. -/
inductive Goal where
  | expr (e : Expr) (t : Ty)
  | args (es : List Expr) (ts : List Ty)
  | block (stmts : List Stmt) (value : Option Expr) (t : Ty)

/-- The typing judgment of Oska Core, in program `P` and context `Γ`. -/
inductive Typed (P : Program) : List Ty → Goal → Prop where
  | unit {Γ : List Ty} : Typed P Γ (.expr .unit .unit)
  | int {Γ : List Ty} {n : Int} : fits n = true → Typed P Γ (.expr (.int n) .int)
  | bool {Γ : List Ty} {b : Bool} : Typed P Γ (.expr (.bool b) .bool)
  | local_ {Γ : List Ty} {i : Nat} {t : Ty} :
      lookup Γ i = some t → Typed P Γ (.expr (.local_ i) t)
  | neg {Γ : List Ty} {e : Expr} :
      Typed P Γ (.expr e .int) → Typed P Γ (.expr (.neg e) .int)
  | not {Γ : List Ty} {e : Expr} :
      Typed P Γ (.expr e .bool) → Typed P Γ (.expr (.not e) .bool)
  | binary {Γ : List Ty} {op : BinOp} {l r : Expr} {a t : Ty} :
      BinOpTyped op a t → Typed P Γ (.expr l a) → Typed P Γ (.expr r a) →
      Typed P Γ (.expr (.binary op l r) t)
  | call {Γ : List Ty} {name : String} {f : Function} {args : List Expr} :
      P.find? name = some f → Typed P Γ (.args args (f.params.map Param.type)) →
      Typed P Γ (.expr (.call name args) f.ret)
  | ifElse {Γ : List Ty} {c th el : Expr} {t : Ty} :
      Typed P Γ (.expr c .bool) → Typed P Γ (.expr th t) → Typed P Γ (.expr el t) →
      Typed P Γ (.expr (.if_ c th (some el)) t)
  /-- An `if` with no `else` has type `()` whatever the type of `then`,
  whose value is dropped. The checker (`crates/oska-check/src/body.rs`)
  types it the same way. -/
  | ifNoElse {Γ : List Ty} {c th : Expr} {t : Ty} :
      Typed P Γ (.expr c .bool) → Typed P Γ (.expr th t) →
      Typed P Γ (.expr (.if_ c th none) .unit)
  | block {Γ : List Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.block ss v t) → Typed P Γ (.expr (.block ss v) t)
  | argsNil {Γ : List Ty} : Typed P Γ (.args [] [])
  | argsCons {Γ : List Ty} {e : Expr} {es : List Expr} {t : Ty} {ts : List Ty} :
      Typed P Γ (.expr e t) → Typed P Γ (.args es ts) → Typed P Γ (.args (e :: es) (t :: ts))
  | blockNone {Γ : List Ty} : Typed P Γ (.block [] none .unit)
  | blockSome {Γ : List Ty} {e : Expr} {t : Ty} :
      Typed P Γ (.expr e t) → Typed P Γ (.block [] (some e) t)
  | blockExpr {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P Γ (.block ss v t) →
      Typed P Γ (.block (.expr e :: ss) v t)
  | blockLetBind {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P (Γ ++ [u]) (.block ss v t) →
      Typed P Γ (.block (.let_ true u e :: ss) v t)
  | blockLetDrop {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P Γ (.block ss v t) →
      Typed P Γ (.block (.let_ false u e :: ss) v t)

/-- A program is well typed when each function's body has its result type,
in a context of the slots its parameters bind. -/
def ProgramTyped (P : Program) : Prop :=
  ∀ name f, P.find? name = some f → Typed P (bindingTypes f.params) (.expr f.body f.ret)

end OskaFormal.Core
