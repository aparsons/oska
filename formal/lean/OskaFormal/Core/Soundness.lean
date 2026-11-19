import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics

/-!
# Oska Core: progress and preservation

Type soundness of Oska Core, version 1, for a well-typed program `P`:

- `progress`: a closed, well-typed expression is a value or takes a step.
- `preservation`: a step of a closed, well-typed expression leads to an
  expression of the same type, or to a panic (an `Int` overflow or a
  division by zero).
-/

namespace OskaFormal.Core

variable {P : Program}

/-- A step's outcome is well typed at `t`: a closed expression of type `t`,
or a panic. -/
def OutcomeTyped (P : Program) : Outcome → Ty → Prop
  | .next e, t => Typed P [] (.expr e t)
  | .panic _, _ => True

theorem OutcomeTyped.map {o : Outcome} {t u : Ty} {f : Expr → Expr}
    (h : OutcomeTyped P o t)
    (hf : ∀ e, Typed P [] (.expr e t) → Typed P [] (.expr (f e) u)) :
    OutcomeTyped P (o.map f) u := by
  cases o with
  | next e => exact hf e h
  | panic _ => trivial

/-! ## Values -/

/-- A value has its type in every context. -/
theorem value_typed {Γ Δ : List Ty} {v : Expr} {t : Ty} (hv : IsValue v)
    (h : Typed P Γ (.expr v t)) : Typed P Δ (.expr v t) := by
  cases hv with
  | unit => cases h; exact Typed.unit
  | int _ => cases h with | int hf => exact Typed.int hf
  | bool _ => cases h; exact Typed.bool

theorem canonical_int {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .int)) : ∃ n, v = .int n := by
  cases hv with
  | unit => cases h
  | int n => exact ⟨n, rfl⟩
  | bool _ => cases h

theorem canonical_bool {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .bool)) : ∃ b, v = .bool b := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool b => exact ⟨b, rfl⟩

/-! ## Substitution -/

/-- `Expr.subst` on what a judgment is about. -/
def Goal.subst (v : Expr) : Goal → Goal
  | .expr e t => .expr (Expr.subst v e) t
  | .args es ts => .args (Expr.substList v es) ts
  | .block ss val t => .block (Expr.substStmts v ss) (Expr.substOpt v val) t

/-- Putting a closed value of type `u` for slot 0 keeps a judgment in a
context whose slot 0 has type `u`, with that slot removed. -/
theorem typed_subst {v : Expr} {u : Ty} (hv : IsValue v) (hvt : Typed P [] (.expr v u)) :
    ∀ {Γ : List Ty} {j : Goal}, Typed P Γ j → ∀ Δ, Γ = u :: Δ → Typed P Δ (j.subst v) := by
  intro Γ j h
  induction h with
  | unit => intro _ _; exact Typed.unit
  | int hf => intro _ _; exact Typed.int hf
  | bool => intro _ _; exact Typed.bool
  | @local_ Γ i t hl =>
    intro Δ hΓ
    subst hΓ
    cases i with
    | zero =>
      simp only [lookup, Option.some.injEq] at hl
      subst hl
      exact value_typed hv hvt
    | succ i =>
      simp only [lookup] at hl
      exact Typed.local_ hl
  | neg _ ih => intro Δ hΓ; exact Typed.neg (ih Δ hΓ)
  | not _ ih => intro Δ hΓ; exact Typed.not (ih Δ hΓ)
  | binary hop _ _ ihl ihr => intro Δ hΓ; exact Typed.binary hop (ihl Δ hΓ) (ihr Δ hΓ)
  | call hf _ ih => intro Δ hΓ; exact Typed.call hf (ih Δ hΓ)
  | ifElse _ _ _ ihc iht ihe =>
    intro Δ hΓ; exact Typed.ifElse (ihc Δ hΓ) (iht Δ hΓ) (ihe Δ hΓ)
  | ifNoElse _ _ ihc iht => intro Δ hΓ; exact Typed.ifNoElse (ihc Δ hΓ) (iht Δ hΓ)
  | block _ ih => intro Δ hΓ; exact Typed.block (ih Δ hΓ)
  | argsNil => intro _ _; exact Typed.argsNil
  | argsCons _ _ ihe ihes => intro Δ hΓ; exact Typed.argsCons (ihe Δ hΓ) (ihes Δ hΓ)
  | blockNone => intro _ _; exact Typed.blockNone
  | blockSome _ ih => intro Δ hΓ; exact Typed.blockSome (ih Δ hΓ)
  | blockExpr _ _ ihe ihs => intro Δ hΓ; exact Typed.blockExpr (ihe Δ hΓ) (ihs Δ hΓ)
  | blockLetBind _ _ ihe ihs =>
    intro Δ hΓ
    subst hΓ
    exact Typed.blockLetBind (ihe Δ rfl) (ihs (Δ ++ [_]) rfl)
  | blockLetDrop _ _ ihe ihs => intro Δ hΓ; exact Typed.blockLetDrop (ihe Δ hΓ) (ihs Δ hΓ)

