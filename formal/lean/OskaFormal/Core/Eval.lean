import OskaFormal.Core.Semantics

/-!
# Oska Core: the executable semantics

The Lean reference runs a program of Oska Core with `step`, a computable
function defined from the same pieces as the `Step` relation of
`Semantics.lean` (`Expr.subst`, `instantiate`, `evalBinary`, `intResult`),
and `run`, which repeats it for at most a given number of steps (its
fuel). There is no second copy of the semantics to drift: `step_sound`
proves that every step `step` takes is a `Step`, and `step_complete` that
`step` takes every `Step` there is, so `step P e = some o` exactly when
`Step P e o`. `run_sound` lifts that to whole runs: a value `run` returns
is one `Step`s reach, and so is a panic.
-/

namespace OskaFormal.Core

/-- Whether an expression is a value: `()`, an integer or a boolean. -/
def Expr.isValue : Expr → Bool
  | .unit | .int _ | .bool _ => true
  | _ => false

theorem Expr.isValue_iff {e : Expr} : e.isValue = true ↔ IsValue e := by
  constructor
  · intro h
    cases e <;> simp [Expr.isValue] at h <;> constructor
  · intro h
    cases h <;> rfl

/-- Whether every expression of a list is a value. -/
def Expr.allValues : List Expr → Bool
  | [] => true
  | e :: es => e.isValue && Expr.allValues es

theorem Expr.allValues_iff {es : List Expr} : Expr.allValues es = true ↔ AllValues es := by
  induction es with
  | nil => exact ⟨fun _ => AllValues.nil, fun _ => rfl⟩
  | cons e es ih =>
    simp only [Expr.allValues, Bool.and_eq_true]
    constructor
    · intro ⟨h₁, h₂⟩
      exact AllValues.cons (Expr.isValue_iff.mp h₁) (ih.mp h₂)
    · intro h
      cases h with
      | cons hv hs => exact ⟨Expr.isValue_iff.mpr hv, ih.mpr hs⟩

/-- A step of a logical operator (`&&`, `||`, `implies`) whose `lhs` is
the value `l`; `none` when `l` is not a boolean. -/
def stepLogical : BinOp → Expr → Expr → Option Outcome
  | .and, .bool false, _ => some (.next (.bool false))
  | .and, .bool true, r => some (.next r)
  | .or, .bool true, _ => some (.next (.bool true))
  | .or, .bool false, r => some (.next r)
  | .implies, .bool false, _ => some (.next (.bool true))
  | .implies, .bool true, r => some (.next r)
  | _, _, _ => none

/-- The step a call takes once its arguments `vs` are values: into the
body of the function it names, with the arguments put for its slots. -/
def stepCall (P : Program) (name : String) (vs : List Expr) : Option Outcome :=
  match P.find? name with
  | some f => (instantiate f.params vs f.body).map .next
  | none => none

mutual
  /-- One step of a closed expression in program `P`, as a function: the
  outcome `Step P e` relates `e` to, or `none` when `e` takes no step (a
  value, or a stuck expression). -/
  def step (P : Program) : Expr → Option Outcome
    | .unit | .int _ | .bool _ | .local_ _ => none
    | .neg e =>
      match e with
      | .int n => some (intResult (-n))
      | e => (step P e).map (·.map .neg)
    | .not e =>
      match e with
      | .bool b => some (.next (.bool (!b)))
      | e => (step P e).map (·.map .not)
    | .binary op l r =>
      if l.isValue then
        if op.isLogical then stepLogical op l r
        else if r.isValue then evalBinary op l r
        else (step P r).map (·.map fun r' => .binary op l r')
      else (step P l).map (·.map fun l' => .binary op l' r)
    | .call name args =>
      match stepArgs P args with
      | some (vs, o, rest) => some (o.map fun e' => .call name (vs ++ e' :: rest))
      | none => if Expr.allValues args then stepCall P name args else none
    | .if_ c th el =>
      match c, el with
      | .bool true, some _ => some (.next th)
      | .bool false, some el => some (.next el)
      | .bool true, none => some (.next (.block [.expr th] none))
      | .bool false, none => some (.next .unit)
      | c, el => (step P c).map (·.map fun c' => .if_ c' th el)
    | .block ss val => stepBlock P ss val

  /-- The step of the first argument of a list that is not a value: the
  values before it, its outcome, and the arguments after it. `none` when
  every argument is a value, or the first that is not takes no step. -/
  def stepArgs (P : Program) : List Expr → Option (List Expr × Outcome × List Expr)
    | [] => none
    | e :: es =>
      if e.isValue then (stepArgs P es).map fun (vs, o, rest) => (e :: vs, o, rest)
      else (step P e).map fun o => ([], o, es)

  /-- One step of a block with statements `ss` and value `val`. -/
  def stepBlock (P : Program) : List Stmt → Option Expr → Option Outcome
    | [], none => some (.next .unit)
    | [], some e => some (.next e)
    | .expr e :: ss, val =>
      if e.isValue then some (.next (.block ss val))
      else (step P e).map (·.map fun e' => .block (.expr e' :: ss) val)
    | .let_ b t e :: ss, val =>
      if e.isValue then
        if b then some (.next (.block (Expr.substStmts e ss) (Expr.substOpt e val)))
        else some (.next (.block ss val))
      else (step P e).map (·.map fun e' => .block (.let_ b t e' :: ss) val)
