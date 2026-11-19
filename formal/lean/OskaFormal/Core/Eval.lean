import OskaFormal.Core.Host

/-!
# Oska Core: the executable semantics

The Lean reference runs a program of Oska Core with `step`, a computable
function defined from the same pieces as the `Step` relation of
`Semantics.lean` (`Expr.subst`, `instantiate`, `evalBinary`, `intResult`;
a call of a closure goes into its body with `instantiate`, as a call of a
function does), and `run`, which repeats it for at most a given number of steps (its
fuel). There is no second copy of the semantics to drift: `step_sound`
proves that every step `step` takes is a `Step`, and `step_complete` that
`step` takes every `Step` there is, so `step P e = some o` exactly when
`Step P e o`. `run_sound` lifts that to whole runs: a value `run` returns
is one `Step`s reach, and so is a panic, and a return. A read or write
of a var no `cell` or running mutating call holds ends the run as
`unbound`, which `preservation` rules out for a closed, typed expression.
-/

namespace OskaFormal.Core

mutual
  /-- Whether an expression is a value: `()`, an integer, a boolean, text,
  a path, a capability, an `IoError`, an `Ok` or `Err` of a value, a
  struct or variant whose fields are values, or a closure of height 0. -/
  def Expr.isValue : Expr → Bool
    | .unit | .int _ | .bool _ => true
    | .str _ | .path _ | .cap _ _ | .ioError => true
    | .result _ v => Expr.isValue v
    | .construct _ _ fs => Expr.allValues fs
    | .closure _ 0 _ _ _ _ => true
    | _ => false

  /-- Whether every expression of a list is a value. -/
  def Expr.allValues : List Expr → Bool
    | [] => true
    | e :: es => e.isValue && Expr.allValues es
end

mutual
  theorem Expr.isValue_sound : ∀ {e : Expr}, e.isValue = true → IsValue e
    | .unit, _ => IsValue.unit
    | .int _, _ => IsValue.int _
    | .bool _, _ => IsValue.bool _
    | .str _, _ => IsValue.str _
    | .path _, _ => IsValue.path _
    | .cap _ _, _ => IsValue.cap _ _
    | .ioError, _ => IsValue.ioError
    | .result _ v, h => IsValue.result _ (Expr.isValue_sound (e := v) (by simpa [Expr.isValue] using h))
    | .construct _ _ fs, h =>
      IsValue.construct (Expr.allValues_sound (es := fs) (by simpa [Expr.isValue] using h)).forall
    | .closure _ 0 _ _ _ _, _ => IsValue.closure _ _ _ _ _
    | .closure _ (_ + 1) _ _ _ _, h => by simp [Expr.isValue] at h
    | .local_ _, h | .neg _, h | .not _, h | .binary _ _ _, h | .call _ _, h | .if_ _ _ _, h
    | .block _ _, h | .field _ _, h | .match_ _ _, h | .builtin _ _, h | .callValue _ _, h
    | .try_ _ _, h | .frame _, h | .var_ _, h | .mutCall _ _ _, h | .cell _ _, h
    | .method _ _ _, h => by simp [Expr.isValue] at h

  theorem Expr.allValues_sound : ∀ {es : List Expr}, Expr.allValues es = true → AllValues es
    | [], _ => AllValues.nil
    | e :: es, h => by
      simp only [Expr.allValues, Bool.and_eq_true] at h
      exact AllValues.cons (Expr.isValue_sound (e := e) h.1) (Expr.allValues_sound (es := es) h.2)
end

theorem Expr.isValue_complete {e : Expr} (h : IsValue e) : e.isValue = true := by
  induction h with
  | unit => rfl
  | int _ => rfl
  | bool _ | str _ | path _ | cap _ _ | ioError | closure _ _ _ _ _ => rfl
  | result _ _ ih => simpa [Expr.isValue] using ih
  | @construct _ _ fs hfs ih =>
    simp only [Expr.isValue]
    clear hfs
    induction fs with
    | nil => rfl
    | cons e es ihs =>
      simp only [Expr.allValues, Bool.and_eq_true]
      exact ⟨ih e List.mem_cons_self, ihs fun e' he' => ih e' (List.mem_cons_of_mem _ he')⟩

