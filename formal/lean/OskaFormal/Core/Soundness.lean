import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics

/-!
# Oska Core: progress and preservation

Type soundness of Oska Core, version 4, for a well-typed program `P`:

- `progress`: a closed, well-typed expression is a value or takes a step.
- `preservation`: a step of a closed, well-typed expression leads to an
  expression of the same type, or to a panic (an `Int` overflow or a
  division by zero), or to an effect after which, for every answer the
  semantics allows the host to give, the run goes on at the same type.
- `exhaustiveArms_sound`: a `match` whose typed arms pass the static
  exhaustiveness check takes an arm with no guard for every value of the
  scrutinee's type, which is what progress needs of it.
- `type_safety`: for a program as the checker hands it over
  (`ProgramSourceTyped`), every expression a source-typed expression steps
  to keeps its type and is a value or takes a step.
-/

namespace OskaFormal.Core

variable {P : Program}

/-- A step's outcome is well typed at `t`: a closed expression of type `t`,
or a panic. -/
def OutcomeTyped (P : Program) : Outcome → Ty → Prop
  | .next e, t => Typed P [] (.expr e t)
  | .panic _, _ => True
  | .perform req k, t => ∀ r, req.Allows r → Typed P [] (.expr (k r) t)

theorem OutcomeTyped.map {o : Outcome} {t u : Ty} {f : Expr → Expr}
    (h : OutcomeTyped P o t)
    (hf : ∀ e, Typed P [] (.expr e t) → Typed P [] (.expr (f e) u)) :
    OutcomeTyped P (o.map f) u := by
  cases o with
  | next e => exact hf e h
  | panic _ => trivial
  | perform req k => exact fun r hr => hf _ (h r hr)

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
  | closure =>
    cases h with
    | closure _ hb => exact Typed.closure (Nat.zero_le _) (by simpa using hb)
  | _ => cases h <;> constructor

theorem canonical_int {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .int)) : ∃ n, v = .int n := by
  cases hv with
  | unit => cases h
  | int n => exact ⟨n, rfl⟩
  | bool _ => cases h
  | construct _ => cases h
  | _ => cases h

theorem canonical_bool {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .bool)) : ∃ b, v = .bool b := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool b => exact ⟨b, rfl⟩
  | construct _ => cases h
  | _ => cases h

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
  | _ => cases h

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
  | closure =>
    cases h with
    | closure _ hb => exact ValueTyped.closure (by simpa using hb)
  | _ => cases h <;> constructor

/-- A value of a function type is a closure the run has closed, whose
body has the function's result type in the context of its parameters. -/
theorem canonical_fn {Γ : List Ty} {v : Expr} {ts : List Ty} {t : Ty} (hv : IsValue v)
    (h : Typed P Γ (.expr v (.fn ts t))) :
    ∃ ps cs body, v = .closure ps 0 cs t body ∧ ts = ps.map Param.type ∧
      Typed P (bindingTypes ps) (.expr body t) := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool _ => cases h
  | construct _ => cases h
  | closure ps cs ret body =>
    cases h with
    | closure _ hb => exact ⟨ps, cs, body, rfl, rfl, by simpa using hb⟩
  | _ => cases h

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
  | _ => rfl

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

/-- Putting a value for a slot leaves the patterns and guards' presence of
the arms, so their static exhaustiveness, as they are. -/
theorem unguarded_substArms {w : Expr} : ∀ {arms : List Arm},
    unguarded (Expr.substArms w arms) = unguarded arms := by
  intro arms
  induction arms with
  | nil => rfl
  | cons arm arms ih =>
    obtain ⟨p, g, b⟩ := arm
    cases g <;> simp [Expr.substArms, Expr.substOpt, unguarded, ih]

theorem exhaustiveArms_substArms {w : Expr} {arms : List Arm} :
    exhaustiveArms P (Expr.substArms w arms) = exhaustiveArms P arms := by
  simp [exhaustiveArms, unguarded_substArms]

