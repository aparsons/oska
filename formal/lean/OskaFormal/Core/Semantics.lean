import OskaFormal.Core.Syntax

/-!
# Oska Core: small-step semantics

A small-step semantics of Oska Core, version 6, matching the interpreter
(`crates/oska-run`) and the rules core IR version 6 states:

- Evaluation is left to right: an operator's `lhs` before its `rhs`, a
  call's arguments in order before the call, a block's statements in
  order, a struct or variant's fields in declaration order.
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
- A closure captures by value: putting a value for a slot of the
  enclosing function puts it in each closure written below it too, which
  then has one slot fewer below its parameters (`height`). A closure is a
  value once its height is 0, with each slot it captured put in its body
  as the slot was bound. Values never change in Oska Core, so this is the
  interpreter's capture when the closure is made (`ClosureValue`). A call
  of a function value evaluates the callee, then its arguments left to
  right, then puts the arguments for the closure's parameters' slots in
  its body, as a call of a function does (`instantiate`).
- A call, of a function or of a function value, runs the body in a
  `frame`, which gives the body's value once it has one.
- `Ok(e)` and `Err(e)` evaluate `e`. `e?` evaluates `e`: on `Ok(v)` it
  gives `v`, and on `Err(v)` the step **returns** `Err(v)` (the outcome
  `ret`), which every enclosing expression passes on as it does a panic,
  until the nearest `frame`, which steps to it: the function or closure
  `e?` is in returns it. When the checker chose a `From` conversion `f`,
  `Err(v)?` first steps to `Err(f(v))?`, so the conversion runs (and may
  panic) before the return, as in the interpreter (`try_expr`).
- A `match` evaluates its scrutinee, then tries its arms in order. An arm
  whose pattern does not match the value is skipped. One whose pattern
  matches puts the values it binds for their slots in its guard and body;
  without a guard it steps to its body, and with one to
  `if guard { body } else { the match of the rest }`, so a false guard
  goes on to the next arm, as in the interpreter.
- A `var` binding evaluates its value, then runs the rest of its block in
  a `cell` holding it: the var. Reading var `i` (`var_ i`) steps to the
  outcome `read`, and an assignment, once its value is a value, to the
  outcome `write`; each enclosing expression passes them on as it does
  an effect, until the cell of var `i` answers: a read with the value
  the cell holds (the field its path names), a write by putting the new
  value at its path in the cell's own value and nowhere else. Every
  other value is left as it is: a `let`, an argument or a field holds a
  value, which never changes, so a change through a `var` is never seen
  through another binding (ADR 0001's value semantics, which the
  interpreter keeps by copying a shared value before it writes it).
- A mutating call `mutCall f place args` evaluates its arguments, then
  reads the receiver at `place` and runs `f`'s body with them in a
  `method` node, where the receiver is var 0, the body's own. When the
  body has its value (or returns with `?`), the node writes the
  receiver's final value back to `place` and gives the body's value: the
  interpreter's `mutating_call_expr`, which takes the receiver out of its
  place and puts it back. A body sees no var of its caller, so the call
  changes its receiver and nothing else. A `Call` of a mutating method
  (on a temporary) runs its body with the receiver in a cell, whose final
  value is dropped.

- A built-in call evaluates its arguments left to right. A pure one
  (`Path`, `Result`, `read_only`, `write_only`) then steps to its value.
  An **effect**, a call on a capability that asks the host for something
  (`println`, `restrict_to`, `read_to_string`, `write`, `create_dir`),
  steps to `perform req k`: the request `req`, labelled with the kind of
  its receiver, and the expression `k r` the run goes on with once the
  host answers `r`. The semantics does not fix the host: it allows any
  answer `req.Allows`, so its theorems hold on every host that answers
  so, and `Eval.lean`'s fake host is one. A run's **trace** is the list
  of its requests (`Steps`).

The interpreter's limit on call depth, which stops runaway recursion with
a panic, is a resource limit and is not part of the semantics.
-/