theorem Expr.isValue_iff {e : Expr} : e.isValue = true ↔ IsValue e :=
  ⟨Expr.isValue_sound, Expr.isValue_complete⟩

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
  | some f => (f.enter vs).map fun e => .next (.frame e)
  | none => none

/-- The step a mutating call takes once its arguments `vs` are values:
read the receiver at `pl`, then run the method's body with it. -/
def stepMutCall (P : Program) (name : String) (pl : Place) (vs : List Expr) : Option Outcome :=
  match P.find? name with
  | some f =>
    match f.receiver with
    | some _ =>
      (instantiate f.params vs f.body).map fun body => .read pl.var pl.fields fun r => .method pl r body
    | none => none
  | none => none

/-- The step a built-in call takes once its arguments `vs` are values: an
effect on a capability of its receiver's kind, or a pure call's value. -/
def stepBuiltin (op : Builtin) (vs : List Expr) : Option Outcome :=
  match op.receiver, vs with
  | some k, .cap k' r :: rest =>
    if k' = k then some (.perform ⟨op, k, r, rest⟩ fun x => x) else none
  | some _, _ => none
  | none, vs => (evalPure op vs).map .next

/-- The step a call of a function value takes once the callee `c` and the
arguments `vs` are values: into the closure's body, with the arguments put
for its parameters' slots, as `Step.callValue` (and a call of a function,
`stepCall`) puts them, with `instantiate`. -/
def stepCallValue : Expr → List Expr → Option Outcome
  | .closure ps 0 _ _ _ body, vs => (instantiate ps vs body).map fun e => .next (.frame e)
  | _, _ => none

/-- The step of `e?` once `e` is a value: the value inside an `Ok`; the
return of an `Err`; or, with a conversion `f`, an `Err` of `f` applied to
the error, which then returns. -/
def stepTry : Expr → Option String → Option Outcome
  | .result true v, _ => some (.next v)
  | .result false v, none => some (.ret (.result false v))
  | .result false v, some f => some (.next (.try_ (.result false (.call f [v])) none))
  | _, _ => none

/-- The step of a field access once its base is the value `b`: to the
field, when `b` is a struct. -/
def stepField : Expr → Nat → Option Outcome
  | .construct _ none fs, i => (fs[i]?).map .next
  | _, _ => none

/-- The step of a `match` once its scrutinee is the value `v`: skip the
first arm when `v` does not match its pattern, else take its body, or
test its guard first. -/
def stepMatch (v : Expr) : List Arm → Option Outcome
  | [] => none
  | .mk p g body :: rest =>
    match matchPat p v, g with
    | none, _ => some (.next (.match_ v rest))
    | some bs, none => some (.next (Expr.substAll bs body))
    | some bs, some g =>
      some (.next (.if_ (Expr.substAll bs g) (Expr.substAll bs body) (some (.match_ v rest))))

mutual
  /-- One step of a closed expression in program `P`, as a function: the
  outcome `Step P e` relates `e` to, or `none` when `e` takes no step (a
  value, or a stuck expression). -/
  def step (P : Program) : Expr → Option Outcome
    | .unit | .int _ | .bool _ | .local_ _ => none
    | .str _ | .path _ | .cap _ _ | .ioError => none
    | .closure _ _ _ _ _ _ => none
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
    | .construct t var fs =>
      match stepArgs P fs with
      | some (vs, o, rest) => some (o.map fun e' => .construct t var (vs ++ e' :: rest))
      | none => none
    | .field b i =>
      if b.isValue then stepField b i
      else (step P b).map (·.map fun b' => .field b' i)
    | .match_ sc arms =>
      if sc.isValue then stepMatch sc arms
      else (step P sc).map (·.map fun sc' => .match_ sc' arms)
    | .builtin op args =>
      match stepArgs P args with
      | some (vs, o, rest) => some (o.map fun e' => .builtin op (vs ++ e' :: rest))
      | none => if Expr.allValues args then stepBuiltin op args else none
    | .callValue c args =>
      if c.isValue then
        match stepArgs P args with
        | some (vs, o, rest) => some (o.map fun e' => .callValue c (vs ++ e' :: rest))
        | none => if Expr.allValues args then stepCallValue c args else none
      else (step P c).map (·.map fun c' => .callValue c' args)
    | .result b e => (step P e).map (·.map (.result b))
    | .try_ e c =>
      if e.isValue then stepTry e c
      else (step P e).map (·.map fun e' => .try_ e' c)
    | .frame e =>
      if e.isValue then some (.next e)
      else (step P e).map Outcome.inFrame
    | .var_ i => some (.read i [] fun x => x)
    | .mutCall name pl args =>
      match stepArgs P args with
      | some (vs, o, rest) => some (o.map fun e' => .mutCall name pl (vs ++ e' :: rest))
      | none => if Expr.allValues args then stepMutCall P name pl args else none
    | .cell v e =>
      if e.isValue then some (.next e)
      else (step P e).map (Outcome.inCell v)
    | .method pl v e =>
      if e.isValue then some (.write pl.var pl.fields v e)
      else (step P e).map (Outcome.inMethod pl v)

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
    | .var_ t e :: ss, val =>
      if e.isValue then some (.next (.cell e (.block ss val)))
      else (step P e).map (·.map fun e' => .block (.var_ t e' :: ss) val)
    | .assign pl e :: ss, val =>
      if e.isValue then some (.write pl.var pl.fields e (.block ss val))
      else (step P e).map (·.map fun e' => .block (.assign pl e' :: ss) val)
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