/-- A function's body, with closed values of its parameters' types put for
the slots they bind, is closed and has the body's type. -/
theorem typed_instantiate :
    ∀ (ps : List Param) {vs : List Expr} {body : Expr} {t : Ty},
      Typed P (bindingTypes ps) (.expr body t) →
      Typed P [] (.args vs (ps.map Param.type)) → AllValues vs →
      Typed P [] (.expr (instantiate ps vs body) t) := by
  intro ps
  induction ps with
  | nil =>
    intro vs body t hb hargs _
    cases hargs
    exact hb
  | cons p ps ih =>
    intro vs body t hb hargs hvals
    cases hargs with
    | argsCons hv hrest =>
      cases hvals with
      | cons hval hvals =>
        cases hbinds : p.binds with
        | true =>
          simp only [bindingTypes, hbinds] at hb
          simp only [instantiate, hbinds]
          exact ih (typed_subst hval hv hb _ rfl) hrest hvals
        | false =>
          simp only [bindingTypes, hbinds] at hb
          simp only [instantiate, hbinds]
          exact ih hb hrest hvals

/-! ## Argument lists -/

theorem args_split {Γ : List Ty} {e : Expr} {rest : List Expr} :
    ∀ (vs : List Expr) {ts : List Ty}, Typed P Γ (.args (vs ++ e :: rest) ts) →
      ∃ ts₁ t ts₂, ts = ts₁ ++ t :: ts₂ ∧ Typed P Γ (.args vs ts₁) ∧
        Typed P Γ (.expr e t) ∧ Typed P Γ (.args rest ts₂) := by
  intro vs
  induction vs with
  | nil =>
    intro ts h
    rw [List.nil_append] at h
    cases h with
    | argsCons he hr => exact ⟨[], _, _, rfl, Typed.argsNil, he, hr⟩
  | cons v vs ih =>
    intro ts h
    rw [List.cons_append] at h
    cases h with
    | argsCons hv hr =>
      obtain ⟨ts₁, t, ts₂, hts, h₁, he, h₂⟩ := ih hr
      subst hts
      exact ⟨_ :: ts₁, t, ts₂, rfl, Typed.argsCons hv h₁, he, h₂⟩

theorem args_append {Γ : List Ty} {ws : List Expr} {ts₂ : List Ty} :
    ∀ (vs : List Expr) {ts₁ : List Ty}, Typed P Γ (.args vs ts₁) →
      Typed P Γ (.args ws ts₂) → Typed P Γ (.args (vs ++ ws) (ts₁ ++ ts₂)) := by
  intro vs
  induction vs with
  | nil => intro ts₁ h₁ h₂; cases h₁; exact h₂
  | cons v vs ih =>
    intro ts₁ h₁ h₂
    cases h₁ with
    | argsCons hv hr => exact Typed.argsCons hv (ih hr h₂)