namespace OskaFormal.Core

/-- What a closure captures once a value is put for slot 0: slot 0 no
longer (its value is in the body), and slot `i + 1` as `i`. -/
def shiftCaptures (cs : List Nat) : List Nat :=
  cs.filterMap fun c => if c = 0 then none else some (c - 1)

mutual
  /-- `e` with the closed value `v` put for slot 0, and each slot `i + 1`
  renumbered as `i`. A closure of height 0 has no slot of the enclosing
  function in scope, so it is left as it is. -/
  def Expr.subst (v : Expr) : Expr → Expr
    | .unit => .unit
    | .int n => .int n
    | .bool b => .bool b
    | .str s => .str s
    | .path s => .path s
    | .cap k r => .cap k r
    | .ioError => .ioError
    | .result b e => .result b (Expr.subst v e)
    | .try_ e c => .try_ (Expr.subst v e) c
    | .frame e => .frame (Expr.subst v e)
    | .local_ 0 => v
    | .local_ (i + 1) => .local_ i
    | .neg e => .neg (Expr.subst v e)
    | .not e => .not (Expr.subst v e)
    | .binary op l r => .binary op (Expr.subst v l) (Expr.subst v r)
    | .call name args => .call name (Expr.substList v args)
    | .if_ c th el => .if_ (Expr.subst v c) (Expr.subst v th) (Expr.substOpt v el)
    | .block ss val => .block (Expr.substStmts v ss) (Expr.substOpt v val)
    | .construct t var fs => .construct t var (Expr.substList v fs)
    | .field b i => .field (Expr.subst v b) i
    | .match_ s arms => .match_ (Expr.subst v s) (Expr.substArms v arms)
    | .builtin op args => .builtin op (Expr.substList v args)
    | .closure ps 0 cs cf ret body => .closure ps 0 cs cf ret body
    | .closure ps (h + 1) cs cf ret body =>
      .closure ps h (shiftCaptures cs) cf ret (Expr.subst v body)
    | .callValue c args => .callValue (Expr.subst v c) (Expr.substList v args)
    | .var_ i => .var_ i
    | .mutCall f pl args => .mutCall f pl (Expr.substList v args)
    | .cell w body => .cell (Expr.subst v w) (Expr.subst v body)
    | .method pl w body => .method pl (Expr.subst v w) (Expr.subst v body)

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
    | .var_ t e :: ss => .var_ t (Expr.subst v e) :: Expr.substStmts v ss
    | .assign pl e :: ss => .assign pl (Expr.subst v e) :: Expr.substStmts v ss

  /-- `Expr.subst` on each arm's guard and body. A pattern holds no
  expression. -/
  def Expr.substArms (v : Expr) : List Arm → List Arm
    | [] => []
    | .mk p g b :: arms => .mk p (Expr.substOpt v g) (Expr.subst v b) :: Expr.substArms v arms
end

/-- `e` with the closed values `vs` put for its first slots, slot 0
first: a `match` arm's guard or body with the values its pattern binds. -/
def Expr.substAll : List Expr → Expr → Expr
  | [], e => e
  | v :: vs, e => Expr.substAll vs (Expr.subst v e)

/-- A function's or closure's body with its arguments' values put for the
slots its parameters bind; the value of a parameter that binds no slot is
dropped. `none` when the number of arguments is not the number of
parameters, so a call of the wrong arity takes no step. -/
def instantiate : List Param → List Expr → Expr → Option Expr
  | [], [], body => some body
  | p :: ps, v :: vs, body =>
    if p.binds then instantiate ps vs (Expr.subst v body) else instantiate ps vs body
  | _, _, _ => none