theorem stepField_sound {P : Program} {b : Expr} {i : Nat} {o : Outcome} (hb : IsValue b)
    (h : stepField b i = some o) : Step P (.field b i) o := by
  unfold stepField at h
  split at h
  · obtain ⟨v, hv, rfl⟩ := map_eq_some_inv h
    cases hb with
    | construct hfs => exact Step.field (AllValues.of_forall hfs) hv
  · cases h

theorem stepMatch_sound {P : Program} {v : Expr} {arms : List Arm} {o : Outcome} (hv : IsValue v)
    (h : stepMatch v arms = some o) : Step P (.match_ v arms) o := by
  cases arms with
  | nil => cases h
  | cons arm rest =>
    obtain ⟨p, g, body⟩ := arm
    simp only [stepMatch] at h
    split at h <;> cases h
    · next hm => exact Step.matchSkip hv hm
    · next hm => exact Step.matchArm hv hm
    · next hm => exact Step.matchGuard hv hm

theorem stepBuiltin_sound {P : Program} {op : Builtin} {vs : List Expr} {o : Outcome}
    (hvs : AllValues vs) (h : stepBuiltin op vs = some o) : Step P (.builtin op vs) o := by
  unfold stepBuiltin at h
  split at h
  · next k k' r rest hk =>
    split at h
    · next heq =>
      subst heq
      cases h
      cases hvs with
      | cons _ hrest => exact Step.builtinEffect hk hrest
    · cases h
  · cases h
  · obtain ⟨v, hv, rfl⟩ := map_eq_some_inv h
    exact Step.builtinPure hvs hv

theorem stepTry_sound {P : Program} {e : Expr} {c : Option String} {o : Outcome} (he : IsValue e)
    (h : stepTry e c = some o) : Step P (.try_ e c) o := by
  unfold stepTry at h
  split at h <;> cases h
  · cases he with | result _ hv => exact Step.tryOk hv
  · cases he with | result _ hv => exact Step.tryErr hv
  · cases he with | result _ hv => exact Step.tryConvert hv

theorem stepCallValue_sound {P : Program} {c : Expr} {vs : List Expr} {o : Outcome}
    (hvs : AllValues vs) (h : stepCallValue c vs = some o) : Step P (.callValue c vs) o := by
  unfold stepCallValue at h
  split at h
  · obtain ⟨e, hi, rfl⟩ := map_eq_some_inv h
    exact Step.callValue hvs hi
  · cases h

