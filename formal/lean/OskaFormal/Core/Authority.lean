import OskaFormal.Core.Soundness
import OskaFormal.Core.Eval

/-!
# Oska Core: authority safety

**Authority safety** (ADR 0018): running a function performs only
effects of capability kinds in its authority.

A function's **authority** is computed statically from its parameter
types, as the glossary defines it: the capabilities it can reach through
its capability-bearing parameters. The kinds its parameters name are what
the checker's authority report lists (`crates/oska-check/src/
authority.rs`); a capability of one kind reaches what its attenuations
reach (ADR 0008: `Fs` > `FsRead` > `DirRead`, and so on), so `authority`
is the kinds those parameters name, each with every kind below it
(`CapKind.below`). In Oska Core no struct or enum holds a capability or a
function (the export refuses both), so a parameter is
capability-bearing when its type is a capability or a `cap fn`; a
mutating method's receiver is a parameter too, its first
(`Function.allParams`), and a struct or enum never bears one. A plain
`fn` is not: its typing lets it capture no capability-bearing value. A
`cap fn` may have captured capabilities, and its type, like the checker's
authority report, does not say which. So a call's bound is `f`'s
authority and every kind below a capability that a `cap fn` among its
arguments captured (`callAuthority`).

- `authority_safety`: in a program as the checker hands it over, every
  effect in the trace of any run of a call of `f` on arguments of its
  parameters' types, whose `cap fn` arguments hold only capabilities of
  kinds in `A` (`FnArgsWithin`), has a kind in `callAuthority f A`:
  `authority f` or below a kind in `A`, whatever answers the host gives
  that the semantics allows.
- `authority_safety_ofArgs`: the same with `A` computed from the
  arguments, the kinds of the capabilities their `cap fn`s hold
  (`capFnKinds`). With no `cap fn` parameter the bound is `authority f`
  (`authority_safety_noCapFn`).
- `authority_safety_reference`: so does every effect the Lean reference
  performs, on its fake host.
- `attenuation_never_widens`: attenuation gives a capability of a kind
  below its receiver's, whose root reaches no path the receiver's does
  not, in the semantics (`read_only`, `write_only`, and any answer to
  `restrict_to` the semantics allows) and on the fake host.
- The examples at the end show the theorem is not vacuous: a function
  without `Stdout` among its parameters performs no stdout effect, and
  one with it does perform one; a closure that captured `Stdout` prints
  when `apply` calls it as a `cap fn`, it has no plain `fn` type, and
  `applyPure`, which calls the plain `fn` it is given, performs no effect
  at all.

The proof needs no typing beyond the arguments': it follows the
capability values in the running expression. A source body holds none,
so every capability a run holds is an argument or an attenuation of one,
and an effect is labelled with the kind of a capability the run holds. A
closure holds the capabilities it captured in its body, so they count
where the closure goes, and a call of it runs that body.
-/

namespace OskaFormal.Core

/-! ## Authority -/

/-- The capability kinds a function's parameters (its receiver
included) name: what the checker's authority report lists for it. -/
def paramKinds (f : Function) : List CapKind :=
  f.allParams.filterMap fun p => p.type.capKind?

/-- **Authority** of a function: the capability kinds it can reach
through its capability-bearing parameters, each kind they name with every
kind it attenuates to. -/
def authority (f : Function) : List CapKind :=
  (paramKinds f).flatMap CapKind.below

/-- A set of kinds that holds every kind below each of its kinds. -/
def Closed (A : List CapKind) : Prop :=
  ∀ k ∈ A, ∀ k' ∈ k.below, k' ∈ A

theorem CapKind.mem_below_self (k : CapKind) : k ∈ k.below := by
  cases k <;> simp [CapKind.below]

theorem CapKind.below_trans {k k' k'' : CapKind} (h : k' ∈ k.below) (h' : k'' ∈ k'.below) :
    k'' ∈ k.below := by
  revert h h'
  cases k <;> cases k' <;> cases k'' <;> simp [CapKind.below]

theorem authority_closed (f : Function) : Closed (authority f) := by
  intro k hk k' hk'
  simp only [authority, List.mem_flatMap] at hk ⊢
  obtain ⟨a, ha, hka⟩ := hk
  exact ⟨a, ha, CapKind.below_trans hka hk'⟩

/-- The kinds a call of `f` can reach when the function values among its
arguments hold only capabilities of kinds in `A`: `f`'s authority, and
every kind below one in `A`. -/
def callAuthority (f : Function) (A : List CapKind) : List CapKind :=
  authority f ++ A.flatMap CapKind.below

theorem callAuthority_closed (f : Function) (A : List CapKind) : Closed (callAuthority f A) := by
  intro k hk k' hk'
  simp only [callAuthority, List.mem_append, List.mem_flatMap] at hk ⊢
  rcases hk with hk | ⟨a, ha, hka⟩
  · exact Or.inl (authority_closed f k hk k' hk')
  · exact Or.inr ⟨a, ha, CapKind.below_trans hka hk'⟩

theorem callAuthority_nil (f : Function) : callAuthority f [] = authority f := by
  simp [callAuthority]

/-! ## Capabilities in an expression -/

theorem capsWithinList_append {A : List CapKind} :
    ∀ {xs ys : List Expr}, Expr.capsWithinList A (xs ++ ys) =
      (Expr.capsWithinList A xs && Expr.capsWithinList A ys) := by
  intro xs ys
  induction xs with
  | nil => simp [Expr.capsWithinList]
  | cons x xs ih => simp [Expr.capsWithinList, ih, Bool.and_assoc]

theorem capsWithinList_get {A : List CapKind} :
    ∀ {xs : List Expr} {i : Nat} {v : Expr}, Expr.capsWithinList A xs = true →
      xs[i]? = some v → v.capsWithin A = true := by
  intro xs
  induction xs with
  | nil => intro i v _ h; simp at h
  | cons x xs ih =>
    intro i v hx h
    simp only [Expr.capsWithinList, Bool.and_eq_true] at hx
    cases i with
    | zero => simp at h; subst h; exact hx.1
    | succ i => exact ih hx.2 (by simpa using h)

/-- A wider set of kinds bounds what a narrower one does. -/
theorem capsWithin_mono {A B : List CapKind} (hAB : ∀ k ∈ A, k ∈ B) :
    ∀ e : Expr, e.capsWithin A = true → e.capsWithin B = true := by
  intro e
  induction e using Expr.capsWithin.induct
    (motive_2 := fun arms => Expr.capsWithinArms A arms = true → Expr.capsWithinArms B arms = true)
    (motive_3 := fun e => Expr.capsWithinOpt A e = true → Expr.capsWithinOpt B e = true)
    (motive_4 := fun ss => Expr.capsWithinStmts A ss = true → Expr.capsWithinStmts B ss = true)
    (motive_5 := fun es => Expr.capsWithinList A es = true → Expr.capsWithinList B es = true)
  all_goals
    intros
    simp_all [Expr.capsWithin, Expr.capsWithinList, Expr.capsWithinOpt, Expr.capsWithinStmts,
      Expr.capsWithinArms]

/-- Putting a value for a slot adds only that value's capabilities. -/
theorem capsWithinList_mono {A B : List CapKind} (hAB : ∀ k ∈ A, k ∈ B) {es : List Expr}
    (h : Expr.capsWithinList A es = true) : Expr.capsWithinList B es = true := by
  have := capsWithin_mono hAB (.call "" es) (by simpa [Expr.capsWithin] using h)
  simpa [Expr.capsWithin] using this

theorem capsWithin_subst {A : List CapKind} {v : Expr} (hv : v.capsWithin A = true) :
    ∀ e : Expr, e.capsWithin A = true → (Expr.subst v e).capsWithin A = true := by
  intro e
  induction e using Expr.subst.induct
    (motive_2 := fun arms => Expr.capsWithinArms A arms = true →
      Expr.capsWithinArms A (Expr.substArms v arms) = true)
    (motive_3 := fun e => Expr.capsWithinOpt A e = true →
      Expr.capsWithinOpt A (Expr.substOpt v e) = true)
    (motive_4 := fun ss => Expr.capsWithinStmts A ss = true →
      Expr.capsWithinStmts A (Expr.substStmts v ss) = true)
    (motive_5 := fun es => Expr.capsWithinList A es = true →
      Expr.capsWithinList A (Expr.substList v es) = true)
  all_goals
    intros
    simp_all [Expr.subst, Expr.substList, Expr.substOpt, Expr.substStmts, Expr.substArms,
      Expr.capsWithin, Expr.capsWithinList, Expr.capsWithinOpt, Expr.capsWithinStmts,
      Expr.capsWithinArms]

theorem capsWithin_substStmts {A : List CapKind} {v : Expr} (hv : v.capsWithin A = true)
    {ss : List Stmt} {val : Option Expr} (h : (Expr.block ss val).capsWithin A = true) :
    (Expr.block (Expr.substStmts v ss) (Expr.substOpt v val)).capsWithin A = true := by
  have := capsWithin_subst hv (.block ss val) h
  simpa [Expr.subst] using this

theorem capsWithin_substAll {A : List CapKind} :
    ∀ {bs : List Expr} {e : Expr}, Expr.capsWithinList A bs = true → e.capsWithin A = true →
      (Expr.substAll bs e).capsWithin A = true := by
  intro bs
  induction bs with
  | nil => intro e _ he; exact he
  | cons b bs ih =>
    intro e hbs he
    simp only [Expr.capsWithinList, Bool.and_eq_true] at hbs
    exact ih hbs.2 (capsWithin_subst hbs.1 e he)

theorem capsWithin_instantiate {A : List CapKind} :
    ∀ {ps : List Param} {vs : List Expr} {body e : Expr}, Expr.capsWithinList A vs = true →
      body.capsWithin A = true → instantiate ps vs body = some e → e.capsWithin A = true := by
  intro ps
  induction ps with
  | nil =>
    intro vs body e _ hb hi
    cases vs <;> simp [instantiate] at hi
    subst hi; exact hb
  | cons p ps ih =>
    intro vs body e hvs hb hi
    cases vs with
    | nil => simp [instantiate] at hi
    | cons v vs =>
      simp only [Expr.capsWithinList, Bool.and_eq_true] at hvs
      simp only [instantiate] at hi
      split at hi
      · exact ih hvs.2 (capsWithin_subst hvs.1 _ hb) hi
      · exact ih hvs.2 hb hi

theorem capsWithin_enter {A : List CapKind} {f : Function} {vs : List Expr} {e : Expr}
    (hvs : Expr.capsWithinList A vs = true) (hb : f.body.capsWithin A = true)
    (he : f.enter vs = some e) : e.capsWithin A = true := by
  unfold Function.enter at he
  split at he
  · exact capsWithin_instantiate hvs hb he
  · obtain ⟨b, hi, rfl⟩ := Option.map_eq_some_iff.mp he
    simp only [Expr.capsWithinList, Bool.and_eq_true] at hvs
    simp only [Expr.capsWithin, Bool.and_eq_true]
    exact ⟨hvs.1, capsWithin_instantiate hvs.2 hb hi⟩
  · cases he

/-- The capabilities of a list with one expression replaced are those of
the list and of the new one. -/
theorem capsWithinList_set {A : List CapKind} {x : Expr} (hx : x.capsWithin A = true) :
    ∀ {xs : List Expr} {i : Nat}, Expr.capsWithinList A xs = true →
      Expr.capsWithinList A (xs.set i x) = true := by
  intro xs
  induction xs with
  | nil => intro i _; rfl
  | cons y ys ih =>
    intro i h
    simp only [Expr.capsWithinList, Bool.and_eq_true] at h
    cases i with
    | zero => simp [Expr.capsWithinList, hx, h.2]
    | succ i => simp [Expr.capsWithinList, h.1, ih h.2]

/-- A field of a value holds only what the value holds. -/
theorem capsWithin_getAt {A : List CapKind} :
    ∀ (is : List Nat) {v : Expr}, v.capsWithin A = true → (v.getAt is).capsWithin A = true := by
  intro is
  induction is with
  | nil => intro v h; exact h
  | cons i is ih =>
    intro v h
    cases v with
    | construct t var fs =>
      cases var with
      | none =>
        simp only [Expr.getAt]
        apply ih
        cases hf : fs[i]? with
        | none => rfl
        | some x => exact capsWithinList_get (by simpa [Expr.capsWithin] using h) hf
      | some _ => simp [Expr.getAt, Expr.capsWithin]
    | _ => simp [Expr.getAt, Expr.capsWithin]

/-- A value with another put in a field holds only what the two hold. -/
theorem capsWithin_setAt {A : List CapKind} {w : Expr} (hw : w.capsWithin A = true) :
    ∀ (is : List Nat) {v : Expr}, v.capsWithin A = true → (v.setAt is w).capsWithin A = true := by
  intro is
  induction is with
  | nil => intro v _; exact hw
  | cons i is ih =>
    intro v h
    cases v with
    | construct t var fs =>
      cases var with
      | none =>
        simp only [Expr.setAt, Expr.capsWithin] at h ⊢
        refine capsWithinList_set (ih ?_) h
        cases hf : fs[i]? with
        | none => rfl
        | some x => exact capsWithinList_get h hf
      | some _ => simpa [Expr.setAt] using h
    | _ => simpa [Expr.setAt] using h

/-- What a pattern binds is part of the value it matches. -/
theorem capsWithin_matchPat {A : List CapKind} :
    ∀ (p : Pattern) (v : Expr) (bs : List Expr), v.capsWithin A = true →
      matchPat p v = some bs → Expr.capsWithinList A bs = true := by
  intro p v
  induction p, v using matchPat.induct
    (motive_2 := fun ps vs => ∀ bs, Expr.capsWithinList A vs = true →
      matchPats ps vs = some bs → Expr.capsWithinList A bs = true)
  · intro bs _ hm; simp [matchPat] at hm; subst hm; rfl
  · intro bs hv hm; simp [matchPat] at hm; subst hm; simp [Expr.capsWithinList, hv]
  · intro bs _ hm; simp [matchPat] at hm; subst hm; rfl
  · intro bs _ hm; simp [matchPat] at hm; subst hm; rfl
  · intro bs _ hm; simp_all [matchPat]
  · intro bs _ hm; simp [matchPat] at hm; subst hm; rfl
  · intro bs _ hm; simp_all [matchPat]
  · rename_i ih
    intro bs hv hm
    simp only [matchPat, ite_true] at hm
    exact ih bs (by simpa [Expr.capsWithin] using hv) hm
  · intro bs _ hm; simp_all [matchPat]
  · rename_i ih
    intro bs hv hm
    simp only [matchPat, ite_true] at hm
    exact ih bs (by simpa [Expr.capsWithin] using hv) hm
  · intro bs _ hm; simp_all [matchPat]
  · rename_i t x h1 h2 h3 h4 h5 h6 h7
    intro bs _ hm
    cases t <;> cases x <;> first
      | (exfalso; first
          | exact h1 rfl | exact h2 _ rfl | exact h3 rfl rfl | exact h4 _ _ rfl rfl
          | exact h5 _ _ rfl rfl | exact h6 _ _ _ _ _ _ rfl rfl | exact h7 _ _ _ _ _ _ rfl rfl)
      | simp_all [matchPat]
  · rename_i bs _ hm; simp [matchPats] at hm; subst hm; rfl
  · rename_i vs bs₁ bss hps hp ih₁ ih₂ bs hv hm
    simp only [matchPats, hps, hp, Option.some.injEq] at hm
    subst hm
    simp only [Expr.capsWithinList, Bool.and_eq_true] at hv
    rw [capsWithinList_append, ih₁ _ hv.1 hp, ih₂ _ hv.2 hps]
    rfl
  · rename_i vs hno ih₁ ih₂ bs hv hm
    simp only [matchPats] at hm
    exact absurd hm (by simp)
  · rename_i t x h1 h2 bs hv hm
    cases t <;> cases x <;> first
      | (exfalso; first | exact h1 rfl rfl | exact h2 _ _ _ _ rfl rfl)
      | simp_all [matchPats]

theorem readOnly_writeOnly_below {k k' : CapKind}
    (h : k.readOnly = some k' ∨ k.writeOnly = some k') : k' ∈ k.below := by
  rcases h with h | h <;> cases k <;> simp [CapKind.readOnly, CapKind.writeOnly] at h <;>
    subst h <;> simp [CapKind.below]

/-- A pure built-in call on a capability is `read_only` or `write_only`:
it gives a capability of a narrower kind with the same root. -/
theorem evalPure_cap {op : Builtin} {k : CapKind} {r : CapRoot} {v : Expr}
    (h : evalPure op [.cap k r] = some v) :
    ∃ k', (k.readOnly = some k' ∨ k.writeOnly = some k') ∧ v = .cap k' r := by
  cases op <;> simp [evalPure] at h
  all_goals
    obtain ⟨rfl, a, ha, rfl⟩ := h
    exact ⟨a, by simp [ha], rfl⟩

/-- What a pure built-in call gives holds only capabilities below those
of its arguments. -/
theorem capsWithin_evalPure {A : List CapKind} (hA : Closed A) {op : Builtin} {vs : List Expr}
    {v : Expr} (hvs : Expr.capsWithinList A vs = true) (h : evalPure op vs = some v) :
    v.capsWithin A = true := by
  by_cases hro : ∃ k₀, op = .readOnly k₀ ∨ op = .writeOnly k₀
  · have shape : ∃ k r, vs = [.cap k r] := by
      obtain ⟨k₀, rfl | rfl⟩ := hro <;> unfold evalPure at h <;> split at h <;>
        first | exact ⟨_, _, rfl⟩ | (cases h; done) | simp_all
    obtain ⟨k, r, rfl⟩ := shape
    obtain ⟨k', hk', rfl⟩ := evalPure_cap h
    simp only [Expr.capsWithinList, Expr.capsWithin, Bool.and_true, decide_eq_true_eq] at hvs ⊢
    exact hA _ hvs _ (readOnly_writeOnly_below hk')
  · unfold evalPure at h
    split at h
    all_goals first
      | (simp only [Option.some.injEq] at h; subst h; simp_all [Expr.capsWithin, Expr.capsWithinList])
      | exact (hro ⟨_, Or.inl rfl⟩).elim
      | exact (hro ⟨_, Or.inr rfl⟩).elim
      | cases h

/-! ## The invariant -/

/-- What a step leads to holds only capabilities of kinds in `A`, and an
effect is on a capability of a kind in `A`, and goes on, for an answer
within `A`, to an expression within `A`. -/
def OutcomeWithin (A : List CapKind) : Outcome → Prop
  | .next e => e.capsWithin A = true
  | .panic _ => True
  | .perform req k => req.kind ∈ A ∧ ∀ r, r.capsWithin A = true → (k r).capsWithin A = true
  | .ret v => v.capsWithin A = true
  | .read _ _ k => ∀ r, r.capsWithin A = true → (k r).capsWithin A = true
  | .write _ _ w e => w.capsWithin A = true ∧ e.capsWithin A = true

theorem OutcomeWithin.map {A : List CapKind} {o : Outcome} {f : Expr → Expr}
    (h : OutcomeWithin A o) (hf : ∀ x, x.capsWithin A = true → (f x).capsWithin A = true) :
    OutcomeWithin A (o.map f) := by
  cases o with
  | next e => exact hf e h
  | panic _ => trivial
  | perform req k => exact ⟨h.1, fun r hr => hf _ (h.2 r hr)⟩
  | ret v => exact h
  | read _ _ k => exact fun r hr => hf _ (h r hr)
  | write _ _ w e => exact ⟨h.1, hf _ h.2⟩

theorem OutcomeWithin.inFrame {A : List CapKind} {o : Outcome} (h : OutcomeWithin A o) :
    OutcomeWithin A o.inFrame := by
  cases o <;> simp_all [OutcomeWithin, Outcome.inFrame, Outcome.map, Expr.capsWithin]

/-- A binder of var 0 holding `v` keeps a step's outcome within `A` when
`v` is: it answers a read with a field of `v`, and a write by putting
the value written in `v`. -/
theorem OutcomeWithin.under {A : List CapKind} {o : Outcome} {v : Expr}
    {wrap : Expr → Expr → Expr} {onRet : Expr → Outcome} (h : OutcomeWithin A o)
    (hv : v.capsWithin A = true)
    (hwrap : ∀ v e, v.capsWithin A = true → e.capsWithin A = true → (wrap v e).capsWithin A = true)
    (hret : ∀ r, r.capsWithin A = true → OutcomeWithin A (onRet r)) :
    OutcomeWithin A (o.under v wrap onRet) := by
  cases o with
  | next e => exact hwrap v e hv h
  | panic _ => trivial
  | perform req k => exact ⟨h.1, fun r hr => hwrap v _ hv (h.2 r hr)⟩
  | ret r => exact hret r h
  | read i is k =>
    cases i with
    | zero => exact hwrap v _ hv (h _ (capsWithin_getAt is hv))
    | succ i => exact fun r hr => hwrap v _ hv (h r hr)
  | write i is w e =>
    cases i with
    | zero => exact hwrap _ e (capsWithin_setAt h.1 is hv) h.2
    | succ i => exact ⟨h.1, hwrap v e hv h.2⟩

/-- No body of the program holds a capability value. -/
def ProgramCapFree (P : Program) : Prop :=
  ∀ name f, P.find? name = some f → f.body.capFree = true

theorem ProgramSourceTyped.capFree {P : Program} (h : ProgramSourceTyped P) : ProgramCapFree P :=
  fun name f hf => (h.1 name f hf).2.2

/-- A step keeps the capabilities of a run within `A`, and labels each
effect with a kind in `A`. -/
theorem step_within {P : Program} (hP : ProgramCapFree P) {A : List CapKind} (hA : Closed A)
    {e : Expr} {o : Outcome} (hs : Step P e o) (he : e.capsWithin A = true) : OutcomeWithin A o := by
  induction hs with
  | negArg _ ih => exact (ih he).map fun _ h => h
  | notArg _ ih => exact (ih he).map fun _ h => h
  | neg => unfold intResult; split <;> first | rfl | trivial
  | not => rfl
  | binaryLhs _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.1).map fun _ h => by simp [Expr.capsWithin, h, he.2]
  | binaryRhs _ _ _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.2).map fun _ h => by simp [Expr.capsWithin, h, he.1]
  | binary _ _ hb =>
    have := evalBinary_wellFormed hb
    unfold evalBinary at hb
    split at hb <;> simp only [Option.some.injEq, reduceCtorEq] at hb <;> subst hb <;>
      first | rfl | (unfold intResult; split <;> trivial) |
        (split <;> first | trivial | (unfold intResult; split <;> trivial))
  | andFalse | orTrue | impliesFalse => rfl
  | andTrue | orFalse | impliesTrue =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact he.2
  | callArg _ _ ih =>
    simp only [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList,
      Bool.and_eq_true] at he
    exact (ih he.2.1).map fun _ h => by
      simp [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList, h, he.1, he.2.2]
  | call hf _ hi =>
    simp only [Expr.capsWithin] at he
    have h := capsWithin_enter he (capsWithin_mono (fun _ h => by simp at h) _ (hP _ _ hf)) hi
    simpa [OutcomeWithin, Expr.capsWithin] using h
  | ifCond _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.1.1).map fun _ h => by simp [Expr.capsWithin, h, he.1.2, he.2]
  | ifTrue | ifFalse =>
    simp_all [Expr.capsWithin, Expr.capsWithinOpt, OutcomeWithin]
  | ifTrueNoElse =>
    simp_all [Expr.capsWithin, Expr.capsWithinOpt, Expr.capsWithinStmts, OutcomeWithin]
  | ifFalseNoElse | blockNone => rfl
  | blockSome =>
    simp_all [Expr.capsWithin, Expr.capsWithinOpt, Expr.capsWithinStmts, OutcomeWithin]
  | blockExprArg _ ih =>
    simp only [Expr.capsWithin, Expr.capsWithinStmts, Bool.and_eq_true] at he
    exact (ih he.1.1).map fun _ h => by
      simp [Expr.capsWithin, Expr.capsWithinStmts, h, he.1.2, he.2]
  | blockExpr _ | blockLetDrop _ =>
    simp_all [Expr.capsWithin, Expr.capsWithinStmts, OutcomeWithin]
  | blockLetArg _ ih =>
    simp only [Expr.capsWithin, Expr.capsWithinStmts, Bool.and_eq_true] at he
    exact (ih he.1.1).map fun _ h => by
      simp [Expr.capsWithin, Expr.capsWithinStmts, h, he.1.2, he.2]
  | blockLetBind _ =>
    simp only [Expr.capsWithin, Expr.capsWithinStmts, Bool.and_eq_true] at he
    exact capsWithin_substStmts he.1.1 (by simp [Expr.capsWithin, he.1.2, he.2])
  | constructArg _ _ ih =>
    simp only [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList,
      Bool.and_eq_true] at he
    exact (ih he.2.1).map fun _ h => by
      simp [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList, h, he.1, he.2.2]
  | fieldBase _ ih => exact (ih he).map fun _ h => h
  | field _ hi =>
    simp only [Expr.capsWithin] at he
    exact capsWithinList_get he hi
  | matchScrutinee _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.1).map fun _ h => by simp [Expr.capsWithin, h, he.2]
  | matchSkip _ _ =>
    simp_all [Expr.capsWithin, Expr.capsWithinArms, OutcomeWithin]
  | matchArm _ hm =>
    simp only [Expr.capsWithin, Expr.capsWithinArms, Bool.and_eq_true] at he
    exact capsWithin_substAll (capsWithin_matchPat _ _ _ he.1 hm) he.2.1.2
  | matchGuard _ hm =>
    simp only [Expr.capsWithin, Expr.capsWithinArms, Expr.capsWithinOpt,
      Bool.and_eq_true] at he
    have hbs := capsWithin_matchPat _ _ _ he.1 hm
    simp [OutcomeWithin, Expr.capsWithin, Expr.capsWithinOpt, capsWithin_substAll hbs he.2.1.1,
      capsWithin_substAll hbs he.2.1.2, he.1, he.2.2]
  | builtinArg _ _ ih =>
    simp only [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList,
      Bool.and_eq_true] at he
    exact (ih he.2.1).map fun _ h => by
      simp [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList, h, he.1, he.2.2]
  | builtinPure _ hv =>
    simp only [Expr.capsWithin] at he
    exact capsWithin_evalPure hA he hv
  | builtinEffect _ _ =>
    simp only [Expr.capsWithin, Expr.capsWithinList, Bool.and_eq_true,
      decide_eq_true_eq] at he
    exact ⟨he.1, fun r hr => hr⟩
  | callValueCallee _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.1).map fun _ h => by simp [Expr.capsWithin, h, he.2]
  | callValueArg _ _ _ ih =>
    simp only [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList,
      Bool.and_eq_true] at he
    exact (ih he.2.2.1).map fun _ h => by
      simp [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList, h, he.1, he.2.1, he.2.2.2]
  | callValue _ hi =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    have h := capsWithin_instantiate he.2 he.1 hi
    simpa [OutcomeWithin, Expr.capsWithin] using h
  | resultArg _ ih | tryArg _ ih =>
    simp only [Expr.capsWithin] at he
    exact (ih he).map fun _ h => by simpa [Expr.capsWithin] using h
  | tryOk _ | tryErr _ | frameValue _ =>
    simpa [OutcomeWithin, Expr.capsWithin] using he
  | tryConvert _ =>
    simpa [OutcomeWithin, Expr.capsWithin, Expr.capsWithinList] using he
  | frame _ ih =>
    simp only [Expr.capsWithin] at he
    exact (ih he).inFrame
  | var => exact fun r hr => hr
  | blockVarArg _ ih | blockAssignArg _ ih =>
    simp only [Expr.capsWithin, Expr.capsWithinStmts, Bool.and_eq_true] at he
    exact (ih he.1.1).map fun _ h => by
      simp [Expr.capsWithin, Expr.capsWithinStmts, h, he.1.2, he.2]
  | blockVar _ | blockAssign _ =>
    simp_all [Expr.capsWithin, Expr.capsWithinStmts, OutcomeWithin]
  | cellBody _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.2).under he.1 (fun _ _ h₁ h₂ => by simp [Expr.capsWithin, h₁, h₂])
      fun _ h => h
  | methodBody _ ih =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact (ih he.2).under he.1 (fun _ _ h₁ h₂ => by simp [Expr.capsWithin, h₁, h₂])
      fun _ h => by simp [OutcomeWithin, Expr.capsWithin, h, he.1]
  | cellValue _ =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact he.2
  | methodValue _ =>
    simp only [Expr.capsWithin, Bool.and_eq_true] at he
    exact he
  | mutCallArg _ _ ih =>
    simp only [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList,
      Bool.and_eq_true] at he
    exact (ih he.2.1).map fun _ h => by
      simp [Expr.capsWithin, capsWithinList_append, Expr.capsWithinList, h, he.1, he.2.2]
  | mutCall hf _ _ hi =>
    simp only [Expr.capsWithin] at he
    have h := capsWithin_instantiate he (capsWithin_mono (fun _ h => by simp at h) _ (hP _ _ hf)) hi
    exact fun r hr => by simp [Expr.capsWithin, hr, h]

/-- An answer the semantics allows an effect on a capability of a kind in
`A` holds only capabilities of kinds in `A`. -/
theorem allows_within {A : List CapKind} (hA : Closed A) {req : Request} {r : Expr}
    (hk : req.kind ∈ A) (hr : req.Allows r) : r.capsWithin A = true := by
  obtain ⟨op, kind, root, args⟩ := req
  cases op <;> simp only [Request.Allows] at hr
  all_goals first
    | (rcases hr with rfl | rfl <;> rfl)
    | (rcases hr with rfl | ⟨_, rfl⟩ <;> rfl)
    | exact hr.elim
    | skip
  obtain ⟨a, k', root', hk', rfl, _⟩ := hr
  simp only [Expr.capsWithin, decide_eq_true_eq]
  refine hA _ hk _ ?_
  revert hk'
  cases kind <;> simp [CapKind.restrictTo, CapKind.below] <;> intros <;> subst_vars <;> simp

/-- **The trace of a run.** In a program whose bodies hold no capability,
a run from an expression whose capabilities have kinds in a closed set
`A` performs only effects of kinds in `A`. -/
theorem steps_within {P : Program} (hP : ProgramCapFree P) {A : List CapKind} (hA : Closed A)
    {e e' : Expr} {tr : List Request} (hs : Steps P e tr e') (he : e.capsWithin A = true) :
    ∀ req ∈ tr, req.kind ∈ A := by
  induction hs with
  | refl => intro req h; cases h
  | next h _ ih => exact ih (step_within hP hA h he)
  | perform h hr _ ih =>
    have ⟨hk, hc⟩ := step_within hP hA h he
    intro req hreq
    cases hreq with
    | head => exact hk
    | tail _ hreq => exact ih (hc _ (allows_within hA hk hr)) req hreq

/-! ## Authority safety -/

/-- A field type is neither a capability nor a `cap fn`. -/
theorem fieldCapFree_caps : ∀ {t : Ty}, t.fieldCapFree = true → t.isCapFn = false ∧ t.capKind? = none
  | .cap _, h | .fn _ _ _, h => by simp [Ty.fieldCapFree] at h
  | .int, _ | .bool, _ | .unit, _ | .string, _ | .path, _ | .ioError, _ | .result _ _, _
  | .named _, _ => ⟨rfl, rfl⟩

/-- A value of a type that is neither a capability nor a `cap fn` holds
no capability, in a program whose structs and enums hold neither; a
capability value has its type's kind. A `cap fn` holds what it captured,
which its type does not say. -/
theorem valueTyped_caps {P : Program} (hT : P.typesCapFree = true) :
    ∀ {g : VGoal}, ValueTyped P g → match g with
      | .one v t => t.isCapFn = false → v.capsWithin t.capKind?.toList = true
      | .many vs ts => ∀ A, (∀ t ∈ ts, t.fieldCapFree = true) → Expr.capsWithinList A vs = true := by
  intro g h
  induction h with
  | cap => intro _; simp [Expr.capsWithin, Ty.capKind?]
  | @closure ps cs cf ret body hcaps _ =>
    intro h
    cases cf with
    | false => exact capsWithin_mono (fun _ h => by simp at h) _ (hcaps rfl)
    | true => simp [Ty.isCapFn] at h
  | construct hf _ ih =>
    intro _
    simpa [Expr.capsWithin, Ty.capKind?] using ih [] (fieldsOf_capFree hT hf)
  | nil => intro A _; rfl
  | @cons v vs t ts _ _ ih₁ ih₂ =>
    intro A hts
    obtain ⟨hcf, hk⟩ := fieldCapFree_caps (hts t List.mem_cons_self)
    simp only [Expr.capsWithinList, Bool.and_eq_true]
    refine ⟨capsWithin_mono (fun _ h => ?_) _ (ih₁ hcf),
      ih₂ A fun t' h' => hts t' (List.mem_cons_of_mem _ h')⟩
    simp [hk] at h
  | ok hf _ ih =>
    intro _
    simp only [Ty.fieldCapFree, Bool.and_eq_true] at hf
    obtain ⟨hcf, hk⟩ := fieldCapFree_caps hf.1
    have h := ih hcf
    rw [hk] at h
    simpa [Expr.capsWithin, Ty.capKind?] using h
  | err hf _ ih =>
    intro _
    simp only [Ty.fieldCapFree, Bool.and_eq_true] at hf
    obtain ⟨hcf, hk⟩ := fieldCapFree_caps hf.2
    have h := ih hcf
    rw [hk] at h
    simpa [Expr.capsWithin, Ty.capKind?] using h
  | _ => intro _; simp [Expr.capsWithin]

/-- The arguments `vs` a call passes for the parameters `ps` whose type is
a `cap fn` hold only capabilities of kinds in `A`: what the closures among
them captured. A plain `fn` argument holds none, by its typing. -/
def FnArgsWithin (A : List CapKind) : List Param → List Expr → Prop
  | p :: ps, v :: vs => (p.type.isCapFn = true → v.capsWithin A = true) ∧ FnArgsWithin A ps vs
  | _, _ => True

/-- With no `cap fn` parameter, there is nothing to bound. -/
theorem FnArgsWithin.of_noCapFn {A : List CapKind} :
    ∀ {ps : List Param} {vs : List Expr}, (∀ p ∈ ps, p.type.isCapFn = false) →
      FnArgsWithin A ps vs
  | [], _, _ => trivial
  | _ :: _, [], _ => trivial
  | p :: ps, _ :: _, h =>
    ⟨fun hp => by simp [h p List.mem_cons_self] at hp,
      FnArgsWithin.of_noCapFn fun q hq => h q (List.mem_cons_of_mem _ hq)⟩

/-- Arguments of a function's parameters' types hold only capabilities
of the kinds those types name, and, for a `cap fn` parameter, those its
closure captured. -/
theorem args_within {P : Program} (hT : P.typesCapFree = true) {A : List CapKind} :
    ∀ {vs : List Expr} {ps : List Param}, ValueTyped P (.many vs (ps.map Param.type)) →
      FnArgsWithin A ps vs →
      Expr.capsWithinList ((ps.filterMap fun p => p.type.capKind?) ++ A) vs = true := by
  intro vs
  induction vs with
  | nil => intro ps _ _; rfl
  | cons v vs ih =>
    intro ps h hfn
    cases ps with
    | nil => cases h
    | cons p ps =>
      cases h with
      | cons hv hvs =>
        obtain ⟨hfv, hfn⟩ := hfn
        have h₂ := ih hvs hfn
        have hv' : v.capsWithin
            ((p.type.capKind?.toList ++ ps.filterMap fun p => p.type.capKind?) ++ A) = true := by
          cases hf : p.type.isCapFn with
          | true => exact capsWithin_mono (fun k hk => by simp [hk]) _ (hfv hf)
          | false => exact capsWithin_mono (fun k hk => by simp [hk]) _ (valueTyped_caps hT hv hf)
        have hfm : (p :: ps).filterMap (fun p => p.type.capKind?) =
            p.type.capKind?.toList ++ ps.filterMap fun p => p.type.capKind? := by
          rw [List.filterMap_cons]
          cases p.type.capKind? <;> rfl
        rw [hfm]
        simp only [Expr.capsWithinList, Bool.and_eq_true]
        refine ⟨hv', capsWithinList_mono (fun k hk => ?_) h₂⟩
        simp only [List.mem_append] at hk ⊢
        rcases hk with hk | hk
        · exact Or.inl (Or.inr hk)
        · exact Or.inr hk

/-- **Authority safety.** In a program as the checker hands it over,
every effect in the trace of any run of a call of `f` on arguments of
its parameters' types has a capability kind in `f`'s authority, or below
a kind in `A` when the function values among the arguments hold only
capabilities of kinds in `A`: a closure reaches what it captured.

The proof does not use `_hf`: the bound holds for any `f` whose
parameter types the arguments have, whichever function `name` names. It
is stated with `_hf` so that it reads as a fact about the function
called. -/
theorem authority_safety {P : Program} (hP : ProgramSourceTyped P) {name : String} {f : Function}
    (_hf : P.find? name = some f) {vs : List Expr}
    (hargs : ValueTyped P (.many vs (f.allParams.map Param.type)))
    {A : List CapKind} (hfn : FnArgsWithin A f.allParams vs)
    {e' : Expr} {tr : List Request} (hrun : Steps P (.call name vs) tr e') :
    ∀ eff ∈ tr, eff.kind ∈ callAuthority f A := by
  refine steps_within hP.capFree (callAuthority_closed f A) hrun
    (capsWithin_mono (A := paramKinds f ++ A) (fun k hk => ?_) _ ?_)
  · simp only [List.mem_append] at hk
    rcases hk with hk | hk
    · exact List.mem_append_left _ (List.mem_flatMap.mpr ⟨k, hk, CapKind.mem_below_self k⟩)
    · exact List.mem_append_right _ (List.mem_flatMap.mpr ⟨k, hk, CapKind.mem_below_self k⟩)
  · simpa [Expr.capsWithin, paramKinds] using args_within hP.2 hargs hfn

mutual
  /-- The kinds of the capability values an expression holds. -/
  def Expr.caps : Expr → List CapKind
    | .cap k _ => [k]
    | .unit | .int _ | .bool _ | .local_ _ | .str _ | .path _ | .ioError => []
    | .neg e | .not e | .field e _ | .result _ e | .try_ e _ | .frame e => Expr.caps e
    | .binary _ l r => Expr.caps l ++ Expr.caps r
    | .call _ args | .builtin _ args => Expr.capsList args
    | .construct _ _ fs => Expr.capsList fs
    | .if_ c th el => Expr.caps c ++ Expr.caps th ++ Expr.capsOpt el
    | .block ss val => Expr.capsStmts ss ++ Expr.capsOpt val
    | .match_ sc arms => Expr.caps sc ++ Expr.capsArms arms
    | .closure _ _ _ _ _ body => Expr.caps body
    | .callValue c args => Expr.caps c ++ Expr.capsList args
    | .var_ _ => []
    | .mutCall _ _ args => Expr.capsList args
    | .cell v e | .method _ v e => Expr.caps v ++ Expr.caps e

  def Expr.capsList : List Expr → List CapKind
    | [] => []
    | e :: es => Expr.caps e ++ Expr.capsList es

  def Expr.capsOpt : Option Expr → List CapKind
    | none => []
    | some e => Expr.caps e

  def Expr.capsStmts : List Stmt → List CapKind
    | [] => []
    | .let_ _ _ e :: ss => Expr.caps e ++ Expr.capsStmts ss
    | .expr e :: ss => Expr.caps e ++ Expr.capsStmts ss
    | .var_ _ e :: ss => Expr.caps e ++ Expr.capsStmts ss
    | .assign _ e :: ss => Expr.caps e ++ Expr.capsStmts ss

  def Expr.capsArms : List Arm → List CapKind
    | [] => []
    | .mk _ g b :: arms => Expr.capsOpt g ++ Expr.caps b ++ Expr.capsArms arms
end

/-- An expression holds only capabilities of the kinds `Expr.caps` lists. -/
theorem capsWithin_caps : ∀ e : Expr, ∀ A, (∀ k ∈ e.caps, k ∈ A) → e.capsWithin A = true := by
  intro e
  induction e using Expr.capsWithin.induct
    (motive_2 := fun arms => ∀ A, (∀ k ∈ Expr.capsArms arms, k ∈ A) →
      Expr.capsWithinArms A arms = true)
    (motive_3 := fun e => ∀ A, (∀ k ∈ Expr.capsOpt e, k ∈ A) → Expr.capsWithinOpt A e = true)
    (motive_4 := fun ss => ∀ A, (∀ k ∈ Expr.capsStmts ss, k ∈ A) →
      Expr.capsWithinStmts A ss = true)
    (motive_5 := fun es => ∀ A, (∀ k ∈ Expr.capsList es, k ∈ A) →
      Expr.capsWithinList A es = true)
  all_goals
    intros
    simp_all [Expr.caps, Expr.capsList, Expr.capsOpt, Expr.capsStmts, Expr.capsArms,
      Expr.capsWithin, Expr.capsWithinList, Expr.capsWithinOpt, Expr.capsWithinStmts,
      Expr.capsWithinArms, or_imp, forall_and]

/-- The kinds of the capabilities the `cap fn` arguments among `vs` hold,
for the parameters `ps`: what the closures passed for them captured. -/
def capFnKinds : List Param → List Expr → List CapKind
  | p :: ps, v :: vs => (if p.type.isCapFn then v.caps else []) ++ capFnKinds ps vs
  | _, _ => []

theorem FnArgsWithin.of_capFnKinds : ∀ (ps : List Param) (vs : List Expr),
    FnArgsWithin (capFnKinds ps vs) ps vs
  | [], _ => trivial
  | _ :: _, [] => trivial
  | p :: ps, v :: vs => by
    refine ⟨fun hp => capsWithin_caps v _ fun k hk => ?_, ?_⟩
    · simp [capFnKinds, hp, hk]
    · have h := FnArgsWithin.of_capFnKinds ps vs
      have mono : ∀ {A B : List CapKind}, (∀ k ∈ A, k ∈ B) → ∀ {ps vs},
          FnArgsWithin A ps vs → FnArgsWithin B ps vs := by
        intro A B hAB ps
        induction ps with
        | nil => intro vs _; cases vs <;> trivial
        | cons q qs ih =>
          intro vs h
          cases vs with
          | nil => trivial
          | cons w ws => exact ⟨fun hq => capsWithin_mono hAB _ (h.1 hq), ih h.2⟩
      exact mono (fun k hk => by simp [capFnKinds, hk]) h

/-- **Authority safety**, with the bound computed from the arguments:
every effect has a kind in `f`'s authority, or below a kind of a
capability that a `cap fn` among the arguments holds. -/
theorem authority_safety_ofArgs {P : Program} (hP : ProgramSourceTyped P) {name : String}
    {f : Function} (hf : P.find? name = some f) {vs : List Expr}
    (hargs : ValueTyped P (.many vs (f.allParams.map Param.type)))
    {e' : Expr} {tr : List Request} (hrun : Steps P (.call name vs) tr e') :
    ∀ eff ∈ tr, eff.kind ∈ authority f ++ (capFnKinds f.allParams vs).flatMap CapKind.below :=
  authority_safety hP hf hargs (FnArgsWithin.of_capFnKinds f.allParams vs) hrun

/-- **Authority safety** for a function with no `cap fn` parameter: every
effect has a kind in its authority. -/
theorem authority_safety_noCapFn {P : Program} (hP : ProgramSourceTyped P) {name : String}
    {f : Function} (hf : P.find? name = some f) (hfo : ∀ p ∈ f.allParams, p.type.isCapFn = false)
    {vs : List Expr} (hargs : ValueTyped P (.many vs (f.allParams.map Param.type)))
    {e' : Expr} {tr : List Request} (hrun : Steps P (.call name vs) tr e') :
    ∀ eff ∈ tr, eff.kind ∈ authority f := by
  have := authority_safety hP hf hargs (A := []) (FnArgsWithin.of_noCapFn hfo) hrun
  simpa [callAuthority_nil] using this

/-- Every effect in a run's trace is one a step performs, so its receiver
has the kind the call takes. -/
theorem steps_receiver {P : Program} {e e' : Expr} {tr : List Request} (hs : Steps P e tr e') :
    ∀ req ∈ tr, req.op.receiver = some req.kind := by
  induction hs with
  | refl => intro req h; cases h
  | next _ _ ih => exact ih
  | perform h _ _ ih =>
    intro req hreq
    cases hreq with
    | head => exact h.wellFormed
    | tail _ hreq => exact ih req hreq

/-- **Authority safety of the Lean reference.** However a run of the
Lean reference on its fake host ends, in a value, a panic or stuck, it
took steps of the semantics whose every effect has a kind in `f`'s
authority, or below a kind in `A` that the function values among the
arguments hold. -/
theorem authority_safety_reference {P : Program} (hP : ProgramSourceTyped P) {name : String}
    {f : Function} (hf : P.find? name = some f) {vs : List Expr}
    (hargs : ValueTyped P (.many vs (f.allParams.map Param.type)))
    {A : List CapKind} (hfn : FnArgsWithin A f.allParams vs) {fuel : Nat} {h h' : Host} :
    (∀ {v}, run P fuel h (.call name vs) = (.value v, h') →
      ∃ tr, Steps P (.call name vs) tr v ∧ ∀ eff ∈ tr, eff.kind ∈ callAuthority f A) ∧
    (∀ {k}, run P fuel h (.call name vs) = (.panic k, h') →
      ∃ tr e', Steps P (.call name vs) tr e' ∧ Step P e' (.panic k) ∧
        ∀ eff ∈ tr, eff.kind ∈ callAuthority f A) ∧
    (∀ {e'}, run P fuel h (.call name vs) = (.stuck e', h') →
      ∃ tr, Steps P (.call name vs) tr e' ∧ ∀ eff ∈ tr, eff.kind ∈ callAuthority f A) := by
  refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_⟩
  · obtain ⟨tr, hs, _⟩ := run_sound.1 hr
    exact ⟨tr, hs, authority_safety hP hf hargs hfn hs⟩
  · obtain ⟨tr, e', hs, hp⟩ := run_sound.2.1 hr
    exact ⟨tr, e', hs, hp, authority_safety hP hf hargs hfn hs⟩
  · obtain ⟨tr, hs, _⟩ := run_sound.2.2.1 hr
    exact ⟨tr, hs, authority_safety hP hf hargs hfn hs⟩

/-! ## Attenuation never widens -/

/-- **Attenuation never widens.** `read_only` and `write_only` give a
capability of a kind below the receiver's with the same root; any answer
the semantics allows `restrict_to` is a capability of a kind below the
receiver's whose root reaches no path the receiver's does not; and the
fake host's answer to `restrict_to` is one.

The second conjunct holds by the definition of `Request.Allows`, which
admits for `restrict_to` only such a capability; the work is in the
third, that the fake host keeps to it (`Host.restrictTo_within`), and in
`fake_host_stays_beneath`, that its reads, writes and new directories
stay beneath the capability's root. -/
theorem attenuation_never_widens :
    (∀ {op : Builtin} {k k' : CapKind} {r r' : CapRoot},
      evalPure op [.cap k r] = some (.cap k' r') → k' ∈ k.below ∧ r' = r) ∧
    (∀ {req : Request} {k' : CapKind} {r' : CapRoot}, req.Allows (.cap k' r') →
      k' ∈ req.kind.below ∧ r'.Within req.root) ∧
    (∀ (h : Host) {k : CapKind} {r : CapRoot} {args : List Expr} {k' : CapKind} {r' : CapRoot},
      (h.perform ⟨.restrictTo k, k, r, args⟩).1 = .cap k' r' → k' ∈ k.below ∧ r'.Within r) := by
  refine ⟨fun {op k k' r r'} h => ?_, fun {req k' r'} h => ?_, fun h {k r args k' r'} hp => ?_⟩
  · obtain ⟨k'', hk, heq⟩ := evalPure_cap h
    cases heq
    exact ⟨readOnly_writeOnly_below hk, rfl⟩
  · obtain ⟨op, kind, root, args⟩ := req
    cases op <;> simp only [Request.Allows] at h
    all_goals first
      | (rcases h with h | h <;> cases h; done)
      | (rcases h with h | ⟨_, h⟩ <;> cases h; done)
      | exact h.elim
      | skip
    obtain ⟨a, k'', root', hk, heq, hw⟩ := h
    cases heq
    refine ⟨?_, hw⟩
    revert hk
    cases kind <;> simp [CapKind.restrictTo, CapKind.below] <;> intros <;> subst_vars <;> simp
  · have hal := Host.perform_allows h (req := ⟨.restrictTo k, k, r, args⟩)
    cases hk : k.restrictTo with
    | none => simp [Host.perform, hk] at hp
    | some ak =>
      have hal := hal (by simp [Builtin.receiver, hk])
      rw [hp] at hal
      obtain ⟨a', k₂, root', hk₂, heq, hw⟩ := hal
      cases heq
      refine ⟨?_, hw⟩
      revert hk₂
      cases k <;> simp [CapKind.restrictTo, CapKind.below] <;> intros <;> subst_vars <;> simp

/-- **The fake host stays beneath a capability's root.** A read reads a
file the root reaches; a write changes or adds only a file the root
reaches; `create_dir` adds only a directory the root reaches. -/
theorem fake_host_stays_beneath (h : Host) (root : CapRoot) :
    (∀ s t, h.read root s = .okStr t → ∃ q, root.Reaches q ∧ h.files.lookup q = some t) ∧
    (∀ s t q x, (q, x) ∈ (h.write root s t).2.files → (q, x) ∈ h.files ∨ root.Reaches q) ∧
    (∀ s q, q ∈ (h.createDir root s).2.dirs → q ∈ h.dirs ∨ root.Reaches q) :=
  ⟨fun s _ hr => Host.read_beneath h root s hr, fun s t => Host.write_beneath h root s t,
    fun s => Host.createDir_beneath h root s⟩

/-! ## Authority safety is not vacuous -/

/-- `fn quiet(out: FsWrite) { let _ = out.restrict_to(".").write(Path.from("a.txt"), "x") }`:
it writes a file, and has no `Stdout`. -/
def quiet : Function where
  name := "quiet"
  params := [{ type := .cap .fsWrite, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .unit .ioError) (.builtin .write
    [.builtin (.restrictTo .fsWrite) [.local_ 0, .str "."], .builtin .pathFrom [.str "a.txt"],
      .str "x"])] none

def quietProgram : Program := { types := [], functions := [quiet] }

theorem quietProgram_sourceTyped : ProgramSourceTyped quietProgram := by
  refine ⟨fun name f hf => ?_, rfl⟩
  simp only [quietProgram, Program.find?, findFunction] at hf
  split at hf
  · cases hf
    refine ⟨?_, rfl, rfl⟩
    exact Typed.block (Typed.blockLetDrop
      (Typed.builtin BuiltinTyped.write
        (Typed.argsCons (Typed.builtin (BuiltinTyped.restrictTo rfl)
            (Typed.argsCons (Typed.local_ rfl) (Typed.argsCons Typed.str Typed.argsNil)))
          (Typed.argsCons (Typed.builtin BuiltinTyped.pathFrom (Typed.argsCons Typed.str Typed.argsNil))
            (Typed.argsCons Typed.str Typed.argsNil))))
      Typed.blockNone)
  · cases hf

/-- A function without `Stdout` among its parameters performs no stdout
effect, in any run, on any arguments of its parameters' types and any
answers the semantics allows. -/
theorem quiet_never_prints {vs : List Expr} {tr : List Request} {e' : Expr}
    (hargs : ValueTyped quietProgram (.many vs [.cap .fsWrite]))
    (hrun : Steps quietProgram (.call "quiet" vs) tr e') : ∀ eff ∈ tr, eff.kind ≠ .stdout := by
  intro eff heff hk
  have := authority_safety_noCapFn quietProgram_sourceTyped (f := quiet) rfl
    (by simp [quiet, Function.allParams, Ty.isCapFn]) hargs hrun eff heff
  rw [hk] at this
  simp [authority, paramKinds, Function.allParams, quiet, Ty.capKind?, CapKind.below] at this

/-- `fn peek(r: FsRead) { let _ = r.restrict_to(".").read_to_string(Path.from("a.txt")) }`:
it reads a file, and has no capability to write one. -/
def peek : Function where
  name := "peek"
  params := [{ type := .cap .fsRead, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .string .ioError) (.builtin .readToString
    [.builtin (.restrictTo .fsRead) [.local_ 0, .str "."], .builtin .pathFrom [.str "a.txt"]])] none

def peekProgram : Program := { types := [], functions := [peek] }

theorem peekProgram_sourceTyped : ProgramSourceTyped peekProgram := by
  refine ⟨fun name f hf => ?_, rfl⟩
  simp only [peekProgram, Program.find?, findFunction] at hf
  split at hf
  · cases hf
    refine ⟨?_, rfl, rfl⟩
    exact Typed.block (Typed.blockLetDrop
      (Typed.builtin BuiltinTyped.readToString
        (Typed.argsCons (Typed.builtin (BuiltinTyped.restrictTo rfl)
            (Typed.argsCons (Typed.local_ rfl) (Typed.argsCons Typed.str Typed.argsNil)))
          (Typed.argsCons (Typed.builtin BuiltinTyped.pathFrom (Typed.argsCons Typed.str Typed.argsNil))
            Typed.argsNil)))
      Typed.blockNone)
  · cases hf

/-- A function whose only capability is `FsRead` never writes a file or
makes a directory, in any run, on any arguments of its parameters' types
and any answers the semantics allows. -/
theorem peek_never_writes {vs : List Expr} {tr : List Request} {e' : Expr}
    (hargs : ValueTyped peekProgram (.many vs [.cap .fsRead]))
    (hrun : Steps peekProgram (.call "peek" vs) tr e') :
    ∀ eff ∈ tr, eff.op ≠ .write ∧ eff.op ≠ .createDir := by
  intro eff heff
  have hk := authority_safety_noCapFn peekProgram_sourceTyped (f := peek) rfl
    (by simp [peek, Function.allParams, Ty.isCapFn]) hargs hrun eff heff
  have hr := steps_receiver hrun eff heff
  simp [authority, paramKinds, Function.allParams, peek, Ty.capKind?, CapKind.below] at hk
  constructor <;> intro hop <;> rw [hop] at hr <;> simp [Builtin.receiver] at hr <;>
    rcases hk with hk | hk <;> rw [hk] at hr <;> cases hr

/-- `fn greet(io: Stdout) { let _ = io.println("hi") }`. -/
def greet : Function where
  name := "greet"
  params := [{ type := .cap .stdout, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .unit .ioError) (.builtin .println [.local_ 0, .str "hi"])] none

def greetProgram : Program := { types := [], functions := [greet] }

/-- A function with `Stdout` among its parameters does perform a stdout
effect: the theorem above is not true for want of any. -/
theorem greet_prints :
    ∃ tr e', Steps greetProgram (.call "greet" [.cap .stdout .anywhere]) tr e' ∧
      ∃ eff ∈ tr, eff.kind = .stdout :=
  ⟨_, _, Steps.next (Step.call rfl (AllValues.cons (IsValue.cap _ _) AllValues.nil) rfl)
    (Steps.perform (req := ⟨.println, .stdout, .anywhere, [.str "hi"]⟩) (r := .okUnit)
      (Step.frame (Step.blockLetArg (Step.builtinEffect rfl
        (AllValues.cons (IsValue.str _) AllValues.nil))))
      (Or.inl rfl) (Steps.refl _)),
    _, List.mem_cons_self, rfl⟩

/-- And the Lean reference, on its fake host, prints `hi`. -/
example : (run greetProgram 10 {} (.call "greet" [.cap .stdout .anywhere])).2.stdout = "hi\n" := by
  decide

/-! ## A closure reaches what it captured -/

/-- `fn apply(f: cap fn() -> Result<(), IoError>) -> Result<(), IoError> { f() }`:
it has no capability of its own and calls the `cap fn` it is given. -/
def apply : Function where
  name := "apply"
  params := [{ type := .fn true [] (.result .unit .ioError), binds := true }]
  ret := .result .unit .ioError
  body := .callValue (.local_ 0) []

/-- `fn applyPure(f: fn() -> Result<(), IoError>) -> Result<(), IoError> { f() }`:
the same with a plain `fn`. -/
def applyPure : Function where
  name := "applyPure"
  params := [{ type := .fn false [] (.result .unit .ioError), binds := true }]
  ret := .result .unit .ioError
  body := .callValue (.local_ 0) []

/-- `|| io.println("hi")`, written where one slot, `io`, is in scope, at
a `cap fn` type when `cf`. -/
def printsHi (cf : Bool) : Expr :=
  .closure [] 1 [0] cf (.result .unit .ioError) (.builtin .println [.local_ 0, .str "hi"])

/-- `fn main(io: Stdout) { let _ = apply(|| io.println("hi")) }`. -/
def closureMain : Function where
  name := "main"
  params := [{ type := .cap .stdout, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .unit .ioError) (.call "apply" [printsHi true])] none

def closureProgram : Program := { types := [], functions := [apply, applyPure, closureMain] }

theorem closureProgram_sourceTyped : ProgramSourceTyped closureProgram := by
  refine ⟨fun name f hf => ?_, rfl⟩
  simp only [closureProgram, Program.find?, findFunction] at hf
  split at hf
  · cases hf
    exact ⟨Typed.callValue (Typed.local_ rfl) Typed.argsNil, rfl, rfl⟩
  · split at hf
    · cases hf
      exact ⟨Typed.callValue (Typed.local_ rfl) Typed.argsNil, rfl, rfl⟩
    · split at hf
      · cases hf
        refine ⟨?_, rfl, rfl⟩
        exact Typed.block (Typed.blockLetDrop
          (Typed.call (f := apply) rfl (Typed.argsCons
            (Typed.closure (by decide) nofun nofun (Typed.builtin BuiltinTyped.println
              (Typed.argsCons (Typed.local_ rfl) (Typed.argsCons Typed.str Typed.argsNil))))
            Typed.argsNil))
          Typed.blockNone)
      · cases hf

/-- A closure that captured `Stdout` prints when `apply` calls it: `main`
puts its `Stdout` in the closure's body as it binds the slot, and the
call of the closure performs the effect. -/
theorem capturing_closure_prints :
    ∃ tr e', Steps closureProgram (.call "main" [.cap .stdout .anywhere]) tr e' ∧
      ∃ eff ∈ tr, eff.kind = .stdout :=
  ⟨_, _, Steps.next (Step.call rfl (AllValues.cons (IsValue.cap _ _) AllValues.nil) rfl)
    (Steps.next (Step.frame (Step.blockLetArg (Step.call rfl
        (AllValues.cons (IsValue.closure _ _ _ _ _) AllValues.nil) rfl)))
      (Steps.next (Step.frame (Step.blockLetArg (Step.frame (Step.callValue AllValues.nil rfl))))
        (Steps.perform (req := ⟨.println, .stdout, .anywhere, [.str "hi"]⟩) (r := .okUnit)
          (Step.frame (Step.blockLetArg (Step.frame (Step.frame
            (Step.builtinEffect (op := .println) rfl (AllValues.cons (IsValue.str _) AllValues.nil))))))
          (Or.inl rfl) (Steps.refl _)))),
    _, List.mem_cons_self, rfl⟩

/-- And the Lean reference, on its fake host, prints `hi`. -/
example :
    (run closureProgram 20 {} (.call "main" [.cap .stdout .anywhere])).2.stdout = "hi\n" := by
  decide

/-- The closure that captures `Stdout` has no plain `fn` type: `main`
could not pass it to `applyPure`. -/
theorem capturing_closure_not_plain (P : Program) (R t : Ty) :
    ¬ Typed P R [.cap .stdout] [] (.expr (printsHi false) t) := by
  intro h
  cases h with
  | closure _ hpure _ _ =>
    have := hpure rfl 0 (.cap .stdout) (by decide) (by decide) rfl
    simp [Program.pureTy] at this

/-- Nor does the closure a run makes of it, with `Stdout` in its body,
have a plain `fn` type as a value. -/
theorem captured_stdout_not_plain_value (P : Program) (t : Ty) :
    ¬ ValueTyped P (.one (.closure [] 0 [] false (.result .unit .ioError)
      (.builtin .println [.cap .stdout .anywhere, .str "hi"])) t) := by
  intro h
  cases h with
  | closure hcaps _ => simp [Expr.capsWithin, Expr.capsWithinList] at hcaps

/-- A plain `fn` gives `applyPure` no authority: whatever closure of its
type `applyPure` is given, any run of it performs no effect at all, on
any answers the semantics allows. -/
theorem applyPure_performs_nothing {g : Expr} {tr : List Request} {e' : Expr}
    (hg : ValueTyped closureProgram (.one g (.fn false [] (.result .unit .ioError))))
    (hrun : Steps closureProgram (.call "applyPure" [g]) tr e') : tr = [] := by
  have := authority_safety_noCapFn closureProgram_sourceTyped (f := applyPure) rfl
    (by simp [applyPure, Function.allParams, Ty.isCapFn]) (ValueTyped.cons hg ValueTyped.nil) hrun
  cases tr with
  | nil => rfl
  | cons eff _ =>
    have h := this eff List.mem_cons_self
    simp [authority, paramKinds, Function.allParams, applyPure, Ty.capKind?] at h

end OskaFormal.Core