/-- The body a call of `f` with the arguments `vs` runs: `instantiate`d,
and for a mutating method (called on a temporary) in a cell holding the
receiver, its first argument, whose final value is dropped. `none` for
the wrong number of arguments. -/
def Function.enter (f : Function) (vs : List Expr) : Option Expr :=
  match f.receiver, vs with
  | none, vs => instantiate f.params vs f.body
  | some _, r :: vs => (instantiate f.params vs f.body).map (.cell r)
  | some _, [] => none

/-- The field at `path` in the value `v`, outermost first: `v` for the
empty path. `()` where `v` has no such field, which a typed place never
asks for. -/
def Expr.getAt : Expr → List Nat → Expr
  | v, [] => v
  | .construct _ none fs, i :: is => Expr.getAt (fs[i]?.getD .unit) is
  | _, _ :: _ => .unit

/-- The value `v` with `w` put at `path`, outermost first, and nothing
else changed: `w` for the empty path. `v` as it is where it has no such
field, which a typed place never asks for. -/
def Expr.setAt : Expr → List Nat → Expr → Expr
  | _, [], w => w
  | .construct t none fs, i :: is, w =>
    .construct t none (fs.set i (Expr.setAt (fs[i]?.getD .unit) is w))
  | v, _ :: _, _ => v

