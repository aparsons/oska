import OskaFormal.Core.Syntax

/-!
# Oska Core: small-step semantics

A small-step semantics of Oska Core, version 1, matching the interpreter
(`crates/oska-run`) and the rules core IR version 1 states:

- Evaluation is left to right: an operator's `lhs` before its `rhs`, a
  call's arguments in order before the call, a block's statements in
  order.
- `Neg` and the arithmetic operators panic with an overflow when the
  result is not an `Int` (outside `-2^63 ..= 2^63 - 1`), and `/` and `%`
  panic with a division by zero when the divisor is 0, which is checked
  first. `/` rounds toward zero (`Int.tdiv`) and `%` has the sign of its
  left operand (`Int.tmod`), so `intMin / -1` overflows and `intMin % -1`
  is 0, as in the interpreter.
- `&&`, `||` and `implies` evaluate `rhs` only when the result needs it.
- A running expression is closed: a call puts its arguments' values for
  its parameters' slots, and a binding `let` puts its value for its slot
  in the rest of its block. Slots count from the first in scope, so
  putting a value for slot 0 renumbers slot `i + 1` as slot `i`.

The interpreter's limit on call depth, which stops runaway recursion with
a panic, is a resource limit and is not part of the semantics.
-/

namespace OskaFormal.Core

mutual
  /-- `e` with the closed value `v` put for slot 0, and each slot `i + 1`
  renumbered as `i`. -/
  def Expr.subst (v : Expr) : Expr → Expr
    | .unit => .unit
    | .int n => .int n
    | .bool b => .bool b
    | .local_ 0 => v
    | .local_ (i + 1) => .local_ i
    | .neg e => .neg (Expr.subst v e)
    | .not e => .not (Expr.subst v e)
    | .binary op l r => .binary op (Expr.subst v l) (Expr.subst v r)
    | .call name args => .call name (Expr.substList v args)
    | .if_ c th el => .if_ (Expr.subst v c) (Expr.subst v th) (Expr.substOpt v el)
    | .block ss val => .block (Expr.substStmts v ss) (Expr.substOpt v val)

  /-- `Expr.subst` on each expression of a list. -/
  def Expr.substList (v : Expr) : List Expr → List Expr
    | [] => []
    | e :: es => Expr.subst v e :: Expr.substList v es

  /-- `Expr.subst` on an optional expression. -/
  def Expr.substOpt (v : Expr) : Option Expr → Option Expr
    | none => none
    | some e => some (Expr.subst v e)

  /-- `Expr.subst` on each statement of a list. -/
  def Expr.substStmts (v : Expr) : List Stmt → List Stmt
    | [] => []
    | .let_ b t e :: ss => .let_ b t (Expr.subst v e) :: Expr.substStmts v ss
    | .expr e :: ss => .expr (Expr.subst v e) :: Expr.substStmts v ss
end

/-- A function's body with its arguments' values put for the slots its
parameters bind; the value of a parameter that binds no slot is dropped. -/
def instantiate : List Param → List Expr → Expr → Expr
  | [], _, body => body
  | _ :: _, [], body => body
  | p :: ps, v :: vs, body =>
    if p.binds then instantiate ps vs (Expr.subst v body) else instantiate ps vs body

