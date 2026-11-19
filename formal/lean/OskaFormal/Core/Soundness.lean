import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics

/-!
# Oska Core: progress and preservation

Type soundness of Oska Core, version 2, for a well-typed program `P`:

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

/-- An argument list typed against `ts` has an expression of type `t` at
each index where `ts` has `t`. -/
theorem args_get {Γ : List Ty} :
    ∀ {es : List Expr} {ts : List Ty} {i : Nat} {t : Ty}, Typed P Γ (.args es ts) →
      ts[i]? = some t → ∃ e, es[i]? = some e ∧ Typed P Γ (.expr e t) := by
  intro es
  induction es with
  | nil => intro ts i t h hi; cases h; simp at hi
  | cons e es ih =>
    intro ts i t h hi
    cases h with
    | argsCons he hr =>
      cases i with
      | zero => simp at hi; subst hi; exact ⟨e, rfl, he⟩
      | succ i => simpa using ih hr (by simpa using hi)

theorem AllValues.append {vs ws : List Expr} (h₁ : AllValues vs) (h₂ : AllValues ws) :
    AllValues (vs ++ ws) := by
  induction h₁ with
  | nil => exact h₂
  | cons hv _ ih => exact AllValues.cons hv ih

/-! ## Values -/

/-- A list whose expressions each keep their type from `Γ` to `Δ` keeps
its typing as an argument list. -/
theorem args_transport {Γ Δ : List Ty} :
    ∀ {es : List Expr} {ts : List Ty}, (∀ e ∈ es, ∀ t, Typed P Γ (.expr e t) → Typed P Δ (.expr e t)) →
      Typed P Γ (.args es ts) → Typed P Δ (.args es ts) := by
  intro es
  induction es with
  | nil => intro ts _ h; cases h; exact Typed.argsNil
  | cons e es ih =>
    intro ts hall h
    cases h with
    | argsCons he hr =>
      exact Typed.argsCons (hall e List.mem_cons_self _ he)
        (ih (fun e' he' => hall e' (List.mem_cons_of_mem _ he')) hr)

/-- A value has its type in every context. -/
theorem value_typed {Γ Δ : List Ty} {v : Expr} {t : Ty} (hv : IsValue v)
    (h : Typed P Γ (.expr v t)) : Typed P Δ (.expr v t) := by
  induction hv generalizing t with
  | unit => cases h; exact Typed.unit
  | int _ => cases h with | int hf => exact Typed.int hf
  | bool _ => cases h; exact Typed.bool
  | construct _ ih =>
    cases h with
    | construct hf hargs => exact Typed.construct hf (args_transport (fun e he t => ih e he) hargs)

theorem canonical_int {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .int)) : ∃ n, v = .int n := by
  cases hv with
  | unit => cases h
  | int n => exact ⟨n, rfl⟩
  | bool _ => cases h
  | construct _ => cases h

theorem canonical_bool {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .bool)) : ∃ b, v = .bool b := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool b => exact ⟨b, rfl⟩
  | construct _ => cases h

/-- A value of a struct or enum type is a struct or variant of it, whose
fields are values of its field types. -/
theorem canonical_named {Γ : List Ty} {v : Expr} {type : String} (hv : IsValue v)
    (h : Typed P Γ (.expr v (.named type))) :
    ∃ variant fs fts, v = .construct type variant fs ∧ P.fieldsOf type variant = some fts ∧
      Typed P Γ (.args fs fts) ∧ AllValues fs := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool _ => cases h
  | construct hfs =>
    cases h with
    | construct hf hargs => exact ⟨_, _, _, rfl, hf, hargs, AllValues.of_forall hfs⟩