/-- Putting a closed value of type `u` for slot 0 keeps a judgment in a
context whose slot 0 has type `u`, with that slot removed. -/
theorem typed_subst {v : Expr} {u : Ty} (hv : IsValue v) (hvt : Typed P [] (.expr v u)) :
    ∀ {Γ : List Ty} {j : Goal}, Typed P Γ j → ∀ Δ, Γ = u :: Δ → Typed P Δ (j.subst v) := by
  intro Γ j h
  induction h with
  | unit => intro _ _; exact Typed.unit
  | int hf => intro _ _; exact Typed.int hf
  | bool => intro _ _; exact Typed.bool
  | str => intro _ _; exact Typed.str
  | path => intro _ _; exact Typed.path
  | cap => intro _ _; exact Typed.cap
  | okUnit => intro _ _; exact Typed.okUnit
  | okStr => intro _ _; exact Typed.okStr
  | err => intro _ _; exact Typed.err
  | builtin hb _ ih => intro Δ hΓ; exact Typed.builtin hb (ih Δ hΓ)
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
    rcases hc with he | ⟨hsv, hcov⟩
    · exact Or.inl (by rw [exhaustiveArms_substArms]; exact he)
    · right
      show IsValue (Expr.subst v sc) ∧ Covered (Expr.substArms v arms) (Expr.subst v sc)
      rw [subst_value hsv]
      exact ⟨hsv, covered_subst hcov⟩
  | armsNil => intro _ _; exact Typed.armsNil
  | armsCons hp _ _ ihb ihr =>
    intro Δ hΓ
    subst hΓ
    exact Typed.armsCons hp (ihb (Δ ++ _) rfl) (ihr Δ rfl)
  | armsConsGuard hp _ _ _ ihg ihb ihr =>
    intro Δ hΓ
    subst hΓ
    exact Typed.armsConsGuard hp (ihg (Δ ++ _) rfl) (ihb (Δ ++ _) rfl) (ihr Δ rfl)
  | @closure Γ ps h cs ret body hle hb ih =>
    intro Δ hΓ
    subst hΓ
    cases h with
    | zero =>
      show Typed P Δ (.expr (.closure ps 0 cs ret body) _)
      exact Typed.closure (Nat.zero_le _) (by simpa using hb)
    | succ h =>
      show Typed P Δ (.expr (.closure ps h (shiftCaptures cs) ret (Expr.subst v body)) _)
      exact Typed.closure (by simpa using hle) (ih (Δ.take h ++ bindingTypes ps) (by simp))
  | callValue _ _ ihc iha => intro Δ hΓ; exact Typed.callValue (ihc Δ hΓ) (iha Δ hΓ)

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

/-! ## Static exhaustiveness -/

theorem matchPats_cons {p : Pattern} {ps : List Pattern} {v : Expr} {vs : List Expr} :
    (matchPats (p :: ps) (v :: vs)).isSome = ((matchPat p v).isSome && (matchPats ps vs).isSome) := by
  cases h₁ : matchPat p v <;> cases h₂ : matchPats ps vs <;> simp [matchPats, h₁, h₂]

theorem matchPats_append {qs : List Pattern} {ys : List Expr} :
    ∀ {ps : List Pattern} {xs : List Expr}, ps.length = xs.length →
      (matchPats (ps ++ qs) (xs ++ ys)).isSome = true →
      (matchPats ps xs).isSome = true ∧ (matchPats qs ys).isSome = true := by
  intro ps
  induction ps with
  | nil =>
    intro xs hl h
    cases xs with
    | nil => exact ⟨by simp [matchPats], by simpa using h⟩
    | cons _ _ => simp at hl
  | cons p ps ih =>
    intro xs hl h
    cases xs with
    | nil => simp at hl
    | cons x xs =>
      simp only [List.cons_append, matchPats_cons, Bool.and_eq_true] at h ⊢
      obtain ⟨h₁, h₂⟩ := ih (by simpa using hl) h.2
      exact ⟨⟨h.1, h₁⟩, h₂⟩

theorem catchAll_matches {p : Pattern} {v : Expr} (hp : p.isCatchAll = true) :
    (matchPat p v).isSome = true := by
  cases p <;> simp_all [Pattern.isCatchAll, matchPat]