end

theorem stepLogical_sound {P : Program} {op : BinOp} {l r : Expr} {o : Outcome}
    (h : stepLogical op l r = some o) : Step P (.binary op l r) o := by
  unfold stepLogical at h
  split at h <;> cases h
  · exact Step.andFalse
  · exact Step.andTrue
  · exact Step.orTrue
  · exact Step.orFalse
  · exact Step.impliesFalse
  · exact Step.impliesTrue

private theorem map_eq_some_inv {α β : Type} {f : α → β} {x : Option α} {b : β}
    (h : x.map f = some b) : ∃ a, x = some a ∧ f a = b := by
  cases x with
  | none => cases h
  | some a => exact ⟨a, rfl, Option.some.inj h⟩

/-- **Soundness of `step`.** Every step `step` takes is a step of the
semantics. -/
theorem step_sound {P : Program} : ∀ {e : Expr} {o : Outcome}, step P e = some o → Step P e o := by
  intro e
  induction e using step.induct P
    (motive_2 := fun ss val => ∀ o, stepBlock P ss val = some o → Step P (.block ss val) o)
    (motive_3 := fun es => ∀ vs o rest, stepArgs P es = some (vs, o, rest) →
      ∃ e, es = vs ++ e :: rest ∧ AllValues vs ∧ Step P e o)
  next => intro o h; simp [step] at h
  next => intro o h; simp [step] at h
  next => intro o h; simp [step] at h
  next => intro o h; simp [step] at h
  next n => intro o h; simp [step] at h; subst h; exact Step.neg
  next e hne ih =>
    intro o h
    rw [step] at h
    · obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
      exact Step.negArg (ih hs)
    · exact hne
  next b => intro o h; simp [step] at h; subst h; exact Step.not
  next e hne ih =>
    intro o h
    rw [step] at h
    · obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
      exact Step.notArg (ih hs)
    · exact hne
  next op l r hl hop =>
    intro o h
    simp only [step, hl, hop, ite_true] at h
    exact stepLogical_sound h
  next op l r hl hop hr =>
    intro o h
    simp only [step, hl, hop, hr, ite_true, ite_false, Bool.false_eq_true] at h
    exact Step.binary (Expr.isValue_iff.mp hl) (Expr.isValue_iff.mp hr) h
  next op l r hl hop hr ih =>
    intro o h
    simp only [step, hl, hop, hr, ite_true, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.binaryRhs (by simpa using hop) (Expr.isValue_iff.mp hl) (ih hs)
  next op l r hl ih =>
    intro o h
    simp only [step, hl, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.binaryLhs (ih hs)
  next name args vs o' rest hargs ih =>
    intro o h
    simp only [step, hargs, Option.some.injEq] at h
    subst h
    obtain ⟨e, rfl, hvs, hs⟩ := ih _ _ _ hargs
    exact Step.callArg hvs hs
  next name args hnone hall _ =>
    intro o h
    simp only [step, hnone, hall, ite_true, stepCall] at h
    split at h
    · next f hf =>
      obtain ⟨body, hi, rfl⟩ := map_eq_some_inv h
      exact Step.call hf (Expr.allValues_iff.mp hall) hi
    · cases h
  next name args hnone hall _ =>
    intro o h
    simp [step, hnone, hall] at h
  next th val => intro o h; simp [step] at h; subst h; exact Step.ifTrue
  next th el => intro o h; simp [step] at h; subst h; exact Step.ifFalse
  next th => intro o h; simp [step] at h; subst h; exact Step.ifTrueNoElse
  next th => intro o h; simp [step] at h; subst h; exact Step.ifFalseNoElse
  next th c el h₁ h₂ h₃ h₄ ih =>
    intro o h
    rw [step] at h
    · obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
      exact Step.ifCond (ih hs)
    all_goals assumption
  next ss val ih =>
    intro o h
    rw [step] at h
    exact ih _ h
  next vs o rest h => simp [stepArgs] at h
  next e es he ih vs o rest h =>
    simp only [stepArgs, he, ite_true] at h
    obtain ⟨⟨vs', o', rest'⟩, hs, heq⟩ := map_eq_some_inv h
    simp only [Prod.mk.injEq] at heq
    obtain ⟨rfl, rfl, rfl⟩ := heq
    obtain ⟨x, rfl, hvs, hx⟩ := ih _ _ _ hs
    exact ⟨x, rfl, AllValues.cons (Expr.isValue_iff.mp he) hvs, hx⟩
  next e es he ih vs o rest h =>
    simp only [stepArgs, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, heq⟩ := map_eq_some_inv h
    simp only [Prod.mk.injEq] at heq
    obtain ⟨rfl, rfl, rfl⟩ := heq
    exact ⟨e, rfl, AllValues.nil, ih hs⟩
  next o h => simp [stepBlock] at h; subst h; exact Step.blockNone
  next e o h => simp [stepBlock] at h; subst h; exact Step.blockSome
  next e ss val he o h =>
    simp only [stepBlock, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.blockExpr (Expr.isValue_iff.mp he)
  next e ss val he ih o h =>
    simp only [stepBlock, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.blockExprArg (ih hs)
  next t e ss val he o h =>
    simp only [stepBlock, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.blockLetBind (Expr.isValue_iff.mp he)
  next b t e ss val he hb o h =>
    simp only [Bool.not_eq_true] at hb
    subst hb
    simp only [stepBlock, he, ite_true, Option.some.injEq, Bool.false_eq_true, ite_false] at h
    subst h
    exact Step.blockLetDrop (Expr.isValue_iff.mp he)
  next b t e ss val he ih o h =>
    simp only [stepBlock, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.blockLetArg (ih hs)

/-- An expression that steps is not a value. -/
theorem Step.not_value {P : Program} {e : Expr} {o : Outcome} (h : Step P e o) :
    e.isValue = false := by
  cases he : e.isValue
  · rfl
  · exact absurd h (value_no_step (Expr.isValue_iff.mp he))

theorem evalBinary_not_logical {op : BinOp} {l r : Expr} {o : Outcome}
    (h : evalBinary op l r = some o) : op.isLogical = false := by
  cases op <;> first | rfl | (simp [evalBinary] at h)

theorem stepArgs_values {P : Program} {vs : List Expr} (h : AllValues vs) : stepArgs P vs = none := by
  induction h with
  | nil => rfl
  | cons hv _ ih => simp [stepArgs, Expr.isValue_iff.mpr hv, ih]

theorem stepArgs_mid {P : Program} {vs rest : List Expr} {e : Expr} {o : Outcome}
    (hvs : AllValues vs) (he : e.isValue = false) (hs : step P e = some o) :
    stepArgs P (vs ++ e :: rest) = some (vs, o, rest) := by
  induction hvs with
  | nil => simp [stepArgs, he, hs]
  | cons hv _ ih => simp [stepArgs, Expr.isValue_iff.mpr hv, ih]

/-- **Completeness of `step`.** `step` takes every step of the semantics,
so with `step_sound`, `step P e = some o` exactly when `Step P e o`: the
Lean reference runs the semantics the proofs are about. -/
theorem step_complete {P : Program} {e : Expr} {o : Outcome} (h : Step P e o) :
    step P e = some o := by
  induction h with
  | negArg hs ih =>
    rw [step, ih]
    · rfl
    · intro n hn; subst hn; cases hs
  | neg => rfl
  | notArg hs ih =>
    rw [step, ih]
    · rfl
    · intro b hb; subst hb; cases hs
  | not => rfl
  | binaryLhs hs ih => simp [step, hs.not_value, ih]
  | binaryRhs hop hl hs ih =>
    simp [step, Expr.isValue_iff.mpr hl, hop, hs.not_value, ih]
  | binary hl hr he =>
    simp [step, Expr.isValue_iff.mpr hl, Expr.isValue_iff.mpr hr, evalBinary_not_logical he, he]
  | andFalse => rfl
  | andTrue => rfl
  | orTrue => rfl
  | orFalse => rfl
  | impliesFalse => rfl
  | impliesTrue => rfl
  | callArg hvs hs ih => simp [step, stepArgs_mid hvs hs.not_value ih]
  | call hf hvs hi =>
    simp [step, stepArgs_values hvs, Expr.allValues_iff.mpr hvs, stepCall, hf, hi]
  | ifCond hs ih =>
    rw [step, ih]
    · rfl
    all_goals (intros; subst_vars; cases hs)
  | ifTrue => rfl
  | ifFalse => rfl
  | ifTrueNoElse => rfl
  | ifFalseNoElse => rfl
  | blockNone => rfl
  | blockSome => rfl
  | blockExprArg hs ih => simp [step, stepBlock, hs.not_value, ih]
  | blockExpr hv => simp [step, stepBlock, Expr.isValue_iff.mpr hv]
  | blockLetArg hs ih => simp [step, stepBlock, hs.not_value, ih]
  | blockLetBind hv => simp [step, stepBlock, Expr.isValue_iff.mpr hv]
  | blockLetDrop hv => simp [step, stepBlock, Expr.isValue_iff.mpr hv]

/-- Zero or more steps of the semantics that each lead to a next
expression. -/
inductive Steps (P : Program) : Expr → Expr → Prop where
  | refl (e : Expr) : Steps P e e
  | next {e e' e'' : Expr} : Step P e (.next e') → Steps P e' e'' → Steps P e e''

/-- How a run of the Lean reference ends. -/
inductive RunResult where
  /-- The expression reached a value. -/
  | value (v : Expr)
  /-- A step panicked. -/
  | panic (kind : PanicKind)
  /-- The expression reached is not a value and takes no step. In a
  well-typed program progress rules this out. -/
  | stuck (e : Expr)
  /-- The fuel ran out before the run ended. -/
  | outOfFuel

/-- Run `e` in program `P` with `step`, for at most `fuel` steps. -/
def run (P : Program) : Nat → Expr → RunResult
  | 0, e => if e.isValue then .value e else .outOfFuel
  | fuel + 1, e =>
    if e.isValue then .value e
    else
      match step P e with
      | some (.next e') => run P fuel e'
      | some (.panic k) => .panic k
      | none => .stuck e

/-- **Soundness of `run`.** A value the Lean reference reaches is one the
semantics reaches, a panic it reports is a step of the semantics from an
expression the semantics reaches, and an expression it reports stuck is
one the semantics reaches and that takes no step of the semantics. -/
theorem run_sound {P : Program} : ∀ {fuel : Nat} {e : Expr},
    (∀ {v}, run P fuel e = .value v → Steps P e v ∧ IsValue v) ∧
    (∀ {k}, run P fuel e = .panic k → ∃ e', Steps P e e' ∧ Step P e' (.panic k)) ∧
    (∀ {e'}, run P fuel e = .stuck e' →
      Steps P e e' ∧ ¬ IsValue e' ∧ ∀ o, ¬ Step P e' o) := by
  intro fuel
  induction fuel with
  | zero =>
    intro e
    refine ⟨fun h => ?_, fun h => ?_, fun h => ?_⟩ <;> simp only [run] at h <;> split at h <;>
      cases h
    exact ⟨Steps.refl _, Expr.isValue_iff.mp ‹_›⟩
  | succ fuel ih =>
    intro e
    by_cases hv : e.isValue = true
    · refine ⟨fun h => ?_, fun h => ?_, fun h => ?_⟩ <;> simp only [run, hv, ite_true] at h <;>
        cases h
      exact ⟨Steps.refl _, Expr.isValue_iff.mp hv⟩
    · have hn : e.isValue = false := by simpa using hv
      cases hs : step P e with
      | none =>
        refine ⟨fun h => ?_, fun h => ?_, fun h => ?_⟩ <;>
          simp only [run, hn, Bool.false_eq_true, ite_false, hs] at h <;> cases h
        refine ⟨Steps.refl _, fun h => by simp [Expr.isValue_iff.mpr h] at hn, fun o ho => ?_⟩
        simp [step_complete ho] at hs
      | some o =>
        cases o with
        | next e' =>
          have h1 := step_sound hs
          obtain ⟨iv, ip, is⟩ := ih (e := e')
          simp only [run, hn, Bool.false_eq_true, ite_false, hs]
          refine ⟨fun h => ?_, fun h => ?_, fun h => ?_⟩
          · obtain ⟨h2, h3⟩ := iv h
            exact ⟨Steps.next h1 h2, h3⟩
          · obtain ⟨x, h2, h3⟩ := ip h
            exact ⟨x, Steps.next h1 h2, h3⟩
          · obtain ⟨h2, h3⟩ := is h
            exact ⟨Steps.next h1 h2, h3⟩
        | panic k =>
          refine ⟨fun h => ?_, fun h => ?_, fun h => ?_⟩ <;>
            simp only [run, hn, Bool.false_eq_true, ite_false, hs] at h <;> cases h
          exact ⟨e, Steps.refl _, step_sound hs⟩

end OskaFormal.Core