/-- A name declares one type: when it is a struct, a constructor of it
names no variant. -/
theorem fieldsOf_struct {type : String} {variant : Option String} {fts fts' : List Ty}
    (h : P.fieldsOf type none = some fts) (h' : P.fieldsOf type variant = some fts') :
    variant = none ∧ fts' = fts := by
  unfold Program.fieldsOf at h h'
  cases hd : findType P.types type with
  | none => simp [hd] at h
  | some d =>
    cases d with
    | struct_ _ fs =>
      cases variant with
      | none => simp [hd] at h h'; exact ⟨rfl, h'.symm.trans h⟩
      | some _ => simp [hd] at h'
    | enum_ _ _ => simp [hd] at h

theorem values_valueTyped : ∀ {fs : List Expr} {fts : List Ty},
    (∀ e ∈ fs, ∀ {t : Ty}, Typed P [] (.expr e t) → ValueTyped P (.one e t)) →
    Typed P [] (.args fs fts) → ValueTyped P (.many fs fts) := by
  intro fs
  induction fs with
  | nil => intro fts _ h; cases h; exact ValueTyped.nil
  | cons e es ihs =>
    intro fts ih h
    cases h with
    | argsCons he hr =>
      exact ValueTyped.cons (ih e List.mem_cons_self he)
        (ihs (fun e' he' => ih e' (List.mem_cons_of_mem _ he')) hr)

/-- A closed value of type `t` has type `t` as a value. -/
theorem value_valueTyped {v : Expr} {t : Ty} (hv : IsValue v) (h : Typed P [] (.expr v t)) :
    ValueTyped P (.one v t) := by
  induction hv generalizing t with
  | unit => cases h; exact ValueTyped.unit
  | int _ => cases h with | int hf => exact ValueTyped.int hf
  | bool _ => cases h; exact ValueTyped.bool
  | @construct type variant fs _ ih =>
    cases h with
    | construct hf hargs => exact ValueTyped.construct hf (values_valueTyped ih hargs)

/-! ## Substitution -/

/-- `Expr.subst` on what a judgment is about. -/
def Goal.subst (v : Expr) : Goal → Goal
  | .expr e t => .expr (Expr.subst v e) t
  | .args es ts => .args (Expr.substList v es) ts
  | .block ss val t => .block (Expr.substStmts v ss) (Expr.substOpt v val) t
  | .arms as u t => .arms (Expr.substArms v as) u t

/-- A value is closed: putting a value for a slot leaves it as it is. -/
theorem subst_value {w v : Expr} (hv : IsValue v) : Expr.subst w v = v := by
  induction hv with
  | unit => rfl
  | int _ => rfl
  | bool _ => rfl
  | @construct type variant fs hfs ih =>
    show Expr.construct type variant (Expr.substList w fs) = _
    congr 1
    clear type variant hfs
    induction fs with
    | nil => rfl
    | cons e es ihs =>
      show Expr.subst w e :: Expr.substList w es = e :: es
      rw [ih e List.mem_cons_self, ihs (fun e' he' => ih e' (List.mem_cons_of_mem _ he'))]

/-- Putting a value for a slot keeps an arm with no guard that a value
matches. -/
theorem covered_subst {w v : Expr} : ∀ {arms : List Arm}, Covered arms v →
    Covered (Expr.substArms w arms) v := by
  intro arms
  induction arms with
  | nil => intro ⟨_, _, hm, _⟩; cases hm
  | cons arm arms ih =>
    intro ⟨p, body, hm, hp⟩
    obtain ⟨p', g', b'⟩ := arm
    cases hm with
    | head => exact ⟨p, Expr.subst w body, List.mem_cons_self, hp⟩
    | tail _ hm =>
      obtain ⟨p'', body'', hm', hp'⟩ := ih ⟨p, body, hm, hp⟩
      exact ⟨p'', body'', List.mem_cons_of_mem _ hm', hp'⟩

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
  | construct hf _ ih => intro Δ hΓ; exact Typed.construct hf (ih Δ hΓ)
  | field hf hi _ ih => intro Δ hΓ; exact Typed.field hf hi (ih Δ hΓ)
  | @match_ Γ sc arms u' t _ _ hc ihs iharms =>
    intro Δ hΓ
    refine Typed.match_ (ihs Δ hΓ) (iharms Δ hΓ) ?_
    intro w hw hsw
    refine covered_subst (hc w hw ?_)
    intro hsv
    rw [← subst_value (w := v) hsv]
    exact hsw (by rw [subst_value hsv]; exact hsv)
  | armsNil => intro _ _; exact Typed.armsNil
  | armsCons hp _ _ ihb ihr =>
    intro Δ hΓ
    subst hΓ
    exact Typed.armsCons hp (ihb (Δ ++ _) rfl) (ihr Δ rfl)
  | armsConsGuard hp _ _ _ ihg ihb ihr =>
    intro Δ hΓ
    subst hΓ
    exact Typed.armsConsGuard hp (ihg (Δ ++ _) rfl) (ihb (Δ ++ _) rfl) (ihr Δ rfl)

