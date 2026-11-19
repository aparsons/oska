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
(`CapKind.below`). In Oska Core no struct or enum holds a capability (the
export refuses one), so a parameter is capability-bearing exactly when its
type is a capability.

- `authority_safety`: in a program as the checker hands it over, every
  effect in the trace of any run of a call of `f` on arguments of its
  parameters' types has a kind in `authority f`, whatever answers the
  host gives that the semantics allows.
- `authority_safety_reference`: so does every effect the Lean reference
  performs, on its fake host.
- `attenuation_never_widens`: attenuation gives a capability of a kind
  below its receiver's, whose root reaches no path the receiver's does
  not, in the semantics (`read_only`, `write_only`, and any answer to
  `restrict_to` the semantics allows) and on the fake host.
- The examples at the end show the theorem is not vacuous: a function
  without `Stdout` among its parameters performs no stdout effect, and
  one with it does perform one.

The proof needs no typing beyond the arguments': it follows the
capability values in the running expression. A source body holds none,
so every capability a run holds is an argument or an attenuation of one,
and an effect is labelled with the kind of a capability the run holds.
-/

namespace OskaFormal.Core

/-! ## Authority -/

/-- The capability kinds a function's parameters name: what the
checker's authority report lists for it. -/
def paramKinds (f : Function) : List CapKind :=
  f.params.filterMap fun p => p.type.capKind?

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
  induction e using Expr.subst.induct
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
  · rename_i t x h1 h2 h3 h4 h5 h6
    intro bs _ hm
    cases t <;> cases x <;> first
      | (exfalso; first
          | exact h1 rfl | exact h2 _ rfl | exact h3 rfl rfl | exact h4 _ _ rfl rfl
          | exact h5 _ _ rfl rfl | exact h6 _ _ _ _ _ _ rfl rfl)
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

theorem OutcomeWithin.map {A : List CapKind} {o : Outcome} {f : Expr → Expr}
    (h : OutcomeWithin A o) (hf : ∀ x, x.capsWithin A = true → (f x).capsWithin A = true) :
    OutcomeWithin A (o.map f) := by
  cases o with
  | next e => exact hf e h
  | panic _ => trivial
  | perform req k => exact ⟨h.1, fun r hr => hf _ (h.2 r hr)⟩

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
    exact capsWithin_instantiate he
      (capsWithin_mono (fun _ h => by simp at h) _ (hP _ _ hf)) hi
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

/-- A value of a type that is not a capability holds no capability, in a
program whose structs and enums hold none; a capability value has its
type's kind. -/
theorem valueTyped_caps {P : Program} (hT : P.typesCapFree = true) :
    ∀ {g : VGoal}, ValueTyped P g → match g with
      | .one v t => v.capsWithin t.capKind?.toList = true
      | .many vs ts => ∀ A, (∀ t ∈ ts, t.capKind? = none) → Expr.capsWithinList A vs = true := by
  intro g h
  induction h with
  | cap => simp [Expr.capsWithin, Ty.capKind?]
  | @construct type variant fs fts hf _ ih =>
    simp only [Expr.capsWithin, Ty.capKind?, Option.toList]
    refine ih [] fun t ht => ?_
    unfold Program.fieldsOf at hf
    unfold Program.typesCapFree at hT
    have mem : ∀ {ds : List TypeDecl} {n : String} {d : TypeDecl}, findType ds n = some d → d ∈ ds := by
      intro ds
      induction ds with
      | nil => intro n d h; simp [findType] at h
      | cons d ds ih =>
        intro n d' h
        simp only [findType] at h
        split at h
        · cases h; exact List.mem_cons_self
        · exact List.mem_cons_of_mem _ (ih h)
    rw [List.all_eq_true] at hT
    split at hf
    · next fs' hd =>
      cases hf
      have := hT _ (mem hd)
      simp only [List.all_eq_true] at this
      simpa using this t ht
    · next vs' v hd =>
      have := hT _ (mem hd)
      simp only [List.all_eq_true] at this
      have hv := lookup_mem hf
      simpa using this _ hv t ht
    · cases hf
  | nil => intro A _; rfl
  | @cons v vs t ts _ _ ih₁ ih₂ =>
    intro A hts
    simp only [Expr.capsWithinList, Bool.and_eq_true]
    refine ⟨capsWithin_mono (fun _ h => ?_) _ ih₁, ih₂ A fun t' h' => hts t' (List.mem_cons_of_mem _ h')⟩
    simp [hts t List.mem_cons_self] at h
  | _ => simp [Expr.capsWithin]