/-- Why a running program panics. -/
inductive PanicKind where
  /-- An `Int` result outside `-2^63 ..= 2^63 - 1` (the interpreter's
  `P0001`). -/
  | overflow
  /-- A `/` or `%` by 0 (the interpreter's `P0004`). -/
  | divisionByZero
  deriving DecidableEq, Repr

/-- An **effect**: what a built-in call on a capability asks the host
for. `kind` is the kind of its receiver, the capability it uses, whose
root is `root`; `args` are its other arguments' values. A run's trace is
the list of its effects. -/
structure Request where
  op : Builtin
  kind : CapKind
  root : CapRoot
  args : List Expr

/-- The answers the semantics allows the host to give a request: `Ok(())`
or an `Err` for a write to stdout, a write or a new directory; the text
read or an `Err` for a read; and for `restrict_to`, a capability of the
kind it gives whose root reaches no path the receiver's does not, so
attenuation never widens. -/
def Request.Allows (req : Request) (r : Expr) : Prop :=
  match req.op with
  | .println | .write | .createDir => r = .okUnit ∨ r = .ioErr
  | .readToString => r = .ioErr ∨ ∃ s, r = .okStr s
  | .restrictTo _ =>
    ∃ a k' root', req.kind.restrictTo = some (a, k') ∧ r = .cap k' root' ∧ root'.Within req.root
  | _ => False

/-- What one step leads to: the next expression, a panic, an effect
`req`, after which the run goes on with `k r` for the host's answer `r`,
a return of the value `v` from the function or closure the step is in
(`?` on an `Err`), a read of the field at `path` of var `var`, after
which the run goes on with `k` of it, or a write of `value` there, after
which it goes on with `next`. -/
inductive Outcome where
  | next (e : Expr)
  | panic (kind : PanicKind)
  | perform (req : Request) (k : Expr → Expr)
  | ret (v : Expr)
  | read (var : Nat) (path : List Nat) (k : Expr → Expr)
  | write (var : Nat) (path : List Nat) (value : Expr) (next : Expr)

/-- `f` applied to the next expression, or to what follows an effect, a
read or a write; a panic or a return left as it is. -/
def Outcome.map (f : Expr → Expr) : Outcome → Outcome
  | .next e => .next (f e)
  | .panic k => .panic k
  | .perform req k => .perform req fun r => f (k r)
  | .ret v => .ret v
  | .read i is k => .read i is fun r => f (k r)
  | .write i is w e => .write i is w (f e)

/-- A step of a body in a `frame`: a return gives its value, and any other
outcome stays in the frame. -/
def Outcome.inFrame : Outcome → Outcome
  | .ret v => .next v
  | o => o.map .frame

/-- A step of a body that holds var 0, whose value is `v`, wrapped back
with `wrap`: the binder answers a read of var 0 with the field of `v` it
asks for, and takes a write of var 0 into its own value; it passes a
read or write of var `i + 1` on as one of var `i`, and a return to
`onRet`. -/
def Outcome.under (v : Expr) (wrap : Expr → Expr → Expr) (onRet : Expr → Outcome) :
    Outcome → Outcome
  | .next e => .next (wrap v e)
  | .panic k => .panic k
  | .perform req k => .perform req fun r => wrap v (k r)
  | .ret r => onRet r
  | .read 0 is k => .next (wrap v (k (v.getAt is)))
  | .read (i + 1) is k => .read i is fun r => wrap v (k r)
  | .write 0 is w e => .next (wrap (v.setAt is w) e)
  | .write (i + 1) is w e => .write i is w (wrap v e)

/-- A step of the rest of a block in the `cell` of its `var`, which holds
`v`: a return leaves the cell. -/
def Outcome.inCell (v : Expr) : Outcome → Outcome :=
  Outcome.under v .cell .ret

/-- A step of a mutating method's body, whose receiver holds `v`: a
return ends the body with its value, as a `frame` does, and the receiver
is still written back. -/
def Outcome.inMethod (pl : Place) (v : Expr) : Outcome → Outcome :=
  Outcome.under v (.method pl) fun r => .next (.method pl v r)

/-- A pure built-in call applied to its arguments' values: `Path.from`
and `Path.new` make a path, `is_ok` and `unwrap_or` read a `Result`, and
`read_only` and `write_only` attenuate a capability to a narrower kind
with the same root. `none` for an effect, or for arguments of the wrong
kind. -/
def evalPure : Builtin → List Expr → Option Expr
  | .pathFrom, [.str s] | .pathNew, [.str s] => some (.path s)
  | .isOk, [.result b _] => some (.bool b)
  | .unwrapOr, [.result true v, _] => some v
  | .unwrapOr, [.result false _, d] => some d
  | .readOnly k, [.cap k' r] => if k' = k then k.readOnly.map (.cap · r) else none
  | .writeOnly k, [.cap k' r] => if k' = k then k.writeOnly.map (.cap · r) else none
  | _, _ => none

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
  | call {name : String} {f : Function} {vs : List Expr} {body : Expr} :
      P.find? name = some f → AllValues vs → f.enter vs = some body →
      Step P (.call name vs) (.next (.frame body))
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
  | constructArg {type : String} {variant : Option String} {vs rest : List Expr} {e : Expr}
      {o : Outcome} :
      AllValues vs → Step P e o →
      Step P (.construct type variant (vs ++ e :: rest))
        (o.map fun e' => .construct type variant (vs ++ e' :: rest))
  | fieldBase {b : Expr} {i : Nat} {o : Outcome} :
      Step P b o → Step P (.field b i) (o.map fun b' => .field b' i)
  | field {type : String} {fs : List Expr} {i : Nat} {v : Expr} :
      AllValues fs → fs[i]? = some v → Step P (.field (.construct type none fs) i) (.next v)
  | builtinArg {op : Builtin} {vs rest : List Expr} {e : Expr} {o : Outcome} :
      AllValues vs → Step P e o →
      Step P (.builtin op (vs ++ e :: rest)) (o.map fun e' => .builtin op (vs ++ e' :: rest))
  | builtinPure {op : Builtin} {vs : List Expr} {v : Expr} :
      AllValues vs → evalPure op vs = some v → Step P (.builtin op vs) (.next v)
  /-- An effect: a built-in call on a capability of its receiver's kind
  asks the host, and the call's value is the answer. -/
  | builtinEffect {op : Builtin} {k : CapKind} {r : CapRoot} {vs : List Expr} :
      op.receiver = some k → AllValues vs →
      Step P (.builtin op (.cap k r :: vs)) (.perform ⟨op, k, r, vs⟩ fun x => x)
  | matchScrutinee {s : Expr} {arms : List Arm} {o : Outcome} :
      Step P s o → Step P (.match_ s arms) (o.map fun s' => .match_ s' arms)
  | matchSkip {v : Expr} {p : Pattern} {g : Option Expr} {body : Expr} {rest : List Arm} :
      IsValue v → matchPat p v = none →
      Step P (.match_ v (.mk p g body :: rest)) (.next (.match_ v rest))
  | matchArm {v : Expr} {p : Pattern} {body : Expr} {rest : List Arm} {bs : List Expr} :
      IsValue v → matchPat p v = some bs →
      Step P (.match_ v (.mk p none body :: rest)) (.next (Expr.substAll bs body))
  | matchGuard {v : Expr} {p : Pattern} {g body : Expr} {rest : List Arm} {bs : List Expr} :
      IsValue v → matchPat p v = some bs →
      Step P (.match_ v (.mk p (some g) body :: rest))
        (.next (.if_ (Expr.substAll bs g) (Expr.substAll bs body) (some (.match_ v rest))))
  | callValueCallee {c : Expr} {args : List Expr} {o : Outcome} :
      Step P c o → Step P (.callValue c args) (o.map fun c' => .callValue c' args)
  | callValueArg {c : Expr} {vs rest : List Expr} {e : Expr} {o : Outcome} :
      IsValue c → AllValues vs → Step P e o →
      Step P (.callValue c (vs ++ e :: rest)) (o.map fun e' => .callValue c (vs ++ e' :: rest))
  /-- A call of a closure the run has closed steps into its body, with the
  arguments put for its parameters' slots. -/
  | callValue {ps : List Param} {cs : List Nat} {cap : Bool} {ret : Ty} {body : Expr}
      {vs : List Expr} {e : Expr} :
      AllValues vs → instantiate ps vs body = some e →
      Step P (.callValue (.closure ps 0 cs cap ret body) vs) (.next (.frame e))
  | resultArg {b : Bool} {e : Expr} {o : Outcome} :
      Step P e o → Step P (.result b e) (o.map (.result b))
  | tryArg {e : Expr} {c : Option String} {o : Outcome} :
      Step P e o → Step P (.try_ e c) (o.map fun e' => .try_ e' c)
  | tryOk {v : Expr} {c : Option String} :
      IsValue v → Step P (.try_ (.result true v) c) (.next v)
  /-- `Err(v)?` returns `Err(v)` from the function or closure it is in. -/
  | tryErr {v : Expr} :
      IsValue v → Step P (.try_ (.result false v) none) (.ret (.result false v))
  /-- With a conversion, the error goes through it first. -/
  | tryConvert {v : Expr} {f : String} :
      IsValue v → Step P (.try_ (.result false v) (some f))
        (.next (.try_ (.result false (.call f [v])) none))
  | frame {e : Expr} {o : Outcome} :
      Step P e o → Step P (.frame e) o.inFrame
  | frameValue {v : Expr} :
      IsValue v → Step P (.frame v) (.next v)
  /-- Reading a var asks the cell that holds it. -/
  | var {i : Nat} : Step P (.var_ i) (.read i [] fun x => x)
  | blockVarArg {t : Ty} {e : Expr} {ss : List Stmt} {val : Option Expr} {o : Outcome} :
      Step P e o →
      Step P (.block (.var_ t e :: ss) val) (o.map fun e' => .block (.var_ t e' :: ss) val)
  /-- A `var` binding runs the rest of its block in a cell holding its
  value. -/
  | blockVar {t : Ty} {v : Expr} {ss : List Stmt} {val : Option Expr} :
      IsValue v → Step P (.block (.var_ t v :: ss) val) (.next (.cell v (.block ss val)))
  | blockAssignArg {pl : Place} {e : Expr} {ss : List Stmt} {val : Option Expr} {o : Outcome} :
      Step P e o →
      Step P (.block (.assign pl e :: ss) val) (o.map fun e' => .block (.assign pl e' :: ss) val)
  /-- An assignment writes its value at its place. -/
  | blockAssign {pl : Place} {v : Expr} {ss : List Stmt} {val : Option Expr} :
      IsValue v →
      Step P (.block (.assign pl v :: ss) val) (.write pl.var pl.fields v (.block ss val))
  | cellBody {v e : Expr} {o : Outcome} :
      Step P e o → Step P (.cell v e) (o.inCell v)
  /-- The block ends, and its var with it. -/
  | cellValue {v w : Expr} :
      IsValue w → Step P (.cell v w) (.next w)
  | mutCallArg {name : String} {pl : Place} {vs rest : List Expr} {e : Expr} {o : Outcome} :
      AllValues vs → Step P e o →
      Step P (.mutCall name pl (vs ++ e :: rest))
        (o.map fun e' => .mutCall name pl (vs ++ e' :: rest))
  /-- Once its arguments are values, a mutating call reads its receiver
  and runs the method's body with it. -/
  | mutCall {name : String} {pl : Place} {f : Function} {u : Ty} {vs : List Expr} {body : Expr} :
      P.find? name = some f → f.receiver = some u → AllValues vs →
      instantiate f.params vs f.body = some body →
      Step P (.mutCall name pl vs) (.read pl.var pl.fields fun r => .method pl r body)
  | methodBody {pl : Place} {v e : Expr} {o : Outcome} :
      Step P e o → Step P (.method pl v e) (o.inMethod pl v)
  /-- The body ends: the receiver is written back to its place, and the
  call gives the body's value. -/
  | methodValue {pl : Place} {v w : Expr} :
      IsValue w → Step P (.method pl v w) (.write pl.var pl.fields v w)