/-- A function's body, with closed values of its parameters' types put for
the slots they bind, is closed and has the body's type. -/
theorem typed_instantiate :
    ∀ (ps : List Param) {vs : List Expr} {body e : Expr} {t : Ty},
      Typed P (bindingTypes ps) (.expr body t) →
      Typed P [] (.args vs (ps.map Param.type)) → AllValues vs →
      instantiate ps vs body = some e → Typed P [] (.expr e t) := by
  intro ps
  induction ps with
  | nil =>
    intro vs body e t hb hargs _ hi
    cases hargs
    simp only [instantiate, Option.some.injEq] at hi
    subst hi
    exact hb
  | cons p ps ih =>
    intro vs body e t hb hargs hvals hi
    cases hargs with
    | argsCons hv hrest =>
      cases hvals with
      | cons hval hvals =>
        cases hbinds : p.binds with
        | true =>
          simp only [bindingTypes, hbinds] at hb
          simp only [instantiate, hbinds] at hi
          exact ih (typed_subst hval hv hb _ rfl) hrest hvals hi
        | false =>
          simp only [bindingTypes, hbinds] at hb
          simp only [instantiate, hbinds] at hi
          exact ih hb hrest hvals hi

/-- Arguments typed against a function's parameters are as many as its
parameters, so the call can put them in. -/
theorem instantiate_some :
    ∀ (ps : List Param) {Γ : List Ty} {vs : List Expr} {body : Expr},
      Typed P Γ (.args vs (ps.map Param.type)) → ∃ e, instantiate ps vs body = some e := by
  intro ps
  induction ps with
  | nil =>
    intro Γ vs body hargs
    cases hargs
    exact ⟨body, rfl⟩
  | cons p ps ih =>
    intro Γ vs body hargs
    cases hargs with
    | argsCons _ hrest =>
      cases hbinds : p.binds with
      | true =>
        simp only [instantiate, hbinds]
        exact ih hrest
      | false =>
        simp only [instantiate, hbinds]
        exact ih hrest

/-- A body with closed values of its bindings' types put for its first
slots is closed and keeps its type. -/
theorem typed_substAll :
    ∀ (bs : List Expr) {bts : List Ty} {e : Expr} {t : Ty},
      Typed P bts (.expr e t) → Typed P [] (.args bs bts) → AllValues bs →
      Typed P [] (.expr (Expr.substAll bs e) t) := by
  intro bs
  induction bs with
  | nil => intro bts e t he hargs _; cases hargs; exact he
  | cons b bs ih =>
    intro bts e t he hargs hvals
    cases hargs with
    | argsCons hb hrest =>
      cases hvals with
      | cons hv hvs => exact ih (typed_subst hv hb he _ rfl) hrest hvs

/-! ## Patterns -/

/-- What a pattern typing says about matching: a closed value of the
pattern's type that matches it binds closed values of the types of its
bindings. -/
def MatchFor (P : Program) : PGoal → Prop
  | .one p u bts => ∀ {v bs}, IsValue v → Typed P [] (.expr v u) → matchPat p v = some bs →
      Typed P [] (.args bs bts) ∧ AllValues bs
  | .many ps ts bts => ∀ {vs bs}, AllValues vs → Typed P [] (.args vs ts) →
      matchPats ps vs = some bs → Typed P [] (.args bs bts) ∧ AllValues bs