/-- Arguments of a function's parameters' types hold only capabilities
of the kinds those types name. -/
theorem args_within {P : Program} (hT : P.typesCapFree = true) :
    ∀ {vs : List Expr} {ps : List Param}, ValueTyped P (.many vs (ps.map Param.type)) →
      Expr.capsWithinList (ps.filterMap fun p => p.type.capKind?) vs = true := by
  intro vs
  induction vs with
  | nil => intro ps _; rfl
  | cons v vs ih =>
    intro ps h
    cases ps with
    | nil => cases h
    | cons p ps =>
      cases h with
      | cons hv hvs =>
        simp only [Expr.capsWithinList, Bool.and_eq_true, List.filterMap_cons]
        have h₁ : v.capsWithin p.type.capKind?.toList = true := valueTyped_caps hT hv
        have h₂ := ih hvs
        cases hk : p.type.capKind? with
        | none =>
          rw [hk] at h₁
          exact ⟨capsWithin_mono (fun _ h => by simp at h) _ h₁, h₂⟩
        | some k =>
          rw [hk] at h₁
          simp only
          refine ⟨capsWithin_mono (fun k' h => ?_) _ h₁,
            capsWithinList_mono (fun k' h => List.mem_cons_of_mem _ h) h₂⟩
          simp at h; simp [h]

/-- **Authority safety.** In a program as the checker hands it over,
every effect in the trace of any run of a call of `f` on arguments of
its parameters' types has a capability kind in `f`'s authority.

The proof does not use `_hf`: the bound holds for any `f` whose
parameter types the arguments have, whichever function `name` names. It
is stated with `_hf` so that it reads as a fact about the function
called. -/
theorem authority_safety {P : Program} (hP : ProgramSourceTyped P) {name : String} {f : Function}
    (_hf : P.find? name = some f) {vs : List Expr}
    (hargs : ValueTyped P (.many vs (f.params.map Param.type)))
    {e' : Expr} {tr : List Request} (hrun : Steps P (.call name vs) tr e') :
    ∀ eff ∈ tr, eff.kind ∈ authority f := by
  refine steps_within hP.capFree (authority_closed f) hrun
    (capsWithin_mono (A := paramKinds f) (fun k hk => ?_) _ ?_)
  · exact List.mem_flatMap.mpr ⟨k, hk, CapKind.mem_below_self k⟩
  · simpa [Expr.capsWithin, paramKinds] using args_within hP.2 hargs

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
authority. -/
theorem authority_safety_reference {P : Program} (hP : ProgramSourceTyped P) {name : String}
    {f : Function} (hf : P.find? name = some f) {vs : List Expr}
    (hargs : ValueTyped P (.many vs (f.params.map Param.type))) {fuel : Nat} {h h' : Host} :
    (∀ {v}, run P fuel h (.call name vs) = (.value v, h') →
      ∃ tr, Steps P (.call name vs) tr v ∧ ∀ eff ∈ tr, eff.kind ∈ authority f) ∧
    (∀ {k}, run P fuel h (.call name vs) = (.panic k, h') →
      ∃ tr e', Steps P (.call name vs) tr e' ∧ Step P e' (.panic k) ∧
        ∀ eff ∈ tr, eff.kind ∈ authority f) ∧
    (∀ {e'}, run P fuel h (.call name vs) = (.stuck e', h') →
      ∃ tr, Steps P (.call name vs) tr e' ∧ ∀ eff ∈ tr, eff.kind ∈ authority f) := by
  refine ⟨fun hr => ?_, fun hr => ?_, fun hr => ?_⟩
  · obtain ⟨tr, hs, _⟩ := run_sound.1 hr
    exact ⟨tr, hs, authority_safety hP hf hargs hs⟩
  · obtain ⟨tr, e', hs, hp⟩ := run_sound.2.1 hr
    exact ⟨tr, e', hs, hp, authority_safety hP hf hargs hs⟩
  · obtain ⟨tr, hs, _⟩ := run_sound.2.2 hr
    exact ⟨tr, hs, authority_safety hP hf hargs hs⟩

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
  body := .block [.let_ false (.result .unit) (.builtin .write
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
  have := authority_safety quietProgram_sourceTyped (f := quiet) rfl hargs hrun eff heff
  rw [hk] at this
  simp [authority, paramKinds, quiet, Ty.capKind?, CapKind.below] at this

/-- `fn peek(r: FsRead) { let _ = r.restrict_to(".").read_to_string(Path.from("a.txt")) }`:
it reads a file, and has no capability to write one. -/
def peek : Function where
  name := "peek"
  params := [{ type := .cap .fsRead, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .string) (.builtin .readToString
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
  have hk := authority_safety peekProgram_sourceTyped (f := peek) rfl hargs hrun eff heff
  have hr := steps_receiver hrun eff heff
  simp [authority, paramKinds, peek, Ty.capKind?, CapKind.below] at hk
  constructor <;> intro hop <;> rw [hop] at hr <;> simp [Builtin.receiver] at hr <;>
    rcases hk with hk | hk <;> rw [hk] at hr <;> cases hr

/-- `fn greet(io: Stdout) { let _ = io.println("hi") }`. -/
def greet : Function where
  name := "greet"
  params := [{ type := .cap .stdout, binds := true }]
  ret := .unit
  body := .block [.let_ false (.result .unit) (.builtin .println [.local_ 0, .str "hi"])] none

def greetProgram : Program := { types := [], functions := [greet] }

/-- A function with `Stdout` among its parameters does perform a stdout
effect: the theorem above is not true for want of any. -/
theorem greet_prints :
    ∃ tr e', Steps greetProgram (.call "greet" [.cap .stdout .anywhere]) tr e' ∧
      ∃ eff ∈ tr, eff.kind = .stdout :=
  ⟨_, _, Steps.next (Step.call rfl (AllValues.cons (IsValue.cap _ _) AllValues.nil) rfl)
    (Steps.perform (Step.blockLetArg (Step.builtinEffect rfl
        (AllValues.cons (IsValue.str _) AllValues.nil)))
      (r := .okUnit) (Or.inl rfl) (Steps.refl _)),
    _, List.mem_cons_self, rfl⟩

/-- And the Lean reference, on its fake host, prints `hi`. -/
example : (run greetProgram 10 {} (.call "greet" [.cap .stdout .anywhere])).2.stdout = "hi\n" := by
  decide

end OskaFormal.Core