/-! ## Operators -/

theorem intResult_typed {n : Int} : OutcomeTyped P (intResult n) .int := by
  unfold intResult
  cases hf : fits n with
  | true => exact Typed.int hf
  | false => trivial

/-- An operator that evaluates both operands, applied to values of its
operand type, gives a result of its result type, or panics. -/
theorem evalBinary_typed {op : BinOp} {a t : Ty} {l r : Expr} {o : Outcome}
    (hop : BinOpTyped op a t) (hl : IsValue l) (hr : IsValue r)
    (htl : Typed P [] (.expr l a)) (htr : Typed P [] (.expr r a))
    (h : evalBinary op l r = some o) : OutcomeTyped P o t := by
  cases hop with
  | eq _ => simp only [evalBinary, Option.some.injEq] at h; subst h; exact Typed.bool
  | ne _ => simp only [evalBinary, Option.some.injEq] at h; subst h; exact Typed.bool
  | and => cases h
  | or => cases h
  | implies => cases h
  | add | sub | mul | lt | le | gt | ge =>
    obtain ⟨x, rfl⟩ := canonical_int hl htl
    obtain ⟨y, rfl⟩ := canonical_int hr htr
    simp only [evalBinary, Option.some.injEq] at h
    subst h
    first | exact intResult_typed | exact Typed.bool
  | div | rem =>
    obtain ⟨x, rfl⟩ := canonical_int hl htl
    obtain ⟨y, rfl⟩ := canonical_int hr htr
    simp only [evalBinary, Option.some.injEq] at h
    subst h
    split
    · trivial
    · exact intResult_typed

/-- An operator that evaluates both operands has a result on values of its
operand type. -/
theorem evalBinary_some {op : BinOp} {a t : Ty} {l r : Expr}
    (hop : BinOpTyped op a t) (hlog : op.isLogical = false) (hl : IsValue l) (hr : IsValue r)
    (htl : Typed P [] (.expr l a)) (htr : Typed P [] (.expr r a)) :
    ∃ o, evalBinary op l r = some o := by
  cases hop with
  | eq _ => exact ⟨_, rfl⟩
  | ne _ => exact ⟨_, rfl⟩
  | and => cases hlog
  | or => cases hlog
  | implies => cases hlog
  | add | sub | mul | div | rem | lt | le | gt | ge =>
    obtain ⟨x, rfl⟩ := canonical_int hl htl
    obtain ⟨y, rfl⟩ := canonical_int hr htr
    exact ⟨_, rfl⟩

/-! ## Progress -/

/-- What progress says about a closed judgment: an expression is a value
or steps, an argument list is all values or has a first argument that is
not a value and steps, and a block steps. -/
def Progress (P : Program) : Goal → Prop
  | .expr e _ => IsValue e ∨ ∃ o, Step P e o
  | .args es _ =>
    AllValues es ∨ ∃ vs e rest o, es = vs ++ e :: rest ∧ AllValues vs ∧ Step P e o
  | .block ss val _ => ∃ o, Step P (.block ss val) o