theorem matchPat_typed : ∀ {g : PGoal}, PatTyped P g → MatchFor P g := by
  intro g h
  induction h with
  | wildcard =>
    intro v bs _ _ hm
    simp [matchPat] at hm
    subst hm
    exact ⟨Typed.argsNil, AllValues.nil⟩
  | bind =>
    intro v bs hv ht hm
    simp [matchPat] at hm
    subst hm
    exact ⟨Typed.argsCons ht Typed.argsNil, AllValues.cons hv AllValues.nil⟩
  | unit | int | bool =>
    intro v bs _ _ hm
    cases v <;> simp [matchPat] at hm
    all_goals (obtain ⟨_, rfl⟩ := hm; exact ⟨Typed.argsNil, AllValues.nil⟩)
  | construct hf _ ih =>
    intro v bs hv ht hm
    obtain ⟨variant', fs, fts', rfl, hf', hargs, hvals⟩ := canonical_named hv ht
    simp only [matchPat] at hm
    split at hm
    · next heq =>
      subst heq
      rw [hf] at hf'
      cases hf'
      exact ih hvals hargs hm
    · cases hm
  | nil =>
    intro vs bs _ ht hm
    cases ht
    simp [matchPats] at hm
    subst hm
    exact ⟨Typed.argsNil, AllValues.nil⟩
  | cons _ _ ih₁ ih₂ =>
    intro vs bs hvs ht hm
    cases ht with
    | argsCons he hr =>
      cases hvs with
      | cons hv hvs =>
        simp only [matchPats] at hm
        split at hm
        · next bs₁ bs₂ h₁ h₂ =>
          cases hm
          obtain ⟨t₁, v₁⟩ := ih₁ hv he h₁
          obtain ⟨t₂, v₂⟩ := ih₂ hvs hr h₂
          exact ⟨args_append _ t₁ t₂, v₁.append v₂⟩
        · cases hm

/-- An arm that does not match a value is not the arm it takes. -/
theorem covered_skip {p : Pattern} {g : Option Expr} {body v : Expr} {rest : List Arm}
    (h : Covered (.mk p g body :: rest) v) (hm : matchPat p v = none) : Covered rest v := by
  obtain ⟨p', body', hmem, hp⟩ := h
  cases hmem with
  | head => rw [hm] at hp; cases hp
  | tail _ hmem => exact ⟨p', body', hmem, hp⟩

/-- An arm with a guard is not the arm a value is sure to take. -/
theorem covered_guard {p : Pattern} {g body v : Expr} {rest : List Arm}
    (h : Covered (.mk p (some g) body :: rest) v) : Covered rest v := by
  obtain ⟨p', body', hmem, hp⟩ := h
  cases hmem with
  | tail _ hmem => exact ⟨p', body', hmem, hp⟩

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
  | eq _ _ => simp only [evalBinary, Option.some.injEq] at h; subst h; exact Typed.bool
  | ne _ _ => simp only [evalBinary, Option.some.injEq] at h; subst h; exact Typed.bool
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
  | eq _ _ => exact ⟨_, rfl⟩
  | ne _ _ => exact ⟨_, rfl⟩
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
def ProgressFor (P : Program) : Goal → Prop
  | .expr e _ => IsValue e ∨ ∃ o, Step P e o
  | .args es _ =>
    AllValues es ∨ ∃ vs e rest o, es = vs ++ e :: rest ∧ AllValues vs ∧ Step P e o
  | .block ss val _ => ∃ o, Step P (.block ss val) o
  | .arms _ _ _ => True