/-- A value takes no step. -/
theorem value_no_step {P : Program} {v : Expr} {o : Outcome} (hv : IsValue v) :
    ¬ Step P v o := by
  induction hv generalizing o with
  | unit => intro h; cases h
  | int => intro h; cases h
  | bool => intro h; cases h
  | result _ _ ih =>
    intro h
    cases h with
    | resultArg hs => exact ih hs
  | construct _ ih =>
    intro h
    cases h with
    | constructArg _ hs => exact ih _ (List.mem_append_right _ List.mem_cons_self) hs
  | _ => intro h; cases h

theorem AllValues.mid {vs rest : List Expr} {e : Expr} :
    AllValues (vs ++ e :: rest) → IsValue e := by
  induction vs with
  | nil => intro h; cases h with | cons he _ => exact he
  | cons _ _ ih => intro h; cases h with | cons _ hs => exact ih hs

/-- A call whose arguments are all values steps only into the called
function's body, so a call of the wrong arity takes no step. -/
theorem call_step {P : Program} {name : String} {vs : List Expr} {o : Outcome}
    (hvs : AllValues vs) (h : Step P (.call name vs) o) :
    ∃ f e, P.find? name = some f ∧ f.enter vs = some e ∧
      o = .next (.frame e) := by
  cases h with
  | callArg _ hs => exact absurd hs (value_no_step (AllValues.mid hvs))
  | call hf _ hi => exact ⟨_, _, hf, hi, rfl⟩

/-- Zero or more steps of the semantics, none of them a panic, a
return, a read or a write, and the
**trace** of their effects, in order: `Steps P e tr e'` when `e` runs to
`e'` performing the effects `tr`, each with an answer the semantics
allows. -/
inductive Steps (P : Program) : Expr → List Request → Expr → Prop where
  | refl (e : Expr) : Steps P e [] e
  | next {e e' e'' : Expr} {tr : List Request} :
      Step P e (.next e') → Steps P e' tr e'' → Steps P e tr e''
  | perform {e e'' r : Expr} {req : Request} {k : Expr → Expr} {tr : List Request} :
      Step P e (.perform req k) → req.Allows r → Steps P (k r) tr e'' → Steps P e (req :: tr) e''

end OskaFormal.Core