theorem matchPats_wildcards : ∀ {xs : List Expr},
    (matchPats (List.replicate xs.length .wildcard) xs).isSome = true := by
  intro xs
  induction xs with
  | nil => simp [matchPats]
  | cons x xs ih =>
    simp only [List.length_cons, List.replicate_succ, matchPats_cons, Bool.and_eq_true]
    exact ⟨catchAll_matches rfl, ih⟩

theorem ValueTyped.many_length : ∀ {xs : List Expr} {ts : List Ty},
    ValueTyped P (.many xs ts) → xs.length = ts.length := by
  intro xs
  induction xs with
  | nil => intro ts h; cases h; rfl
  | cons x xs ih => intro ts h; cases h with | cons _ hr => simp [ih hr]

theorem ValueTyped.many_append {ys : List Expr} {us : List Ty} (hy : ValueTyped P (.many ys us)) :
    ∀ {xs : List Expr} {ts : List Ty}, ValueTyped P (.many xs ts) →
      ValueTyped P (.many (xs ++ ys) (ts ++ us)) := by
  intro xs
  induction xs with
  | nil => intro ts h; cases h; exact hy
  | cons x xs ih => intro ts h; cases h with | cons hx hr => exact ValueTyped.cons hx (ih hr)

theorem mem_defaultRows : ∀ {rows : List Row} {row : Row}, row ∈ defaultRows rows →
    ∃ p, p.isCatchAll = true ∧ (p :: row) ∈ rows := by
  intro rows
  induction rows with
  | nil => intro row h; simp [defaultRows] at h
  | cons r rows ih =>
    intro row h
    cases r with
    | nil =>
      obtain ⟨p, hp, hm⟩ := ih (by simpa [defaultRows] using h)
      exact ⟨p, hp, List.mem_cons_of_mem _ hm⟩
    | cons q rest =>
      simp only [defaultRows] at h
      split at h
      · next hq =>
        cases h with
        | head => exact ⟨q, hq, List.mem_cons_self⟩
        | tail _ h =>
          obtain ⟨p, hp, hm⟩ := ih h
          exact ⟨p, hp, List.mem_cons_of_mem _ hm⟩
      · obtain ⟨p, hp, hm⟩ := ih h
        exact ⟨p, hp, List.mem_cons_of_mem _ hm⟩

theorem mem_specialize {h : Head} {k : Nat} : ∀ {rows : List Row} {row : Row},
    row ∈ specialize h k rows → ∃ p rest, (p :: rest) ∈ rows ∧
      ((p.isCatchAll = true ∧ row = List.replicate k .wildcard ++ rest) ∨
        ∃ ps, h.args k p = some ps ∧ row = ps ++ rest) := by
  intro rows
  induction rows with
  | nil => intro row hr; simp [specialize] at hr
  | cons r rows ih =>
    intro row hr
    have lift : (∃ p rest, (p :: rest) ∈ rows ∧
        ((p.isCatchAll = true ∧ row = List.replicate k .wildcard ++ rest) ∨
          ∃ ps, h.args k p = some ps ∧ row = ps ++ rest)) →
        ∃ p rest, (p :: rest) ∈ r :: rows ∧
        ((p.isCatchAll = true ∧ row = List.replicate k .wildcard ++ rest) ∨
          ∃ ps, h.args k p = some ps ∧ row = ps ++ rest) :=
      fun ⟨p, rest, hm, hc⟩ => ⟨p, rest, List.mem_cons_of_mem _ hm, hc⟩
    cases r with
    | nil => exact lift (ih (by simpa [specialize] using hr))
    | cons q rest =>
      simp only [specialize] at hr
      split at hr
      · next hq =>
        cases hr with
        | head => exact ⟨q, rest, List.mem_cons_self, Or.inl ⟨hq, rfl⟩⟩
        | tail _ hr => exact lift (ih hr)
      · split at hr
        · next ps hps =>
          cases hr with
          | head => exact ⟨q, rest, List.mem_cons_self, Or.inr ⟨ps, hps, rfl⟩⟩
          | tail _ hr => exact lift (ih hr)
        · exact lift (ih hr)