theorem progress_goal : ∀ {Γ : List Ty} {j : Goal}, Typed P Γ j → Γ = [] → ProgressFor P j := by
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
  | call hf hargs ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
    · obtain ⟨_, hi⟩ := instantiate_some _ hargs
      exact Or.inr ⟨_, Step.call hf hall hi⟩
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
  | construct _ _ ih =>
    intro hΓ
    rcases ih hΓ with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
    · exact Or.inl (IsValue.construct hall.forall)
    · exact Or.inr ⟨_, Step.constructArg hvs hs⟩
  | field hf hi hb ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hs⟩
    · obtain ⟨variant, fs, fts', rfl, hf', hargs, hvals⟩ := canonical_named hv hb
      obtain ⟨rfl, rfl⟩ := fieldsOf_struct hf hf'
      obtain ⟨e, he, _⟩ := args_get hargs hi
      exact Or.inr ⟨_, Step.field hvals he⟩
    · exact Or.inr ⟨_, Step.fieldBase hs⟩
  | @match_ _ _ arms _ _ hs _ hc ih _ =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hstep⟩
    · obtain ⟨p, body, hmem, _⟩ := hc _ (value_valueTyped hv hs) (fun _ => rfl)
      cases arms with
      | nil => cases hmem
      | cons arm rest =>
        obtain ⟨p, g, body⟩ := arm
        cases hm : matchPat p _ with
        | none => exact Or.inr ⟨_, Step.matchSkip hv hm⟩
        | some bs =>
          cases g with
          | none => exact Or.inr ⟨_, Step.matchArm hv hm⟩
          | some g => exact Or.inr ⟨_, Step.matchGuard hv hm⟩
    · exact Or.inr ⟨_, Step.matchScrutinee hstep⟩
  | armsNil => intro _; trivial
  | armsCons _ _ _ _ _ => intro _; trivial
  | armsConsGuard _ _ _ _ _ _ _ => intro _; trivial

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
  | call hf hvs hi =>
    intro t ht
    cases ht with
    | call hf' hargs =>
      rw [hf] at hf'
      cases hf'
      exact typed_instantiate _ (hP _ _ hf) hargs hvs hi
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
  | constructArg _ _ ih =>
    intro t ht
    cases ht with
    | construct hf hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.construct hf ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | fieldBase _ ih =>
    intro t ht
    cases ht with
    | field hf hi hb => exact (ih hb).map fun _ h => Typed.field hf hi h
  | field _ he =>
    intro t ht
    cases ht with
    | field hf hi hb =>
      cases hb with
      | construct hf' hargs =>
        rw [hf] at hf'
        cases hf'
        obtain ⟨e, he', hte⟩ := args_get hargs hi
        rw [he] at he'
        cases he'
        exact hte
  | matchScrutinee hstep ih =>
    intro t ht
    cases ht with
    | match_ hs harms hc =>
      exact (ih hs).map fun _ h =>
        Typed.match_ h harms fun w hw _ => hc w hw fun hv => absurd hstep (value_no_step hv)
  | @matchSkip _ _ _ _ rest hv hm =>
    intro t ht
    cases ht with
    | @match_ _ _ _ u _ hs harms hc =>
      have hrest : Typed P [] (.arms rest u t) := by
        cases harms with
        | armsCons _ _ hr => exact hr
        | armsConsGuard _ _ _ hr => exact hr
      refine Typed.match_ hs hrest fun w hw hvw => ?_
      have hvw := hvw hv
      subst hvw
      exact covered_skip (hc _ hw fun _ => rfl) hm
  | matchArm hv hm =>
    intro t ht
    cases ht with
    | match_ hs harms hc =>
      cases harms with
      | armsCons hp hbody _ =>
        obtain ⟨hargs, hvals⟩ := matchPat_typed hp hv hs hm
        exact typed_substAll _ (by simpa using hbody) hargs hvals
  | matchGuard hv hm =>
    intro t ht
    cases ht with
    | match_ hs harms hc =>
      cases harms with
      | armsConsGuard hp hg hbody hrest =>
        obtain ⟨hargs, hvals⟩ := matchPat_typed hp hv hs hm
        refine Typed.ifElse (typed_substAll _ (by simpa using hg) hargs hvals)
          (typed_substAll _ (by simpa using hbody) hargs hvals) ?_
        refine Typed.match_ hs hrest fun w hw hvw => ?_
        have hvw := hvw hv
        subst hvw
        exact covered_guard (hc _ hw fun _ => rfl)

end OskaFormal.Core