theorem stepMutCall_sound {P : Program} {name : String} {pl : Place} {vs : List Expr}
    {o : Outcome} (hvs : AllValues vs) (h : stepMutCall P name pl vs = some o) :
    Step P (.mutCall name pl vs) o := by
  unfold stepMutCall at h
  split at h
  · next f hf =>
    split at h
    · next u hu =>
      obtain ⟨body, hi, rfl⟩ := map_eq_some_inv h
      exact Step.mutCall hf hu hvs hi
    · cases h
  · cases h

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
  next => intro o h; simp [step] at h
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
  next t var fs vs o' rest hargs ih =>
    intro o h
    simp only [step, hargs, Option.some.injEq] at h
    subst h
    obtain ⟨e, rfl, hvs, hs⟩ := ih _ _ _ hargs
    exact Step.constructArg hvs hs
  next t var fs hnone _ =>
    intro o h
    simp [step, hnone] at h
  next b i hb =>
    intro o h
    simp only [step, hb, ite_true] at h
    exact stepField_sound (Expr.isValue_iff.mp hb) h
  next b i hb ih =>
    intro o h
    simp only [step, hb, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.fieldBase (ih hs)
  next sc arms hsc =>
    intro o h
    simp only [step, hsc, ite_true] at h
    exact stepMatch_sound (Expr.isValue_iff.mp hsc) h
  next sc arms hsc ih =>
    intro o h
    simp only [step, hsc, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.matchScrutinee (ih hs)
  next op args vs o' rest hargs ih =>
    intro o h
    simp only [step, hargs, Option.some.injEq] at h
    subst h
    obtain ⟨e, rfl, hvs, hs⟩ := ih _ _ _ hargs
    exact Step.builtinArg hvs hs
  next op args hnone hall _ =>
    intro o h
    simp only [step, hnone, hall, ite_true] at h
    exact stepBuiltin_sound (Expr.allValues_iff.mp hall) h
  next op args hnone hall _ =>
    intro o h
    simp [step, hnone, hall] at h
  next c args hc vs o' rest hargs ih =>
    intro o h
    simp only [step, hc, ite_true, hargs, Option.some.injEq] at h
    subst h
    obtain ⟨e, rfl, hvs, hs⟩ := ih _ _ _ hargs
    exact Step.callValueArg (Expr.isValue_iff.mp hc) hvs hs
  next c args hc hnone hall _ =>
    intro o h
    simp only [step, hc, ite_true, hnone, hall] at h
    exact stepCallValue_sound (Expr.allValues_iff.mp hall) h
  next c args hc hnone hall _ =>
    intro o h
    simp [step, hc, hnone, hall] at h
  next c args hc ih =>
    intro o h
    simp only [step, hc, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.callValueCallee (ih hs)
  next b e ih =>
    intro o h
    simp only [step] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.resultArg (ih hs)
  next e c he =>
    intro o h
    simp only [step, he, ite_true] at h
    exact stepTry_sound (Expr.isValue_iff.mp he) h
  next e c he ih =>
    intro o h
    simp only [step, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.tryArg (ih hs)
  next e he =>
    intro o h
    simp only [step, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.frameValue (Expr.isValue_iff.mp he)
  next e he ih =>
    intro o h
    simp only [step, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.frame (ih hs)
  next i => intro o h; simp [step] at h; subst h; exact Step.var
  next name pl args vs o' rest hargs ih =>
    intro o h
    simp only [step, hargs, Option.some.injEq] at h
    subst h
    obtain ⟨e, rfl, hvs, hs⟩ := ih _ _ _ hargs
    exact Step.mutCallArg hvs hs
  next name pl args hnone hall _ =>
    intro o h
    simp only [step, hnone, hall, ite_true] at h
    exact stepMutCall_sound (Expr.allValues_iff.mp hall) h
  next name pl args hnone hall _ =>
    intro o h
    simp [step, hnone, hall] at h
  next v e he =>
    intro o h
    simp only [step, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.cellValue (Expr.isValue_iff.mp he)
  next v e he ih =>
    intro o h
    simp only [step, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.cellBody (ih hs)
  next pl v e he =>
    intro o h
    simp only [step, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.methodValue (Expr.isValue_iff.mp he)
  next pl v e he ih =>
    intro o h
    simp only [step, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.methodBody (ih hs)
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
  next t e ss val he o h =>
    simp only [stepBlock, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.blockVar (Expr.isValue_iff.mp he)
  next t e ss val he ih o h =>
    simp only [stepBlock, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.blockVarArg (ih hs)
  next pl e ss val he o h =>
    simp only [stepBlock, he, ite_true, Option.some.injEq] at h
    subst h
    exact Step.blockAssign (Expr.isValue_iff.mp he)
  next pl e ss val he ih o h =>
    simp only [stepBlock, he, ite_false, Bool.false_eq_true] at h
    obtain ⟨o', hs, rfl⟩ := map_eq_some_inv h
    exact Step.blockAssignArg (ih hs)

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

/-- A pure built-in call is no effect: it has no receiver. -/
theorem evalPure_receiver {op : Builtin} {vs : List Expr} {v : Expr}
    (h : evalPure op vs = some v) : op.receiver = none := by
  cases op <;> first | rfl | (unfold evalPure at h; split at h <;> simp_all)

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
  | constructArg hvs hs ih => simp [step, stepArgs_mid hvs hs.not_value ih]
  | fieldBase hs ih => simp [step, hs.not_value, ih]
  | @field type fs _ _ hvs he =>
    have hv : (Expr.construct type none fs).isValue = true :=
      Expr.isValue_iff.mpr (IsValue.construct hvs.forall)
    simp [step, hv, stepField, he]
  | matchScrutinee hs ih => simp [step, hs.not_value, ih]
  | matchSkip hv hm => simp [step, Expr.isValue_iff.mpr hv, stepMatch, hm]
  | matchArm hv hm => simp [step, Expr.isValue_iff.mpr hv, stepMatch, hm]
  | matchGuard hv hm => simp [step, Expr.isValue_iff.mpr hv, stepMatch, hm]
  | builtinArg hvs hs ih => simp [step, stepArgs_mid hvs hs.not_value ih]
  | builtinPure hvs he =>
    simp [step, stepArgs_values hvs, Expr.allValues_iff.mpr hvs, stepBuiltin,
      evalPure_receiver he, he]
  | @builtinEffect op k r vs hk hvs =>
    have hall := AllValues.cons (IsValue.cap k r) hvs
    simp [step, stepArgs_values hall, Expr.allValues_iff.mpr hall, stepBuiltin, hk]
  | callValueCallee hs ih => simp [step, hs.not_value, ih]
  | callValueArg hc hvs hs ih =>
    simp [step, Expr.isValue_iff.mpr hc, stepArgs_mid hvs hs.not_value ih]
  | callValue hvs hi =>
    simp [step, Expr.isValue, stepArgs_values hvs, Expr.allValues_iff.mpr hvs, stepCallValue, hi]
  | resultArg _ ih => simp [step, ih]
  | tryArg hs ih => simp [step, hs.not_value, ih]
  | tryOk hv | tryErr hv | tryConvert hv =>
    simp [step, stepTry, Expr.isValue, Expr.isValue_iff.mpr hv]
  | frame hs ih => simp [step, hs.not_value, ih]
  | frameValue hv => simp [step, Expr.isValue_iff.mpr hv]
  | var => rfl
  | blockVarArg hs ih => simp [step, stepBlock, hs.not_value, ih]
  | blockVar hv => simp [step, stepBlock, Expr.isValue_iff.mpr hv]
  | blockAssignArg hs ih => simp [step, stepBlock, hs.not_value, ih]
  | blockAssign hv => simp [step, stepBlock, Expr.isValue_iff.mpr hv]
  | cellBody hs ih => simp [step, hs.not_value, ih]
  | cellValue hv => simp [step, Expr.isValue_iff.mpr hv]
  | mutCallArg hvs hs ih => simp [step, stepArgs_mid hvs hs.not_value ih]
  | mutCall hf hr hvs hi =>
    simp [step, stepArgs_values hvs, Expr.allValues_iff.mpr hvs, stepMutCall, hf, hr, hi]
  | methodBody hs ih => simp [step, hs.not_value, ih]
  | methodValue hv => simp [step, Expr.isValue_iff.mpr hv]

/-- The capabilities `main` receives: for each parameter, the capability
its type names, rooted anywhere, as the runtime grants them
(`Capability::granted`). -/
def grants (ps : List Param) : List Expr :=
  ps.map fun p => match p.type with
    | .cap k => .cap k .anywhere
    | _ => .unit

/-- How a run of the Lean reference ends. -/
inductive RunResult where
  /-- The expression reached a value. -/
  | value (v : Expr)
  /-- A step panicked. -/
  | panic (kind : PanicKind)
  /-- A step returned `v` from outside any function (`?` on an `Err`
  where no call is running). A run from a call never ends so: the call
  runs its body in a `frame`, which takes the return. -/
  | returned (v : Expr)
  /-- The expression reached is not a value and takes no step. In a
  well-typed program progress rules this out. -/
  | stuck (e : Expr)
  /-- A step read or wrote a var that no cell holds: a `var` read
  outside its block. In a well-typed program, which starts with no var in
  scope, preservation rules this out. -/
  | unbound (e : Expr)
  /-- The fuel ran out before the run ended. -/
  | outOfFuel

/-- Run `e` in program `P` on the fake host `h` with `step`, for at most
`fuel` steps: each effect is answered by `Host.perform`. How the run
ended, and the host after it. -/
def run (P : Program) : Nat → Host → Expr → RunResult × Host
  | 0, h, e => (if e.isValue then .value e else .outOfFuel, h)
  | fuel + 1, h, e =>
    if e.isValue then (.value e, h)
    else
      match step P e with
      | some (.next e') => run P fuel h e'
      | some (.panic k) => (.panic k, h)
      | some (.perform req k) => run P fuel (h.perform req).2 (k (h.perform req).1)
      | some (.ret v) => (.returned v, h)
      | some (.read _ _ _) | some (.write _ _ _ _) => (.unbound e, h)
      | none => (.stuck e, h)

/-- Every effect a step performs is on a capability of its receiver's
kind. -/
def Outcome.WellFormed : Outcome → Prop
  | .perform req _ => req.op.receiver = some req.kind
  | _ => True

theorem intResult_wellFormed (n : Int) : (intResult n).WellFormed := by
  unfold intResult; split <;> trivial

theorem evalBinary_wellFormed {op : BinOp} {l r : Expr} {o : Outcome}
    (h : evalBinary op l r = some o) : o.WellFormed := by
  unfold evalBinary at h
  split at h <;> simp only [Option.some.injEq, reduceCtorEq] at h <;> subst h <;>
    first | trivial | exact intResult_wellFormed _ | (split <;> first | trivial | exact intResult_wellFormed _)

theorem Step.wellFormed {P : Program} {e : Expr} {o : Outcome} (h : Step P e o) :
    o.WellFormed := by
  have map : ∀ {o : Outcome} {f : Expr → Expr}, o.WellFormed → (o.map f).WellFormed := by
    intro o f h; cases o <;> simp_all [Outcome.map, Outcome.WellFormed]
  have inFrame : ∀ {o : Outcome}, o.WellFormed → o.inFrame.WellFormed := by
    intro o h; cases o <;> simp_all [Outcome.inFrame, Outcome.map, Outcome.WellFormed]
  have under : ∀ {o : Outcome} {v : Expr} {wrap : Expr → Expr → Expr} {onRet : Expr → Outcome},
      (∀ r, (onRet r).WellFormed) → o.WellFormed → (o.under v wrap onRet).WellFormed := by
    intro o v wrap onRet hr h
    cases o with
    | ret r => exact hr r
    | perform req k => exact h
    | read i _ _ => cases i <;> trivial
    | write i _ _ _ => cases i <;> trivial
    | _ => trivial
  have inCell : ∀ {o : Outcome} {v : Expr}, o.WellFormed → (o.inCell v).WellFormed :=
    fun h => under (fun _ => trivial) h
  have inMethod : ∀ {o : Outcome} {pl : Place} {v : Expr}, o.WellFormed →
      (o.inMethod pl v).WellFormed :=
    fun h => under (fun _ => trivial) h
  induction h <;> first | trivial | (exact map ‹_›) | (exact inFrame ‹_›) | assumption |
    exact intResult_wellFormed _ | exact evalBinary_wellFormed ‹_› | (exact inCell ‹_›) |
    (exact inMethod ‹_›)

/-- **Soundness of `run`.** A value the Lean reference reaches is one the
semantics reaches, with the trace of the effects the fake host answered;
a panic it reports is a step of the semantics from an expression the
semantics reaches, and so is a return; and an expression it reports
stuck is one the semantics reaches and that takes no step of the
semantics. -/
theorem run_sound {P : Program} : ∀ {fuel : Nat} {h : Host} {e : Expr},
    (∀ {v h'}, run P fuel h e = (.value v, h') → ∃ tr, Steps P e tr v ∧ IsValue v) ∧
    (∀ {k h'}, run P fuel h e = (.panic k, h') →
      ∃ tr e', Steps P e tr e' ∧ Step P e' (.panic k)) ∧
    (∀ {e' h'}, run P fuel h e = (.stuck e', h') →
      ∃ tr, Steps P e tr e' ∧ ¬ IsValue e' ∧ ∀ o, ¬ Step P e' o) ∧
    (∀ {v h'}, run P fuel h e = (.returned v, h') →
      ∃ tr e', Steps P e tr e' ∧ Step P e' (.ret v)) := by
  intro fuel
  induction fuel with
  | zero =>
    intro h e
    refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;> simp only [run] at hr <;>
      split at hr <;> simp at hr
    obtain ⟨rfl, -⟩ := hr
    exact ⟨[], Steps.refl _, Expr.isValue_iff.mp ‹_›⟩
  | succ fuel ih =>
    intro h e
    by_cases hv : e.isValue = true
    · refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
        simp only [run, hv, ite_true] at hr <;> simp at hr
      obtain ⟨rfl, -⟩ := hr
      exact ⟨[], Steps.refl _, Expr.isValue_iff.mp hv⟩
    · have hn : e.isValue = false := by simpa using hv
      cases hs : step P e with
      | none =>
        refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
          simp only [run, hn, Bool.false_eq_true, ite_false, hs] at hr <;> simp at hr
        obtain ⟨rfl, -⟩ := hr
        refine ⟨[], Steps.refl _, fun h => by simp [Expr.isValue_iff.mpr h] at hn, fun o ho => ?_⟩
        simp [step_complete ho] at hs
      | some o =>
        cases o with
        | next e' =>
          have h1 := step_sound hs
          obtain ⟨iv, ip, is, ir⟩ := ih (h := h) (e := e')
          simp only [run, hn, Bool.false_eq_true, ite_false, hs]
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩
          · obtain ⟨tr, h2, h3⟩ := iv hr
            exact ⟨tr, Steps.next h1 h2, h3⟩
          · obtain ⟨tr, x, h2, h3⟩ := ip hr
            exact ⟨tr, x, Steps.next h1 h2, h3⟩
          · obtain ⟨tr, h2, h3⟩ := is hr
            exact ⟨tr, Steps.next h1 h2, h3⟩
          · obtain ⟨tr, x, h2, h3⟩ := ir hr
            exact ⟨tr, x, Steps.next h1 h2, h3⟩
        | panic k =>
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
            simp only [run, hn, Bool.false_eq_true, ite_false, hs] at hr <;> simp at hr
          obtain ⟨rfl, -⟩ := hr
          exact ⟨[], e, Steps.refl _, step_sound hs⟩
        | ret v =>
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
            simp only [run, hn, Bool.false_eq_true, ite_false, hs] at hr <;> simp at hr
          obtain ⟨rfl, -⟩ := hr
          exact ⟨[], e, Steps.refl _, step_sound hs⟩
        | read i is k =>
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
            simp only [run, hn, Bool.false_eq_true, ite_false, hs] at hr <;> simp at hr
        | write i is w n =>
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩ <;>
            simp only [run, hn, Bool.false_eq_true, ite_false, hs] at hr <;> simp at hr
        | perform req k =>
          have h1 := step_sound hs
          have hal := Host.perform_allows h h1.wellFormed
          obtain ⟨iv, ip, is, ir⟩ := ih (h := (h.perform req).2) (e := k (h.perform req).1)
          simp only [run, hn, Bool.false_eq_true, ite_false, hs]
          refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_, fun hr => ?_⟩
          · obtain ⟨tr, h2, h3⟩ := iv hr
            exact ⟨_, Steps.perform h1 hal h2, h3⟩
          · obtain ⟨tr, x, h2, h3⟩ := ip hr
            exact ⟨_, x, Steps.perform h1 hal h2, h3⟩
          · obtain ⟨tr, h2, h3⟩ := is hr
            exact ⟨_, Steps.perform h1 hal h2, h3⟩
          · obtain ⟨tr, x, h2, h3⟩ := ir hr
            exact ⟨_, x, Steps.perform h1 hal h2, h3⟩

end OskaFormal.Core
