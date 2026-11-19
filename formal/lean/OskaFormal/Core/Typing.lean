import OskaFormal.Core.Syntax

/-!
# Oska Core: typing

The typing rules of Oska Core, version 2. A context lists the types of the
slots in scope, slot 0 first; a `let` that binds adds its slot at the end,
and it leaves scope with its block. A `match` arm's pattern adds its
bindings' slots at the end, for its guard and body.

The rules for expressions, argument lists, blocks and `match` arms form
one inductive judgment, `Typed`, indexed by a `Goal`, so that a proof about
typing is one induction. Patterns (`PatTyped`) and values (`ValueTyped`)
have judgments of their own, indexed the same way.

A `match` is exhaustive: every value of the scrutinee's type matches an
arm with no guard. The checker decides that (`crates/oska-check/src/
usefulness.rs`); the typing rule states it of the values themselves.
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
  | eq (t : Ty) : t.isBase = true → BinOpTyped .eq t .bool
  | ne (t : Ty) : t.isBase = true → BinOpTyped .ne t .bool
  | and : BinOpTyped .and .bool .bool
  | or : BinOpTyped .or .bool .bool
  | implies : BinOpTyped .implies .bool .bool

/-- What a value typing is about: a value and its type, or a list of
values and their types. -/
inductive VGoal where
  | one (v : Expr) (t : Ty)
  | many (vs : List Expr) (ts : List Ty)

/-- The types of the values of Oska Core in program `P`: an `Int` in
range, a boolean, `()`, and a struct or variant `P` declares whose fields
have its field types. -/
inductive ValueTyped (P : Program) : VGoal → Prop where
  | unit : ValueTyped P (.one .unit .unit)
  | int {n : Int} : fits n = true → ValueTyped P (.one (.int n) .int)
  | bool {b : Bool} : ValueTyped P (.one (.bool b) .bool)
  | construct {type : String} {variant : Option String} {fs : List Expr} {fts : List Ty} :
      P.fieldsOf type variant = some fts → ValueTyped P (.many fs fts) →
      ValueTyped P (.one (.construct type variant fs) (.named type))
  | nil : ValueTyped P (.many [] [])
  | cons {v : Expr} {vs : List Expr} {t : Ty} {ts : List Ty} :
      ValueTyped P (.one v t) → ValueTyped P (.many vs ts) → ValueTyped P (.many (v :: vs) (t :: ts))

/-- What a pattern typing is about: a pattern, the type of the value it
matches and the types of the slots it binds, or the same for a list of
patterns. -/
inductive PGoal where
  | one (p : Pattern) (t : Ty) (binds : List Ty)
  | many (ps : List Pattern) (ts : List Ty) (binds : List Ty)

/-- The typing of patterns in program `P`. -/
inductive PatTyped (P : Program) : PGoal → Prop where
  | wildcard {t : Ty} : PatTyped P (.one .wildcard t [])
  | bind {t : Ty} : PatTyped P (.one (.bind t) t [t])
  | unit : PatTyped P (.one .unit .unit [])
  | int {n : Int} : PatTyped P (.one (.int n) .int [])
  | bool {b : Bool} : PatTyped P (.one (.bool b) .bool [])
  | construct {type : String} {variant : Option String} {ps : List Pattern} {fts bs : List Ty} :
      P.fieldsOf type variant = some fts → PatTyped P (.many ps fts bs) →
      PatTyped P (.one (.construct type variant ps) (.named type) bs)
  | nil : PatTyped P (.many [] [] [])
  | cons {p : Pattern} {ps : List Pattern} {t : Ty} {ts bs bss : List Ty} :
      PatTyped P (.one p t bs) → PatTyped P (.many ps ts bss) →
      PatTyped P (.many (p :: ps) (t :: ts) (bs ++ bss))

/-- The value `v` matches an arm of `arms` that has no guard, so a `match`
of `v` on `arms` takes an arm. -/
def Covered (arms : List Arm) (v : Expr) : Prop :=
  ∃ p body, Arm.mk p none body ∈ arms ∧ (matchPat p v).isSome = true

/-- What a typing judgment is about: an expression and its type, a list of
arguments and their types, a block's statements and value and the block's
type, or a `match`'s arms, the type of the value they match and the type
of their bodies. -/
inductive Goal where
  | expr (e : Expr) (t : Ty)
  | args (es : List Expr) (ts : List Ty)
  | block (stmts : List Stmt) (value : Option Expr) (t : Ty)
  | arms (arms : List Arm) (scrutinee : Ty) (t : Ty)

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
  /-- A struct or enum variant has the type of its struct or enum. -/
  | construct {Γ : List Ty} {type : String} {variant : Option String} {fs : List Expr}
      {fts : List Ty} :
      P.fieldsOf type variant = some fts → Typed P Γ (.args fs fts) →
      Typed P Γ (.expr (.construct type variant fs) (.named type))
  | field {Γ : List Ty} {b : Expr} {type : String} {fts : List Ty} {i : Nat} {t : Ty} :
      P.fieldsOf type none = some fts → fts[i]? = some t → Typed P Γ (.expr b (.named type)) →
      Typed P Γ (.expr (.field b i) t)
  /-- A `match` whose arms match values of the scrutinee's type and have
  bodies of type `t`, and that is exhaustive: every value of the
  scrutinee's type, or the scrutinee itself once it is a value, matches
  an arm with no guard. -/
  | match_ {Γ : List Ty} {s : Expr} {arms : List Arm} {u t : Ty} :
      Typed P Γ (.expr s u) → Typed P Γ (.arms arms u t) →
      (∀ v, ValueTyped P (.one v u) → (IsValue s → s = v) → Covered arms v) →
      Typed P Γ (.expr (.match_ s arms) t)
  | armsNil {Γ : List Ty} {u t : Ty} : Typed P Γ (.arms [] u t)
  | armsCons {Γ : List Ty} {p : Pattern} {body : Expr} {rest : List Arm} {u t : Ty} {bs : List Ty} :
      PatTyped P (.one p u bs) → Typed P (Γ ++ bs) (.expr body t) → Typed P Γ (.arms rest u t) →
      Typed P Γ (.arms (.mk p none body :: rest) u t)
  | armsConsGuard {Γ : List Ty} {p : Pattern} {g body : Expr} {rest : List Arm} {u t : Ty}
      {bs : List Ty} :
      PatTyped P (.one p u bs) → Typed P (Γ ++ bs) (.expr g .bool) →
      Typed P (Γ ++ bs) (.expr body t) → Typed P Γ (.arms rest u t) →
      Typed P Γ (.arms (.mk p (some g) body :: rest) u t)
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