/-- Why a running program panics. -/
inductive PanicKind where
  /-- An `Int` result outside `-2^63 ..= 2^63 - 1` (the interpreter's
  `P0001`). -/
  | overflow
  /-- A `/` or `%` by 0 (the interpreter's `P0004`). -/
  | divisionByZero
  deriving DecidableEq, Repr

/-- What one step leads to: the next expression, or a panic. -/
inductive Outcome where
  | next (e : Expr)
  | panic (kind : PanicKind)

/-- `f` applied to the next expression, a panic left as it is. -/
def Outcome.map (f : Expr → Expr) : Outcome → Outcome
  | .next e => .next (f e)
  | .panic k => .panic k

/-- The integer `n` as a result: an overflow when it is not an `Int`. -/
def intResult (n : Int) : Outcome :=
  if fits n then .next (.int n) else .panic .overflow

/-- Whether two values are equal, for `==` and `!=`. -/
def valueEq : Expr → Expr → Bool
  | .unit, .unit => true
  | .int a, .int b => decide (a = b)
  | .bool a, .bool b => a == b
  | _, _ => false

/-- An operator that evaluates both operands, applied to their values;
`none` for `&&`, `||` and `implies`, and for operands of the wrong kind. -/
def evalBinary : BinOp → Expr → Expr → Option Outcome
  | .add, .int a, .int b => some (intResult (a + b))
  | .sub, .int a, .int b => some (intResult (a - b))
  | .mul, .int a, .int b => some (intResult (a * b))
  | .div, .int a, .int b =>
    some (if b = 0 then .panic .divisionByZero else intResult (a.tdiv b))
  | .rem, .int a, .int b =>
    some (if b = 0 then .panic .divisionByZero else intResult (a.tmod b))
  | .lt, .int a, .int b => some (.next (.bool (decide (a < b))))
  | .le, .int a, .int b => some (.next (.bool (decide (a ≤ b))))
  | .gt, .int a, .int b => some (.next (.bool (decide (a > b))))
  | .ge, .int a, .int b => some (.next (.bool (decide (a ≥ b))))
  | .eq, a, b => some (.next (.bool (valueEq a b)))
  | .ne, a, b => some (.next (.bool (!valueEq a b)))
  | _, _, _ => none

/-- Whether an operator may skip its `rhs`: `&&`, `||` and `implies`. -/
def BinOp.isLogical : BinOp → Bool
  | .and | .or | .implies => true
  | _ => false

/-- One step of a closed expression in program `P`. -/
inductive Step (P : Program) : Expr → Outcome → Prop where
  | negArg {e : Expr} {o : Outcome} :
      Step P e o → Step P (.neg e) (o.map .neg)
  | neg {n : Int} : Step P (.neg (.int n)) (intResult (-n))
  | notArg {e : Expr} {o : Outcome} :
      Step P e o → Step P (.not e) (o.map .not)
  | not {b : Bool} : Step P (.not (.bool b)) (.next (.bool (!b)))
  | binaryLhs {op : BinOp} {l r : Expr} {o : Outcome} :
      Step P l o → Step P (.binary op l r) (o.map fun l' => .binary op l' r)
  | binaryRhs {op : BinOp} {l r : Expr} {o : Outcome} :
      op.isLogical = false → IsValue l → Step P r o →
      Step P (.binary op l r) (o.map fun r' => .binary op l r')
  | binary {op : BinOp} {l r : Expr} {o : Outcome} :
      IsValue l → IsValue r → evalBinary op l r = some o → Step P (.binary op l r) o
  | andFalse {r : Expr} : Step P (.binary .and (.bool false) r) (.next (.bool false))
  | andTrue {r : Expr} : Step P (.binary .and (.bool true) r) (.next r)
  | orTrue {r : Expr} : Step P (.binary .or (.bool true) r) (.next (.bool true))
  | orFalse {r : Expr} : Step P (.binary .or (.bool false) r) (.next r)
  | impliesFalse {r : Expr} : Step P (.binary .implies (.bool false) r) (.next (.bool true))
  | impliesTrue {r : Expr} : Step P (.binary .implies (.bool true) r) (.next r)
  | callArg {name : String} {vs rest : List Expr} {e : Expr} {o : Outcome} :
      AllValues vs → Step P e o →
      Step P (.call name (vs ++ e :: rest)) (o.map fun e' => .call name (vs ++ e' :: rest))
  | call {name : String} {f : Function} {vs : List Expr} :
      P.find? name = some f → AllValues vs →
      Step P (.call name vs) (.next (instantiate f.params vs f.body))
  | ifCond {c th : Expr} {el : Option Expr} {o : Outcome} :
      Step P c o → Step P (.if_ c th el) (o.map fun c' => .if_ c' th el)
  | ifTrue {th el : Expr} : Step P (.if_ (.bool true) th (some el)) (.next th)
  | ifFalse {th el : Expr} : Step P (.if_ (.bool false) th (some el)) (.next el)
  | ifTrueNoElse {th : Expr} :
      Step P (.if_ (.bool true) th none) (.next (.block [.expr th] none))
  | ifFalseNoElse {th : Expr} : Step P (.if_ (.bool false) th none) (.next .unit)
  | blockNone : Step P (.block [] none) (.next .unit)
  | blockSome {e : Expr} : Step P (.block [] (some e)) (.next e)
  | blockExprArg {e : Expr} {ss : List Stmt} {val : Option Expr} {o : Outcome} :
      Step P e o → Step P (.block (.expr e :: ss) val) (o.map fun e' => .block (.expr e' :: ss) val)
  | blockExpr {v : Expr} {ss : List Stmt} {val : Option Expr} :
      IsValue v → Step P (.block (.expr v :: ss) val) (.next (.block ss val))
  | blockLetArg {b : Bool} {t : Ty} {e : Expr} {ss : List Stmt} {val : Option Expr} {o : Outcome} :
      Step P e o →
      Step P (.block (.let_ b t e :: ss) val) (o.map fun e' => .block (.let_ b t e' :: ss) val)
  | blockLetBind {t : Ty} {v : Expr} {ss : List Stmt} {val : Option Expr} :
      IsValue v →
      Step P (.block (.let_ true t v :: ss) val)
        (.next (.block (Expr.substStmts v ss) (Expr.substOpt v val)))
  | blockLetDrop {t : Ty} {v : Expr} {ss : List Stmt} {val : Option Expr} :
      IsValue v → Step P (.block (.let_ false t v :: ss) val) (.next (.block ss val))

end OskaFormal.Core