theorem Head.args_length {h : Head} {k : Nat} {p : Pattern} {ps : List Pattern}
    (hp : h.args k p = some ps) : ps.length = k := by
  unfold Head.args at hp
  split at hp
  · cases hp; rfl
  · split at hp <;> cases hp; rfl
  · split at hp
    · next hc => cases hp; exact hc.2
    · cases hp
  · cases hp

/-- A row of the specialized rows that matches the fields `xs` and the
rest `vs` gives a row of the rows that matches a value `v` with those
fields, when each pattern with this head matches `v` once its sub-patterns
match `xs`. -/
theorem specialize_sound {h : Head} {k : Nat} {rows : List Row} {row : Row} {v : Expr}
    {xs vs : List Expr} (hrow : row ∈ specialize h k rows) (hk : xs.length = k)
    (hm : (matchPats row (xs ++ vs)).isSome = true)
    (hv : ∀ p ps, h.args k p = some ps → (matchPats ps xs).isSome = true →
      (matchPat p v).isSome = true) :
    ∃ r ∈ rows, (matchPats r (v :: vs)).isSome = true := by
  obtain ⟨p, rest, hmem, ⟨hp, rfl⟩ | ⟨ps, hps, rfl⟩⟩ := mem_specialize hrow
  · obtain ⟨_, hr⟩ := matchPats_append (by simp [hk]) hm
    exact ⟨p :: rest, hmem, by simp [matchPats_cons, catchAll_matches hp, hr]⟩
  · obtain ⟨h₁, hr⟩ := matchPats_append (by rw [Head.args_length hps, hk]) hm
    exact ⟨p :: rest, hmem, by simp [matchPats_cons, hv p ps hps h₁, hr]⟩

theorem lookup_mem {α β : Type} [BEq α] [LawfulBEq α] :
    ∀ {l : List (α × β)} {a : α} {b : β}, l.lookup a = some b → (a, b) ∈ l := by
  intro l
  induction l with
  | nil => intro a b h; simp at h
  | cons x l ih =>
    intro a b h
    obtain ⟨x₁, x₂⟩ := x
    simp only [List.lookup] at h
    split at h
    · next hx =>
      cases h
      have := LawfulBEq.eq_of_beq hx
      subst this
      exact List.mem_cons_self
    · exact List.mem_cons_of_mem _ (ih h)