theorem progress_goal : ∀ {Γ : List Ty} {j : Goal}, Typed P Γ j → Γ = [] → Progress P j := by
  intro Γ j h
  induction h with
  | unit => intro _; exact Or.inl IsValue.unit
  | int _ => intro _; exact Or.inl (IsValue.int _)
  | bool => intro _; exact Or.inl (IsValue.bool _)
  | local_ hl =>
    intro hΓ
    subst hΓ
    simp [lookup] at hl
  | neg he ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hs⟩
    · obtain ⟨n, rfl⟩ := canonical_int hv he
      exact Or.inr ⟨_, Step.neg⟩
    · exact Or.inr ⟨_, Step.negArg hs⟩
  | not he ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hs⟩
    · obtain ⟨b, rfl⟩ := canonical_bool hv he
      exact Or.inr ⟨_, Step.not⟩
    · exact Or.inr ⟨_, Step.notArg hs⟩
  | @binary Γ op l r a t hop hl hr ihl ihr =>
    intro hΓ
    subst hΓ
    rcases ihl rfl with hlv | ⟨o, hs⟩
    · cases hlog : op.isLogical with
      | false =>
        rcases ihr rfl with hrv | ⟨o, hs⟩
        · obtain ⟨o, he⟩ := evalBinary_some hop hlog hlv hrv hl hr
          exact Or.inr ⟨_, Step.binary hlv hrv he⟩
        · exact Or.inr ⟨_, Step.binaryRhs hlog hlv hs⟩
      | true =>
        cases hop with
        | and =>
          obtain ⟨b, rfl⟩ := canonical_bool hlv hl
          cases b
          · exact Or.inr ⟨_, Step.andFalse⟩
          · exact Or.inr ⟨_, Step.andTrue⟩
        | or =>
          obtain ⟨b, rfl⟩ := canonical_bool hlv hl
          cases b
          · exact Or.inr ⟨_, Step.orFalse⟩
          · exact Or.inr ⟨_, Step.orTrue⟩
        | implies =>
          obtain ⟨b, rfl⟩ := canonical_bool hlv hl
          cases b
          · exact Or.inr ⟨_, Step.impliesFalse⟩
          · exact Or.inr ⟨_, Step.impliesTrue⟩
        | _ => simp [BinOp.isLogical] at hlog
    · exact Or.inr ⟨_, Step.binaryLhs hs⟩
  | call hf _ ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
    · exact Or.inr ⟨_, Step.call hf hall⟩
    · exact Or.inr ⟨_, Step.callArg hvs hs⟩
  | ifElse hc _ _ ihc _ _ =>
    intro hΓ
    subst hΓ
    rcases ihc rfl with hv | ⟨o, hs⟩
    · obtain ⟨b, rfl⟩ := canonical_bool hv hc
      cases b
      · exact Or.inr ⟨_, Step.ifFalse⟩
      · exact Or.inr ⟨_, Step.ifTrue⟩
    · exact Or.inr ⟨_, Step.ifCond hs⟩
  | ifNoElse hc _ ihc _ =>
    intro hΓ
    subst hΓ
    rcases ihc rfl with hv | ⟨o, hs⟩
    · obtain ⟨b, rfl⟩ := canonical_bool hv hc
      cases b
      · exact Or.inr ⟨_, Step.ifFalseNoElse⟩
      · exact Or.inr ⟨_, Step.ifTrueNoElse⟩
    · exact Or.inr ⟨_, Step.ifCond hs⟩
  | block _ ih => intro hΓ; exact Or.inr (ih hΓ)
  | argsNil => intro _; exact Or.inl AllValues.nil
  | argsCons _ _ ihe ihes =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · rcases ihes hΓ with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
      · exact Or.inl (AllValues.cons hv hall)
      · exact Or.inr ⟨_ :: vs, e, rest, o, rfl, AllValues.cons hv hvs, hs⟩
    · exact Or.inr ⟨[], _, _, o, rfl, AllValues.nil, hs⟩
  | blockNone => intro _; exact ⟨_, Step.blockNone⟩
  | blockSome _ _ => intro _; exact ⟨_, Step.blockSome⟩
  | blockExpr _ _ ihe _ =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact ⟨_, Step.blockExpr hv⟩
    · exact ⟨_, Step.blockExprArg hs⟩
  | blockLetBind _ _ ihe _ =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact ⟨_, Step.blockLetBind hv⟩
    · exact ⟨_, Step.blockLetArg hs⟩
  | blockLetDrop _ _ ihe _ =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact ⟨_, Step.blockLetDrop hv⟩
    · exact ⟨_, Step.blockLetArg hs⟩