/-- **Soundness of `covers`.** Rows that cover the types `tys` have a row
that matches every list of values of those types. -/
theorem covers_sound : ∀ (fuel : Nat) {rows : List Row} {tys : List Ty} {vs : List Expr},
    covers P fuel rows tys = true → ValueTyped P (.many vs tys) →
    ∃ row ∈ rows, (matchPats row vs).isSome = true := by
  intro fuel
  induction fuel with
  | zero => intro rows tys vs h; simp [covers] at h
  | succ fuel ih =>
    intro rows tys vs h hvs
    cases tys with
    | nil =>
      cases hvs
      simp only [covers, List.any_eq_true, List.isEmpty_iff] at h
      obtain ⟨row, hrow, rfl⟩ := h
      exact ⟨[], hrow, by simp [matchPats]⟩
    | cons t ts =>
      cases hvs with
      | @cons v vs t ts hv hvs =>
      simp only [covers, Bool.or_eq_true] at h
      rcases h with hd | hc
      · obtain ⟨row, hrow, hm⟩ := ih hd hvs
        obtain ⟨p, hp, hmem⟩ := mem_defaultRows hrow
        exact ⟨p :: row, hmem, by simp [matchPats_cons, catchAll_matches hp, hm]⟩
      · cases t with
        | unit =>
          cases hv
          obtain ⟨row, hrow, hm⟩ := ih hc hvs
          refine specialize_sound (xs := []) hrow rfl hm fun p ps hps _ => ?_
          cases p <;> simp_all [Head.args, matchPat]
        | bool =>
          cases hv with
          | @bool b =>
          simp only [Bool.and_eq_true] at hc
          have hb : covers P fuel (specialize (.bool b) 0 rows) ts = true := by
            cases b
            · exact hc.2
            · exact hc.1
          obtain ⟨row, hrow, hm⟩ := ih hb hvs
          refine specialize_sound (xs := []) hrow rfl hm fun p ps hps _ => ?_
          cases p <;> simp_all [Head.args, matchPat] <;> (split at hps <;> simp_all)
        | int | string | path | cap _ | result _ | fn _ _ => simp at hc
        | named n =>
          cases hv with
          | @construct _ variant fs fts hf hfs =>
          have hlen := ValueTyped.many_length hfs
          have hk : covers P fuel (specialize (.ctor variant) fts.length rows) (fts ++ ts) = true := by
            unfold Program.fieldsOf at hf
            cases hd : findType P.types n with
            | none => simp [hd] at hf
            | some d =>
              cases d with
              | struct_ _ fs' =>
                cases variant with
                | none => simp [hd] at hf hc; subst hf; exact hc
                | some _ => simp [hd] at hf
              | enum_ _ vs' =>
                cases variant with
                | none => simp [hd] at hf
                | some name =>
                  simp only [hd] at hf hc
                  rw [List.all_eq_true] at hc
                  exact hc (name, fts) (lookup_mem hf)
          obtain ⟨row, hrow, hm⟩ := ih hk (ValueTyped.many_append hvs hfs)
          refine specialize_sound hrow hlen hm fun p ps hps hps' => ?_
          cases p <;> simp [Head.args] at hps
          obtain ⟨⟨rfl, _⟩, rfl⟩ := hps
          simp [matchPat, hps']

/-- The patterns of the arms with no guard come from such arms. -/
theorem mem_unguarded : ∀ {arms : List Arm} {p : Pattern}, p ∈ unguarded arms →
    ∃ body, Arm.mk p none body ∈ arms := by
  intro arms
  induction arms with
  | nil => intro p h; simp [unguarded] at h
  | cons arm arms ih =>
    intro p h
    obtain ⟨q, g, b⟩ := arm
    cases g with
    | none =>
      simp only [unguarded, List.mem_cons] at h
      rcases h with rfl | h
      · exact ⟨b, List.mem_cons_self⟩
      · obtain ⟨body, hm⟩ := ih h
        exact ⟨body, List.mem_cons_of_mem _ hm⟩
    | some _ =>
      obtain ⟨body, hm⟩ := ih (by simpa [unguarded] using h)
      exact ⟨body, List.mem_cons_of_mem _ hm⟩

/-- The patterns of typed arms match values of the scrutinee's type. -/
theorem unguarded_typed {Γ : List Ty} {u t : Ty} : ∀ {arms : List Arm} {p : Pattern},
    Typed P Γ (.arms arms u t) → p ∈ unguarded arms → ∃ bs, PatTyped P (.one p u bs) := by
  intro arms
  induction arms with
  | nil => intro p _ h; simp [unguarded] at h
  | cons arm arms ih =>
    intro p ht h
    cases ht with
    | armsCons hp _ hr =>
      simp only [unguarded, List.mem_cons] at h
      rcases h with rfl | h
      · exact ⟨_, hp⟩
      · exact ih hr h
    | armsConsGuard _ _ _ hr => exact ih hr (by simpa [unguarded] using h)

theorem headType_typed {p : Pattern} {u u' : Ty} {bs : List Ty} (hp : PatTyped P (.one p u bs))
    (h : p.headType = some u') : u' = u := by
  cases hp <;> simp_all [Pattern.headType]

theorem headType_none {p : Pattern} (h : p.headType = none) : p.isCatchAll = true := by
  cases p <;> simp_all [Pattern.headType, Pattern.isCatchAll]

/-- **Static exhaustiveness gives coverage.** When typed arms pass
`exhaustiveArms`, every value of the scrutinee's type matches an arm with
no guard: the premise the runtime rule asks of a value scrutinee. -/
theorem exhaustiveArms_sound {Γ : List Ty} {arms : List Arm} {u t : Ty} {v : Expr}
    (he : exhaustiveArms P arms = true) (ht : Typed P Γ (.arms arms u t))
    (hv : ValueTyped P (.one v u)) : Covered arms v := by
  dsimp only [exhaustiveArms] at he
  split at he
  · next hnone =>
    rw [List.findSome?_eq_none_iff] at hnone
    cases hps : unguarded arms with
    | nil => simp [hps] at he
    | cons p ps =>
      have hp : p ∈ unguarded arms := by rw [hps]; exact List.mem_cons_self
      obtain ⟨body, hm⟩ := mem_unguarded hp
      exact ⟨p, body, hm, catchAll_matches (headType_none (hnone p hp))⟩
  · next u' hsome =>
    obtain ⟨q, hq, hqt⟩ := List.exists_of_findSome?_eq_some hsome
    obtain ⟨_, hqtyped⟩ := unguarded_typed ht hq
    have hu := headType_typed hqtyped hqt
    subst hu
    obtain ⟨row, hrow, hm⟩ := covers_sound _ he (ValueTyped.cons hv ValueTyped.nil)
    obtain ⟨p, hp, rfl⟩ := List.mem_map.mp hrow
    obtain ⟨body, hmem⟩ := mem_unguarded hp
    refine ⟨p, body, hmem, ?_⟩
    simp only [matchPats_cons, Bool.and_eq_true] at hm
    exact hm.1

/-- A typed `match` whose scrutinee is a value takes an arm with no guard
for it: by static exhaustiveness, or by the runtime premise. -/
theorem match_covered {s : Expr} {arms : List Arm} {u t : Ty}
    (hc : exhaustiveArms P arms = true ∨ (IsValue s ∧ Covered arms s)) (hs : IsValue s)
    (hst : Typed P [] (.expr s u)) (harms : Typed P [] (.arms arms u t)) : Covered arms s :=
  hc.elim (fun he => exhaustiveArms_sound he harms (value_valueTyped hs hst)) (·.2)

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

/-! ## Built-in calls -/

theorem canonical_cap {Γ : List Ty} {v : Expr} {k : CapKind} (hv : IsValue v)
    (h : Typed P Γ (.expr v (.cap k))) : ∃ r, v = .cap k r := by
  cases hv <;> cases h; exact ⟨_, rfl⟩

theorem canonical_str {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P Γ (.expr v .string)) : ∃ s, v = .str s := by
  cases hv <;> cases h; exact ⟨_, rfl⟩

theorem canonical_result {Γ : List Ty} {v : Expr} {t : Ty} (hv : IsValue v)
    (h : Typed P Γ (.expr v (.result t))) :
    v = .err ∨ (t = .unit ∧ v = .okUnit) ∨ ∃ s, t = .string ∧ v = .okStr s := by
  cases hv <;> cases h
  · exact Or.inr (Or.inl ⟨rfl, rfl⟩)
  · exact Or.inr (Or.inr ⟨_, rfl, rfl⟩)
  · exact Or.inl rfl

/-- A built-in call whose arguments are values of its types is an effect
on a capability of its receiver's kind, or a pure call with a value. -/
theorem builtin_progress {op : Builtin} {ts : List Ty} {t : Ty} {vs : List Expr}
    (hb : BuiltinTyped op ts t) (hargs : Typed P [] (.args vs ts)) (hvs : AllValues vs) :
    ∃ o, Step P (.builtin op vs) o := by
  cases hb with
  | println | readToString | createDir | write =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hvs with
      | cons hv₁ hvs =>
        obtain ⟨r, rfl⟩ := canonical_cap hv₁ h₁
        exact ⟨_, Step.builtinEffect rfl hvs⟩
  | restrictTo hk =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hvs with
      | cons hv₁ hvs =>
        obtain ⟨r, rfl⟩ := canonical_cap hv₁ h₁
        exact ⟨_, Step.builtinEffect (by simp [Builtin.receiver, hk]) hvs⟩
  | readOnly hk | writeOnly hk =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ _ =>
        obtain ⟨r, rfl⟩ := canonical_cap hv₁ h₁
        exact ⟨_, Step.builtinPure (AllValues.cons (IsValue.cap _ _) AllValues.nil)
          (by simp [evalPure, hk]; rfl)⟩
  | pathFrom | pathNew =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ hvs =>
        obtain ⟨s, rfl⟩ := canonical_str hv₁ h₁
        exact ⟨_, Step.builtinPure (AllValues.cons (IsValue.str _) hvs) rfl⟩
  | isOk =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ hvs =>
        rcases canonical_result hv₁ h₁ with rfl | ⟨_, rfl⟩ | ⟨_, _, rfl⟩
        all_goals exact ⟨_, Step.builtinPure (AllValues.cons hv₁ hvs) rfl⟩
  | unwrapOr =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr with
      | argsCons _ hr =>
        cases hr
        cases hvs with
        | cons hv₁ hvs =>
          rcases canonical_result hv₁ h₁ with rfl | ⟨_, rfl⟩ | ⟨_, _, rfl⟩
          all_goals exact ⟨_, Step.builtinPure (AllValues.cons hv₁ hvs) rfl⟩

/-- A pure built-in call on values of its types gives a value of its
result type. -/
theorem evalPure_typed {op : Builtin} {ts : List Ty} {t : Ty} {vs : List Expr} {v : Expr}
    (hb : BuiltinTyped op ts t) (hargs : Typed P [] (.args vs ts)) (hvs : AllValues vs)
    (h : evalPure op vs = some v) : Typed P [] (.expr v t) := by
  cases hb with
  | println | readToString | createDir | write | restrictTo _ =>
    cases hargs with
    | argsCons h₁ _ =>
      cases hvs with
      | cons hv₁ _ =>
        obtain ⟨r, rfl⟩ := canonical_cap hv₁ h₁
        simp [evalPure] at h
  | readOnly hk | writeOnly hk =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ _ =>
        obtain ⟨r, rfl⟩ := canonical_cap hv₁ h₁
        simp [evalPure, hk] at h
        subst h
        exact Typed.cap
  | pathFrom | pathNew =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ _ =>
        obtain ⟨s, rfl⟩ := canonical_str hv₁ h₁
        simp [evalPure] at h
        subst h
        exact Typed.path
  | isOk =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr
      cases hvs with
      | cons hv₁ _ =>
        rcases canonical_result hv₁ h₁ with rfl | ⟨_, rfl⟩ | ⟨_, _, rfl⟩ <;>
          (simp [evalPure] at h; subst h; exact Typed.bool)
  | unwrapOr =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr with
      | argsCons h₂ hr =>
        cases hr
        cases hvs with
        | cons hv₁ _ =>
          rcases canonical_result hv₁ h₁ with rfl | ⟨rfl, rfl⟩ | ⟨_, rfl, rfl⟩ <;>
            simp [evalPure] at h <;> subst h
          · exact h₂
          · exact Typed.unit
          · exact Typed.str

/-- Every answer the semantics allows an effect is a value of the call's
result type. -/
theorem allows_typed {op : Builtin} {k : CapKind} {root : CapRoot} {ts : List Ty} {t : Ty}
    {vs : List Expr} {r : Expr} (hb : BuiltinTyped op (.cap k :: ts) t)
    (hr : Request.Allows ⟨op, k, root, vs⟩ r) : Typed P [] (.expr r t) := by
  cases hb with
  | println | write | createDir =>
    rcases hr with rfl | rfl
    · exact Typed.okUnit
    · exact Typed.err
  | readToString =>
    rcases hr with rfl | ⟨s, rfl⟩
    · exact Typed.err
    · exact Typed.okStr
  | restrictTo hk =>
    obtain ⟨a, k', root', hk', rfl, _⟩ := hr
    rw [hk] at hk'
    cases hk'
    exact Typed.cap
  | readOnly _ | writeOnly _ => exact absurd hr id

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
  | str => intro _; exact Or.inl (IsValue.str _)
  | path => intro _; exact Or.inl (IsValue.path _)
  | cap => intro _; exact Or.inl (IsValue.cap _ _)
  | okUnit => intro _; exact Or.inl IsValue.okUnit
  | okStr => intro _; exact Or.inl (IsValue.okStr _)
  | err => intro _; exact Or.inl IsValue.err
  | builtin hb hargs ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
    · exact Or.inr (builtin_progress hb hargs hall)
    · exact Or.inr ⟨_, Step.builtinArg hvs hs⟩
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
  | @match_ _ _ arms _ _ hs harms hc ih _ =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hstep⟩
    · obtain ⟨p, body, hmem, _⟩ := match_covered hc hv hs harms
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
  | @closure _ ps h cs ret body hle _ _ =>
    intro hΓ
    subst hΓ
    have : h = 0 := by simpa using hle
    subst this
    exact Or.inl (IsValue.closure _ _ _ _)
  | callValue hc hargs ihc iha =>
    intro hΓ
    subst hΓ
    rcases ihc rfl with hcv | ⟨o, hs⟩
    · rcases iha rfl with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
      · obtain ⟨ps, cs, body, rfl, rfl, _⟩ := canonical_fn hcv hc
        obtain ⟨_, hi⟩ := instantiate_some _ hargs
        exact Or.inr ⟨_, Step.callValue hall hi⟩
      · exact Or.inr ⟨_, Step.callValueArg hcv hvs hs⟩
    · exact Or.inr ⟨_, Step.callValueCallee hs⟩
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
  | builtinArg _ _ ih =>
    intro t ht
    cases ht with
    | builtin hb hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.builtin hb ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | builtinPure hvs he =>
    intro t ht
    cases ht with
    | builtin hb hargs => exact evalPure_typed hb hargs hvs he
  | builtinEffect _ _ =>
    intro t ht
    cases ht with
    | builtin hb hargs =>
      cases hargs with
      | argsCons h₁ _ =>
        cases h₁
        exact fun r hr => allows_typed hb hr
  | matchScrutinee hstep ih =>
    intro t ht
    cases ht with
    | match_ hs harms hc =>
      exact (ih hs).map fun _ h =>
        Typed.match_ h harms (hc.elim Or.inl fun ⟨hv, _⟩ => absurd hstep (value_no_step hv))
  | @matchSkip _ _ _ _ rest hv hm =>
    intro t ht
    cases ht with
    | @match_ _ _ _ u _ hs harms hc =>
      have hcov := match_covered hc hv hs harms
      have hrest : Typed P [] (.arms rest u t) := by
        cases harms with
        | armsCons _ _ hr => exact hr
        | armsConsGuard _ _ _ hr => exact hr
      exact Typed.match_ hs hrest (Or.inr ⟨hv, covered_skip hcov hm⟩)
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
      have hcov := match_covered hc hv hs harms
      cases harms with
      | armsConsGuard hp hg hbody hrest =>
        obtain ⟨hargs, hvals⟩ := matchPat_typed hp hv hs hm
        refine Typed.ifElse (typed_substAll _ (by simpa using hg) hargs hvals)
          (typed_substAll _ (by simpa using hbody) hargs hvals) ?_
        exact Typed.match_ hs hrest (Or.inr ⟨hv, covered_guard hcov⟩)
  | callValueCallee _ ih =>
    intro t ht
    cases ht with
    | callValue hc hargs => exact (ih hc).map fun _ h => Typed.callValue h hargs
  | callValueArg _ _ _ ih =>
    intro t ht
    cases ht with
    | callValue hc hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.callValue hc ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | callValue hvs hi =>
    intro t ht
    cases ht with
    | callValue hc hargs =>
      obtain ⟨ps, cs, body, heq, rfl, hb⟩ := canonical_fn (IsValue.closure _ _ _ _) hc
      cases heq
      exact typed_instantiate _ hb hargs hvs hi

/-! ## Type safety -/

/-- **Type safety** of a program as the checker hands it over. In a
program whose functions are source typed, every expression a closed,
source-typed expression steps to is typed at its type, and is a value or
takes a step (to an expression, a panic or an effect): a run never gets
stuck, whatever answers the host gives that the semantics allows. -/
theorem type_safety (hP : ProgramSourceTyped P) {e e' : Expr} {t : Ty} {tr : List Request}
    (he : SourceTyped P [] e t) (hs : Steps P e tr e') :
    Typed P [] (.expr e' t) ∧ (IsValue e' ∨ ∃ o, Step P e' o) := by
  have typed : Typed P [] (.expr e' t) := by
    have := he.1
    clear he
    induction hs with
    | refl => exact this
    | next h _ ih => exact ih (preservation hP.typed h this)
    | perform h hr _ ih => exact ih (preservation hP.typed h this _ hr)
  exact ⟨typed, progress typed⟩

end OskaFormal.Core