/-- **Progress.** A closed, well-typed expression of Oska Core is a value
or takes a step, to an expression or to a panic. -/
theorem progress {e : Expr} {t : Ty} (h : Typed P [] (.expr e t)) :
    IsValue e ∨ ∃ o, Step P e o :=
  progress_goal h rfl

/-! ## Preservation -/

/-- **Preservation.** In a well-typed program, a step of a closed
expression of type `t` leads to a closed expression of type `t`, or to a
panic. -/
theorem preservation (hP : ProgramTyped P) {e : Expr} {o : Outcome} (hs : Step P e o) :
    ∀ {t : Ty}, Typed P [] (.expr e t) → OutcomeTyped P o t := by
  induction hs with
  | negArg _ ih =>
    intro t ht
    cases ht with
    | neg he => exact (ih he).map fun _ h => Typed.neg h
  | neg =>
    intro t ht
    cases ht with
    | neg _ => exact intResult_typed
  | notArg _ ih =>
    intro t ht
    cases ht with
    | not he => exact (ih he).map fun _ h => Typed.not h
  | not =>
    intro t ht
    cases ht with
    | not _ => exact Typed.bool
  | binaryLhs _ ih =>
    intro t ht
    cases ht with
    | binary hop hl hr => exact (ih hl).map fun _ h => Typed.binary hop h hr
  | binaryRhs _ _ _ ih =>
    intro t ht
    cases ht with
    | binary hop hl hr => exact (ih hr).map fun _ h => Typed.binary hop hl h
  | binary hl hr he =>
    intro t ht
    cases ht with
    | binary hop htl htr => exact evalBinary_typed hop hl hr htl htr he
  | andFalse | orTrue | impliesFalse =>
    intro t ht
    cases ht with
    | binary hop _ _ => cases hop; exact Typed.bool
  | andTrue | orFalse | impliesTrue =>
    intro t ht
    cases ht with
    | binary hop _ hr => cases hop; exact hr
  | callArg _ _ ih =>
    intro t ht
    cases ht with
    | call hf hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.call hf ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | call hf hvs =>
    intro t ht
    cases ht with
    | call hf' hargs =>
      rw [hf] at hf'
      cases hf'
      exact typed_instantiate _ (hP _ _ hf) hargs hvs
  | ifCond _ ih =>
    intro t ht
    cases ht with
    | ifElse hc hth hel => exact (ih hc).map fun _ h => Typed.ifElse h hth hel
    | ifNoElse hc hth => exact (ih hc).map fun _ h => Typed.ifNoElse h hth
  | ifTrue =>
    intro t ht
    cases ht with
    | ifElse _ hth _ => exact hth
  | ifFalse =>
    intro t ht
    cases ht with
    | ifElse _ _ hel => exact hel
  | ifTrueNoElse =>
    intro t ht
    cases ht with
    | ifNoElse _ hth => exact Typed.block (Typed.blockExpr hth Typed.blockNone)
  | ifFalseNoElse =>
    intro t ht
    cases ht with
    | ifNoElse _ _ => exact Typed.unit
  | blockNone =>
    intro t ht
    cases ht with
    | block hb => cases hb; exact Typed.unit
  | blockSome =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockSome he => exact he
  | blockExprArg _ ih =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockExpr he hrest => exact (ih he).map fun _ h => Typed.block (Typed.blockExpr h hrest)
  | blockExpr _ =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockExpr _ hrest => exact Typed.block hrest
  | blockLetArg _ ih =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetBind he hrest =>
        exact (ih he).map fun _ h => Typed.block (Typed.blockLetBind h hrest)
      | blockLetDrop he hrest =>
        exact (ih he).map fun _ h => Typed.block (Typed.blockLetDrop h hrest)
  | blockLetBind hv =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetBind he hrest => exact Typed.block (typed_subst hv he hrest [] rfl)
  | blockLetDrop _ =>
    intro t ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetDrop _ hrest => exact Typed.block hrest

end OskaFormal.Core
