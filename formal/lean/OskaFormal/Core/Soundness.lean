import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics

/-!
# Oska Core: progress and preservation

Type soundness of Oska Core, version 6, for a well-typed program `P`:

- `progress`: a closed, well-typed expression is a value or takes a step.
- `preservation`: a step of a closed, well-typed expression leads to an
  expression of the same type, or to a panic (an `Int` overflow or a
  division by zero), or to an effect after which, for every answer the
  semantics allows the host to give, the run goes on at the same type,
  or to a return (`?` on an `Err`) of a value of the result type of the
  function or closure the expression is in, which its `frame` then
  gives at the frame's type.
- `exhaustiveArms_sound`: a `match` whose typed arms pass the static
  exhaustiveness check takes an arm with no guard for every value of the
  scrutinee's type, which is what progress needs of it.
- `type_safety`: for a program as the checker hands it over
  (`ProgramSourceTyped`), every expression a source-typed expression steps
  to keeps its type and is a value or takes a step.

Value semantics of mutating methods, over whole runs, is in
`ValueSemantics.lean`.
-/

namespace OskaFormal.Core

variable {P : Program} {R : Ty} {M : List Ty}

/-- A step's outcome is well typed at `t`, in a function or closure whose
result type is `R`, with the vars `M`: a closed expression of type `t`, a
panic, an effect after which the run goes on at type `t` for every answer
the semantics allows, a return of a value of type `R`, a read of a place
of `M` after which the run goes on at type `t` for every value of the
place's type, or a write to a place of `M` of a value of its type, after
which the run goes on at type `t`. -/
def OutcomeTyped (P : Program) (R : Ty) (M : List Ty) : Outcome → Ty → Prop
  | .next e, t => Typed P R [] M (.expr e t)
  | .panic _, _ => True
  | .perform req k, t => ∀ r, req.Allows r → Typed P R [] M (.expr (k r) t)
  | .ret v, _ => IsValue v ∧ Typed P R [] M (.expr v R)
  | .read i is k, t => ∃ u, P.placeTy M ⟨i, is⟩ = some u ∧
      ∀ w, IsValue w → Typed P R [] M (.expr w u) → Typed P R [] M (.expr (k w) t)
  | .write i is w e, t => ∃ u, P.placeTy M ⟨i, is⟩ = some u ∧ IsValue w ∧
      Typed P R [] M (.expr w u) ∧ Typed P R [] M (.expr e t)

theorem OutcomeTyped.map {o : Outcome} {t u : Ty} {f : Expr → Expr}
    (h : OutcomeTyped P R M o t)
    (hf : ∀ e, Typed P R [] M (.expr e t) → Typed P R [] M (.expr (f e) u)) :
    OutcomeTyped P R M (o.map f) u := by
  cases o with
  | next e => exact hf e h
  | panic _ => trivial
  | perform req k => exact fun r hr => hf _ (h r hr)
  | ret v => exact h
  | read i is k =>
    obtain ⟨w, hw, hk⟩ := h
    exact ⟨w, hw, fun r hr hrt => hf _ (hk r hr hrt)⟩
  | write i is w e =>
    obtain ⟨u, hu, hw, hwt, he⟩ := h
    exact ⟨u, hu, hw, hwt, hf _ he⟩

/-! ## Argument lists -/

theorem args_split {Γ : List Ty} {e : Expr} {rest : List Expr} :
    ∀ (vs : List Expr) {ts : List Ty}, Typed P R Γ M (.args (vs ++ e :: rest) ts) →
      ∃ ts₁ t ts₂, ts = ts₁ ++ t :: ts₂ ∧ Typed P R Γ M (.args vs ts₁) ∧
        Typed P R Γ M (.expr e t) ∧ Typed P R Γ M (.args rest ts₂) := by
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
    ∀ (vs : List Expr) {ts₁ : List Ty}, Typed P R Γ M (.args vs ts₁) →
      Typed P R Γ M (.args ws ts₂) → Typed P R Γ M (.args (vs ++ ws) (ts₁ ++ ts₂)) := by
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
    ∀ {es : List Expr} {ts : List Ty} {i : Nat} {t : Ty}, Typed P R Γ M (.args es ts) →
      ts[i]? = some t → ∃ e, es[i]? = some e ∧ Typed P R Γ M (.expr e t) := by
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
theorem args_transport {R' : Ty} {Γ Δ M' : List Ty} :
    ∀ {es : List Expr} {ts : List Ty},
      (∀ e ∈ es, ∀ t, Typed P R Γ M (.expr e t) → Typed P R' Δ M' (.expr e t)) →
      Typed P R Γ M (.args es ts) → Typed P R' Δ M' (.args es ts) := by
  intro es
  induction es with
  | nil => intro ts _ h; cases h; exact Typed.argsNil
  | cons e es ih =>
    intro ts hall h
    cases h with
    | argsCons he hr =>
      exact Typed.argsCons (hall e List.mem_cons_self _ he)
        (ih (fun e' he' => hall e' (List.mem_cons_of_mem _ he')) hr)

/-- A value has its type in every context, with any vars, and in a
function or closure of any result type. -/
theorem value_typed {R' : Ty} {Γ Δ M' : List Ty} {v : Expr} {t : Ty} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v t)) : Typed P R' Δ M' (.expr v t) := by
  induction hv generalizing t with
  | unit => cases h; exact Typed.unit
  | int _ => cases h with | int hf => exact Typed.int hf
  | bool _ => cases h; exact Typed.bool
  | result _ _ ih =>
    cases h with
    | ok hf he => exact Typed.ok hf (ih he)
    | err hf he => exact Typed.err hf (ih he)
  | construct _ ih =>
    cases h with
    | construct hf hargs => exact Typed.construct hf (args_transport (fun e he t => ih e he) hargs)
  | closure =>
    cases h with
    | closure _ _ hcaps hb =>
      exact Typed.closure (Nat.zero_le _) (fun _ i _ hi => absurd hi (Nat.not_lt_zero i)) hcaps
        (by simpa using hb)
  | _ => cases h <;> constructor

theorem canonical_int {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v .int)) : ∃ n, v = .int n := by
  cases hv with
  | unit => cases h
  | int n => exact ⟨n, rfl⟩
  | bool _ => cases h
  | construct _ => cases h
  | _ => cases h

theorem canonical_bool {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v .bool)) : ∃ b, v = .bool b := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool b => exact ⟨b, rfl⟩
  | construct _ => cases h
  | _ => cases h

/-- A value of a struct or enum type is a struct or variant of it, whose
fields are values of its field types. -/
theorem canonical_named {Γ : List Ty} {v : Expr} {type : String} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v (.named type))) :
    ∃ variant fs fts, v = .construct type variant fs ∧ P.fieldsOf type variant = some fts ∧
      Typed P R Γ M (.args fs fts) ∧ AllValues fs := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool _ => cases h
  | construct hfs =>
    cases h with
    | construct hf hargs => exact ⟨_, _, _, rfl, hf, hargs, AllValues.of_forall hfs⟩
  | _ => cases h

/-- A value of a `Result` type is an `Ok` of a value of its `Ok` type or
an `Err` of a value of its `Err` type. -/
theorem canonical_result {Γ : List Ty} {v : Expr} {t u : Ty} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v (.result t u))) :
    (∃ w, v = .result true w ∧ IsValue w ∧ Typed P R Γ M (.expr w t)) ∨
      (∃ w, v = .result false w ∧ IsValue w ∧ Typed P R Γ M (.expr w u)) := by
  cases hv with
  | result _ hw =>
    cases h with
    | ok _ ht => exact Or.inl ⟨_, rfl, hw, ht⟩
    | err _ ht => exact Or.inr ⟨_, rfl, hw, ht⟩
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
    (∀ e ∈ fs, ∀ {t : Ty}, Typed P R [] M (.expr e t) → ValueTyped P (.one e t)) →
    Typed P R [] M (.args fs fts) → ValueTyped P (.many fs fts) := by
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
theorem value_valueTyped {v : Expr} {t : Ty} (hv : IsValue v) (h : Typed P R [] M (.expr v t)) :
    ValueTyped P (.one v t) := by
  induction hv generalizing t with
  | unit => cases h; exact ValueTyped.unit
  | int _ => cases h with | int hf => exact ValueTyped.int hf
  | bool _ => cases h; exact ValueTyped.bool
  | result _ _ ih =>
    cases h with
    | ok hf he => exact ValueTyped.ok hf (ih he)
    | err hf he => exact ValueTyped.err hf (ih he)
  | @construct type variant fs _ ih =>
    cases h with
    | construct hf hargs => exact ValueTyped.construct hf (values_valueTyped ih hargs)
  | closure =>
    cases h with
    | closure _ _ hcaps hb => exact ValueTyped.closure hcaps (by simpa using hb)
  | _ => cases h <;> constructor

/-- A value of a function type is a closure the run has closed, whose
body has the function's result type in the context of its parameters. -/
theorem canonical_fn {Γ : List Ty} {v : Expr} {cf : Bool} {ts : List Ty} {t : Ty}
    (hv : IsValue v) (h : Typed P R Γ M (.expr v (.fn cf ts t))) :
    ∃ ps cs body, v = .closure ps 0 cs cf t body ∧ ts = ps.map Param.type ∧
      Typed P t (bindingTypes ps) [] (.expr body t) := by
  cases hv with
  | unit => cases h
  | int _ => cases h
  | bool _ => cases h
  | construct _ => cases h
  | closure ps cs _ ret body =>
    cases h with
    | closure _ _ _ hb => exact ⟨ps, cs, body, rfl, rfl, by simpa using hb⟩
  | _ => cases h

/-! ## Capabilities a value holds -/

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

theorem findType_mem : ∀ {ds : List TypeDecl} {n : String} {d : TypeDecl},
    findType ds n = some d → d ∈ ds := by
  intro ds
  induction ds with
  | nil => intro n d h; simp [findType] at h
  | cons d ds ih =>
    intro n d' h
    simp only [findType] at h
    split at h
    · cases h; exact List.mem_cons_self
    · exact List.mem_cons_of_mem _ (ih h)

/-- In a program whose structs and enums hold no capability or function,
every field type of a struct or variant is such a type. -/
theorem fieldsOf_capFree (hT : P.typesCapFree = true) {type : String} {variant : Option String}
    {fts : List Ty} (hf : P.fieldsOf type variant = some fts) : ∀ t ∈ fts, t.fieldCapFree = true := by
  intro t ht
  unfold Program.fieldsOf at hf
  unfold Program.typesCapFree at hT
  rw [List.all_eq_true] at hT
  split at hf
  · next fs' hd =>
    cases hf
    have := hT _ (findType_mem hd)
    simp only [List.all_eq_true] at this
    exact this t ht
  · next vs' v hd =>
    have := hT _ (findType_mem hd)
    simp only [List.all_eq_true] at this
    exact this _ (lookup_mem hf) t ht
  · cases hf

/-- A field type, which holds no capability or function, is a type whose
values hold no capability, in a program whose structs and enums hold
none. -/
theorem fieldCapFree_pureTy (hT : P.typesCapFree = true) :
    ∀ {t : Ty}, t.fieldCapFree = true → P.pureTy t = true
  | .int, _ | .bool, _ | .unit, _ | .string, _ | .path, _ | .ioError, _ => rfl
  | .named _, _ => by simp [Program.pureTy, hT]
  | .result a b, h => by
    simp only [Ty.fieldCapFree, Bool.and_eq_true] at h
    simp only [Program.pureTy, fieldCapFree_pureTy hT h.1, fieldCapFree_pureTy hT h.2, Bool.and_self]
  | .cap _, h | .fn _ _ _, h => by simp [Ty.fieldCapFree] at h

/-- A value of a type whose values hold no capability holds none: a plain
`fn` by its typing, a struct or variant by its fields, an `Ok` or `Err`
by the value inside. -/
theorem valueTyped_pure : ∀ {g : VGoal}, ValueTyped P g → match g with
    | .one v t => P.pureTy t = true → v.capsWithin [] = true
    | .many vs ts => P.typesCapFree = true → (∀ t ∈ ts, t.fieldCapFree = true) →
      Expr.capsWithinList [] vs = true := by
  intro g h
  induction h with
  | cap => intro h; simp [Program.pureTy] at h
  | @closure ps cs cf ret body hcaps _ =>
    intro h
    cases cf with
    | false => exact hcaps rfl
    | true => simp [Program.pureTy] at h
  | construct hf _ ih =>
    intro hT
    simp only [Program.pureTy] at hT
    simpa [Expr.capsWithin] using ih hT (fieldsOf_capFree hT hf)
  | nil => intro _ _; rfl
  | @cons v vs t ts _ _ ih₁ ih₂ =>
    intro hT hts
    have ht := hts t List.mem_cons_self
    simp only [Expr.capsWithinList, Bool.and_eq_true]
    exact ⟨ih₁ (fieldCapFree_pureTy hT ht), ih₂ hT fun t' h' => hts t' (List.mem_cons_of_mem _ h')⟩
  | ok _ _ ih =>
    intro h
    simp only [Program.pureTy, Bool.and_eq_true] at h
    simpa [Expr.capsWithin] using ih h.1
  | err _ _ ih =>
    intro h
    simp only [Program.pureTy, Bool.and_eq_true] at h
    simpa [Expr.capsWithin] using ih h.2
  | _ => intro _; rfl

/-! ## The slots an expression reads -/

theorem readsList_values {i : Nat} :
    ∀ {vs : List Expr}, (∀ e ∈ vs, IsValue e → e.reads i = false) → (∀ e ∈ vs, IsValue e) →
      Expr.readsList i vs = false := by
  intro vs
  induction vs with
  | nil => intro _ _; rfl
  | cons e es ih =>
    intro h hv
    simp only [Expr.readsList, Bool.or_eq_false_iff]
    exact ⟨h e List.mem_cons_self (hv e List.mem_cons_self),
      ih (fun e' he' => h e' (List.mem_cons_of_mem _ he')) (fun e' he' => hv e' (List.mem_cons_of_mem _ he'))⟩

/-- A value reads no slot: it is closed. -/
theorem reads_value {v : Expr} (hv : IsValue v) : ∀ i, v.reads i = false := by
  intro i
  induction hv with
  | construct hfs ih =>
    simp only [Expr.reads]
    exact readsList_values (fun e he _ => ih e he) hfs
  | closure => simp [Expr.reads]
  | result _ _ ih => simpa [Expr.reads] using ih
  | _ => rfl

/-- What an expression with a value put for slot 0 reads, it read one slot
up. -/
theorem reads_subst {v : Expr} (hv : IsValue v) :
    ∀ e i, (Expr.subst v e).reads i = true → e.reads (i + 1) = true := by
  intro e
  induction e using Expr.subst.induct
    (motive_2 := fun arms => ∀ i, Expr.readsArms i (Expr.substArms v arms) = true →
      Expr.readsArms (i + 1) arms = true)
    (motive_3 := fun e => ∀ i, Expr.readsOpt i (Expr.substOpt v e) = true →
      Expr.readsOpt (i + 1) e = true)
    (motive_4 := fun ss => ∀ i, Expr.readsStmts i (Expr.substStmts v ss) = true →
      Expr.readsStmts (i + 1) ss = true)
    (motive_5 := fun es => ∀ i, Expr.readsList i (Expr.substList v es) = true →
      Expr.readsList (i + 1) es = true)
  all_goals
    intros
    simp_all [Expr.subst, Expr.substList, Expr.substOpt, Expr.substStmts, Expr.substArms,
      Expr.reads, Expr.readsList, Expr.readsOpt, Expr.readsStmts, Expr.readsArms, reads_value hv]
  all_goals first
    | omega
    | (rcases ‹_ ∨ _› with h | h <;> (try rcases h with h | h) <;> simp_all)
    | skip

/-- Putting a value for slot 0 keeps an expression within `A` when the
value is within `A`, or the expression does not read slot 0. -/
theorem capsWithin_subst_reads {A : List CapKind} {v : Expr} :
    ∀ e, e.capsWithin A = true → (e.reads 0 = true → v.capsWithin A = true) →
      (Expr.subst v e).capsWithin A = true := by
  intro e
  induction e using Expr.subst.induct
    (motive_2 := fun arms => Expr.capsWithinArms A arms = true →
      (Expr.readsArms 0 arms = true → v.capsWithin A = true) →
      Expr.capsWithinArms A (Expr.substArms v arms) = true)
    (motive_3 := fun e => Expr.capsWithinOpt A e = true →
      (Expr.readsOpt 0 e = true → v.capsWithin A = true) →
      Expr.capsWithinOpt A (Expr.substOpt v e) = true)
    (motive_4 := fun ss => Expr.capsWithinStmts A ss = true →
      (Expr.readsStmts 0 ss = true → v.capsWithin A = true) →
      Expr.capsWithinStmts A (Expr.substStmts v ss) = true)
    (motive_5 := fun es => Expr.capsWithinList A es = true →
      (Expr.readsList 0 es = true → v.capsWithin A = true) →
      Expr.capsWithinList A (Expr.substList v es) = true)
  all_goals
    intros
    simp_all [Expr.subst, Expr.substList, Expr.substOpt, Expr.substStmts, Expr.substArms,
      Expr.capsWithin, Expr.capsWithinList, Expr.capsWithinOpt, Expr.capsWithinStmts,
      Expr.capsWithinArms, Expr.reads, Expr.readsList, Expr.readsOpt, Expr.readsStmts,
      Expr.readsArms]

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
  | result b _ ih => show Expr.result b (Expr.subst w _) = _; rw [ih]
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
theorem typed_subst {R₀ : Ty} {M₀ : List Ty} {v : Expr} {u : Ty} (hv : IsValue v)
    (hvt : Typed P R₀ [] M₀ (.expr v u)) :
    ∀ {R : Ty} {Γ M : List Ty} {j : Goal}, Typed P R Γ M j → ∀ Δ, Γ = u :: Δ →
      Typed P R Δ M (j.subst v) := by
  intro R Γ M j h
  induction h with
  | unit => intro _ _; exact Typed.unit
  | int hf => intro _ _; exact Typed.int hf
  | bool => intro _ _; exact Typed.bool
  | str => intro _ _; exact Typed.str
  | path => intro _ _; exact Typed.path
  | cap => intro _ _; exact Typed.cap
  | ioError => intro _ _; exact Typed.ioError
  | ok hf _ ih => intro Δ hΓ; exact Typed.ok hf (ih Δ hΓ)
  | err hf _ ih => intro Δ hΓ; exact Typed.err hf (ih Δ hΓ)
  | try_ hf _ hc ih => intro Δ hΓ; exact Typed.try_ hf (ih Δ hΓ) hc
  | frame _ ih => intro Δ hΓ; exact Typed.frame (ih Δ hΓ)
  | builtin hb _ ih => intro Δ hΓ; exact Typed.builtin hb (ih Δ hΓ)
  | @local_ _ Γ _ i t hl =>
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
  | @match_ _ Γ _ sc arms u' t _ _ hc ihs iharms =>
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
  | @closure _ Γ M ps h cs cf ret body hle hpure hcaps hb ih =>
    intro Δ hΓ
    subst hΓ
    cases h with
    | zero =>
      show Typed P _ Δ M (.expr (.closure ps 0 cs cf ret body) _)
      exact Typed.closure (Nat.zero_le _) (fun _ i _ hi => absurd hi (Nat.not_lt_zero i)) hcaps
        (by simpa using hb)
    | succ h =>
      show Typed P _ Δ M (.expr (.closure ps h (shiftCaptures cs) cf ret (Expr.subst v body)) _)
      refine Typed.closure (by simpa using hle) (fun hc i t hi hr hl => ?_) (fun hc => ?_)
        (ih (Δ.take h ++ bindingTypes ps) (by simp))
      · exact hpure hc (i + 1) t (by omega) (reads_subst hv body i hr) (by simpa [lookup] using hl)
      · refine capsWithin_subst_reads body (hcaps hc) fun hr => ?_
        exact valueTyped_pure (value_valueTyped hv hvt) (hpure hc 0 u (Nat.succ_pos _) hr rfl)
  | callValue _ _ ihc iha => intro Δ hΓ; exact Typed.callValue (ihc Δ hΓ) (iha Δ hΓ)
  | var_ hl => intro _ _; exact Typed.var_ hl
  | blockVar _ _ ihe ihs => intro Δ hΓ; exact Typed.blockVar (ihe Δ hΓ) (ihs Δ hΓ)
  | blockAssign hpl _ _ ihe ihs => intro Δ hΓ; exact Typed.blockAssign hpl (ihe Δ hΓ) (ihs Δ hΓ)
  | mutCall hf hr hpl _ ih => intro Δ hΓ; exact Typed.mutCall hf hr hpl (ih Δ hΓ)
  | @cell _ _ _ w e _ _ hw hwt _ _ ihe =>
    intro Δ hΓ
    show Typed P _ Δ _ (.expr (.cell (Expr.subst v w) (Expr.subst v e)) _)
    rw [subst_value hw]
    exact Typed.cell hw (value_typed hw hwt) (ihe Δ hΓ)
  | @method _ _ _ pl w e _ _ hpl hw hwt _ _ ihe =>
    intro Δ hΓ
    show Typed P _ Δ _ (.expr (.method pl (Expr.subst v w) (Expr.subst v e)) _)
    rw [subst_value hw]
    exact Typed.method hpl hw (value_typed hw hwt) (ihe Δ hΓ)

/-- A function's body, with closed values of its parameters' types put for
the slots they bind, is closed and has the body's type. -/
theorem typed_instantiate {R' : Ty} {M' : List Ty} :
    ∀ (ps : List Param) {vs : List Expr} {body e : Expr} {t : Ty},
      Typed P R (bindingTypes ps) M (.expr body t) →
      Typed P R' [] M' (.args vs (ps.map Param.type)) → AllValues vs →
      instantiate ps vs body = some e → Typed P R [] M (.expr e t) := by
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
      Typed P R Γ M (.args vs (ps.map Param.type)) → ∃ e, instantiate ps vs body = some e := by
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
theorem typed_substAll {R' : Ty} {M' : List Ty} :
    ∀ (bs : List Expr) {bts : List Ty} {e : Expr} {t : Ty},
      Typed P R bts M (.expr e t) → Typed P R' [] M' (.args bs bts) → AllValues bs →
      Typed P R [] M (.expr (Expr.substAll bs e) t) := by
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
def MatchFor (P : Program) (R : Ty) (M : List Ty) : PGoal → Prop
  | .one p u bts => ∀ {v bs}, IsValue v → Typed P R [] M (.expr v u) → matchPat p v = some bs →
      Typed P R [] M (.args bs bts) ∧ AllValues bs
  | .many ps ts bts => ∀ {vs bs}, AllValues vs → Typed P R [] M (.args vs ts) →
      matchPats ps vs = some bs → Typed P R [] M (.args bs bts) ∧ AllValues bs

theorem matchPat_typed : ∀ {g : PGoal}, PatTyped P g → MatchFor P R M g := by
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
  | resultOk _ ih =>
    intro v bs hv ht hm
    rcases canonical_result hv ht with ⟨w, rfl, hw, htw⟩ | ⟨w, rfl, _, _⟩
    · simp only [matchPat, ite_true] at hm
      exact ih hw htw hm
    · simp [matchPat] at hm
  | resultErr _ ih =>
    intro v bs hv ht hm
    rcases canonical_result hv ht with ⟨w, rfl, _, _⟩ | ⟨w, rfl, hw, htw⟩
    · simp [matchPat] at hm
    · simp only [matchPat, ite_true] at hm
      exact ih hw htw hm
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
  · split at hp
    · cases hp; rfl
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
        | result ok err =>
          simp only [Bool.and_eq_true] at hc
          have key : ∀ (b : Bool) {w : Expr} {u : Ty}, ValueTyped P (.one w u) →
              covers P fuel (specialize (.result b) 1 rows) (u :: ts) = true →
              ∃ row ∈ rows, (matchPats row (.result b w :: vs)).isSome = true := by
            intro b w u hw hcov
            obtain ⟨row, hrow, hm⟩ := ih hcov (ValueTyped.cons hw hvs)
            refine specialize_sound (xs := [w]) hrow rfl hm fun p ps hps hps' => ?_
            cases p with
            | result b' _ _ q =>
              simp only [Head.args] at hps
              split at hps
              · next hb =>
                cases hps
                subst hb
                simp only [matchPats_cons, Bool.and_eq_true] at hps'
                simpa [matchPat] using hps'.1
              · cases hps
            | _ => simp [Head.args] at hps
          cases hv with
          | ok _ hw => exact key true hw hc.1
          | err _ hw => exact key false hw hc.2
        | int | string | path | ioError | cap _ | fn _ _ => simp at hc
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
    Typed P R Γ M (.arms arms u t) → p ∈ unguarded arms → ∃ bs, PatTyped P (.one p u bs) := by
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
    (he : exhaustiveArms P arms = true) (ht : Typed P R Γ M (.arms arms u t))
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
    (hst : Typed P R [] M (.expr s u)) (harms : Typed P R [] M (.arms arms u t)) : Covered arms s :=
  hc.elim (fun he => exhaustiveArms_sound he harms (value_valueTyped hs hst)) (·.2)

/-! ## Operators -/

theorem intResult_typed {n : Int} : OutcomeTyped P R M (intResult n) .int := by
  unfold intResult
  cases hf : fits n with
  | true => exact Typed.int hf
  | false => trivial

/-- An operator that evaluates both operands, applied to values of its
operand type, gives a result of its result type, or panics. -/
theorem evalBinary_typed {op : BinOp} {a t : Ty} {l r : Expr} {o : Outcome}
    (hop : BinOpTyped op a t) (hl : IsValue l) (hr : IsValue r)
    (htl : Typed P R [] M (.expr l a)) (htr : Typed P R [] M (.expr r a))
    (h : evalBinary op l r = some o) : OutcomeTyped P R M o t := by
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
    (htl : Typed P R [] M (.expr l a)) (htr : Typed P R [] M (.expr r a)) :
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
    (h : Typed P R Γ M (.expr v (.cap k))) : ∃ r, v = .cap k r := by
  cases hv <;> cases h; exact ⟨_, rfl⟩

theorem canonical_str {Γ : List Ty} {v : Expr} (hv : IsValue v)
    (h : Typed P R Γ M (.expr v .string)) : ∃ s, v = .str s := by
  cases hv <;> cases h; exact ⟨_, rfl⟩

/-- A built-in call whose arguments are values of its types is an effect
on a capability of its receiver's kind, or a pure call with a value. -/
theorem builtin_progress {op : Builtin} {ts : List Ty} {t : Ty} {vs : List Expr}
    (hb : BuiltinTyped op ts t) (hargs : Typed P R [] M (.args vs ts)) (hvs : AllValues vs) :
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
        rcases canonical_result hv₁ h₁ with ⟨_, rfl, _⟩ | ⟨_, rfl, _⟩
        all_goals exact ⟨_, Step.builtinPure (AllValues.cons hv₁ hvs) rfl⟩
  | unwrapOr =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr with
      | argsCons _ hr =>
        cases hr
        cases hvs with
        | cons hv₁ hvs =>
          rcases canonical_result hv₁ h₁ with ⟨_, rfl, _⟩ | ⟨_, rfl, _⟩
          all_goals exact ⟨_, Step.builtinPure (AllValues.cons hv₁ hvs) rfl⟩

/-- A pure built-in call on values of its types gives a value of its
result type. -/
theorem evalPure_typed {op : Builtin} {ts : List Ty} {t : Ty} {vs : List Expr} {v : Expr}
    (hb : BuiltinTyped op ts t) (hargs : Typed P R [] M (.args vs ts)) (hvs : AllValues vs)
    (h : evalPure op vs = some v) : Typed P R [] M (.expr v t) := by
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
        rcases canonical_result hv₁ h₁ with ⟨_, rfl, _⟩ | ⟨_, rfl, _⟩ <;>
          (simp [evalPure] at h; subst h; exact Typed.bool)
  | unwrapOr =>
    cases hargs with
    | argsCons h₁ hr =>
      cases hr with
      | argsCons h₂ hr =>
        cases hr
        cases hvs with
        | cons hv₁ _ =>
          rcases canonical_result hv₁ h₁ with ⟨_, rfl, _, hw⟩ | ⟨_, rfl, _, _⟩ <;>
            simp [evalPure] at h <;> subst h
          · exact hw
          · exact h₂

/-- Every answer the semantics allows an effect is a value of the call's
result type. -/
theorem allows_typed {op : Builtin} {k : CapKind} {root : CapRoot} {ts : List Ty} {t : Ty}
    {vs : List Expr} {r : Expr} (hb : BuiltinTyped op (.cap k :: ts) t)
    (hr : Request.Allows ⟨op, k, root, vs⟩ r) : Typed P R [] M (.expr r t) := by
  cases hb with
  | println | write | createDir =>
    rcases hr with rfl | rfl
    · exact Typed.ok rfl Typed.unit
    · exact Typed.err rfl Typed.ioError
  | readToString =>
    rcases hr with rfl | ⟨s, rfl⟩
    · exact Typed.err rfl Typed.ioError
    · exact Typed.ok rfl Typed.str
  | restrictTo hk =>
    obtain ⟨a, k', root', hk', rfl, _⟩ := hr
    rw [hk] at hk'
    cases hk'
    exact Typed.cap
  | readOnly _ | writeOnly _ => exact absurd hr id

/-! ## Places and calls -/

/-- An argument list keeps its typing when one argument is replaced by an
expression of the same type. -/
theorem args_set {Γ : List Ty} :
    ∀ {es : List Expr} {ts : List Ty} {i : Nat} {t : Ty} {e : Expr},
      Typed P R Γ M (.args es ts) → ts[i]? = some t → Typed P R Γ M (.expr e t) →
      Typed P R Γ M (.args (es.set i e) ts) := by
  intro es
  induction es with
  | nil => intro ts i t e h _ _; cases h; exact Typed.argsNil
  | cons x xs ih =>
    intro ts i t e h hi he
    cases h with
    | argsCons hx hr =>
      cases i with
      | zero =>
        simp only [List.getElem?_cons_zero, Option.some.injEq] at hi
        subst hi
        exact Typed.argsCons he hr
      | succ i => exact Typed.argsCons hx (ih hr (by simpa using hi) he)

/-- The field at a path of a value, which the value's type has, is a value
of the path's type. -/
theorem getAt_typed {Γ : List Ty} :
    ∀ (is : List Nat) {v : Expr} {u t : Ty}, IsValue v → Typed P R Γ M (.expr v u) →
      P.pathTy u is = some t → IsValue (v.getAt is) ∧ Typed P R Γ M (.expr (v.getAt is) t) := by
  intro is
  induction is with
  | nil =>
    intro v u t hv h hp
    simp only [Program.pathTy, Option.some.injEq] at hp
    subst hp
    exact ⟨hv, h⟩
  | cons i is ih =>
    intro v u t hv h hp
    cases u with
    | named n =>
      simp only [Program.pathTy] at hp
      split at hp
      · next fts hf =>
        split at hp
        · next t' hi =>
          obtain ⟨variant, fs, fts', rfl, hf', hargs, hvals⟩ := canonical_named hv h
          obtain ⟨rfl, rfl⟩ := fieldsOf_struct hf hf'
          obtain ⟨e, he, hte⟩ := args_get hargs hi
          have hev : IsValue e := hvals.forall e (List.mem_of_getElem? he)
          simp only [Expr.getAt, he, Option.getD_some]
          exact ih hev hte hp
        · cases hp
      · cases hp
    | _ => simp [Program.pathTy] at hp

/-- A value with a value of a path's type put at the path is a value of
its type. -/
theorem setAt_typed {Γ : List Ty} :
    ∀ (is : List Nat) {v w : Expr} {u t : Ty}, IsValue v → Typed P R Γ M (.expr v u) →
      P.pathTy u is = some t → IsValue w → Typed P R Γ M (.expr w t) →
      IsValue (v.setAt is w) ∧ Typed P R Γ M (.expr (v.setAt is w) u) := by
  intro is
  induction is with
  | nil =>
    intro v w u t _ _ hp hw hwt
    simp only [Program.pathTy, Option.some.injEq] at hp
    subst hp
    exact ⟨hw, hwt⟩
  | cons i is ih =>
    intro v w u t hv h hp hw hwt
    cases u with
    | named n =>
      simp only [Program.pathTy] at hp
      split at hp
      · next fts hf =>
        split at hp
        · next t' hi =>
          obtain ⟨variant, fs, fts', rfl, hf', hargs, hvals⟩ := canonical_named hv h
          obtain ⟨rfl, rfl⟩ := fieldsOf_struct hf hf'
          obtain ⟨e, he, hte⟩ := args_get hargs hi
          have hev : IsValue e := hvals.forall e (List.mem_of_getElem? he)
          obtain ⟨hv', ht'⟩ := ih hev hte hp hw hwt
          simp only [Expr.setAt, he, Option.getD_some]
          refine ⟨IsValue.construct fun x hx => ?_, Typed.construct hf (args_set hargs hi ht')⟩
          rcases List.mem_or_eq_of_mem_set hx with hx | rfl
          · exact hvals.forall x hx
          · exact hv'
        · cases hp
      · cases hp
    | _ => simp [Program.pathTy] at hp

/-- The type of a place of var `i + 1` under one more var is that of the
place of var `i`. -/
theorem placeTy_succ {u : Ty} {i : Nat} {is : List Nat} :
    P.placeTy (u :: M) ⟨i + 1, is⟩ = P.placeTy M ⟨i, is⟩ := by
  simp [Program.placeTy, lookup]

/-- The type of a place of var 0 is that of its path in the var's type. -/
theorem placeTy_zero {u : Ty} {is : List Nat} :
    P.placeTy (u :: M) ⟨0, is⟩ = P.pathTy u is := by
  simp [Program.placeTy, lookup]

/-- No place is typed with no var in scope. -/
theorem placeTy_nil {pl : Place} : P.placeTy [] pl = none := by
  simp [Program.placeTy, lookup]

/-- The body a call runs, with closed values of the argument types put
in, is closed, sees no var, and has the body's type. -/
theorem typed_enter {R' : Ty} {M' : List Ty} {f : Function} {vs : List Expr} {e : Expr} {t : Ty}
    (hb : Typed P R (bindingTypes f.params) f.receiver.toList (.expr f.body t))
    (hargs : Typed P R' [] M' (.args vs f.argTypes)) (hvs : AllValues vs)
    (he : f.enter vs = some e) : Typed P R [] [] (.expr e t) := by
  unfold Function.enter at he
  rw [Function.argTypes_eq] at hargs
  cases hr : f.receiver with
  | none =>
    rw [hr] at he hb hargs
    exact typed_instantiate _ hb hargs hvs he
  | some u =>
    rw [hr] at he hb hargs
    cases hargs with
    | argsCons hrt hrest =>
      cases hvs with
      | cons hrv hvs =>
        obtain ⟨b, hi, rfl⟩ := Option.map_eq_some_iff.mp he
        exact Typed.cell hrv (value_typed hrv hrt) (typed_instantiate _ hb hrest hvs hi)

/-- Arguments typed against a function's argument types are as many as
it takes, so a call can enter it. -/
theorem enter_some {Γ : List Ty} {f : Function} {vs : List Expr}
    (hargs : Typed P R Γ M (.args vs f.argTypes)) : ∃ e, f.enter vs = some e := by
  unfold Function.enter
  rw [Function.argTypes_eq] at hargs
  cases hr : f.receiver with
  | none =>
    rw [hr] at hargs
    exact instantiate_some _ hargs
  | some u =>
    rw [hr] at hargs
    cases hargs with
    | argsCons _ hrest =>
      obtain ⟨b, hb⟩ := instantiate_some (body := f.body) _ hrest
      simp [hb]

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

theorem progress_goal : ∀ {R : Ty} {Γ M : List Ty} {j : Goal}, Typed P R Γ M j → Γ = [] →
    ProgressFor P j := by
  intro R Γ M j h
  induction h with
  | unit => intro _; exact Or.inl IsValue.unit
  | int _ => intro _; exact Or.inl (IsValue.int _)
  | bool => intro _; exact Or.inl (IsValue.bool _)
  | str => intro _; exact Or.inl (IsValue.str _)
  | path => intro _; exact Or.inl (IsValue.path _)
  | cap => intro _; exact Or.inl (IsValue.cap _ _)
  | ioError => intro _; exact Or.inl IsValue.ioError
  | ok _ _ ih | err _ _ ih =>
    intro hΓ
    rcases ih hΓ with hv | ⟨o, hs⟩
    · exact Or.inl (IsValue.result _ hv)
    · exact Or.inr ⟨_, Step.resultArg hs⟩
  | try_ _ he hc ih =>
    intro hΓ
    subst hΓ
    rcases ih rfl with hv | ⟨o, hs⟩
    · rcases canonical_result hv he with ⟨w, rfl, hw, _⟩ | ⟨w, rfl, hw, _⟩
      · exact Or.inr ⟨_, Step.tryOk hw⟩
      · cases hc with
        | none => exact Or.inr ⟨_, Step.tryErr hw⟩
        | some _ _ _ => exact Or.inr ⟨_, Step.tryConvert hw⟩
    · exact Or.inr ⟨_, Step.tryArg hs⟩
  | frame _ ih =>
    intro hΓ
    rcases ih hΓ with hv | ⟨o, hs⟩
    · exact Or.inr ⟨_, Step.frameValue hv⟩
    · exact Or.inr ⟨_, Step.frame hs⟩
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
  | @binary _ Γ _ op l r a t hop hl hr ihl ihr =>
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
    · obtain ⟨_, hi⟩ := enter_some hargs
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
  | @match_ _ _ _ _ arms _ _ hs harms hc ih _ =>
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
  | @closure _ _ _ ps h cs cf ret body hle _ _ _ _ =>
    intro hΓ
    subst hΓ
    have : h = 0 := by simpa using hle
    subst this
    exact Or.inl (IsValue.closure _ _ _ _ _)
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
  | var_ _ => intro _; exact Or.inr ⟨_, Step.var⟩
  | blockVar _ _ ihe _ =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact ⟨_, Step.blockVar hv⟩
    · exact ⟨_, Step.blockVarArg hs⟩
  | blockAssign _ _ _ ihe _ =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact ⟨_, Step.blockAssign hv⟩
    · exact ⟨_, Step.blockAssignArg hs⟩
  | mutCall hf hr _ hargs ih =>
    intro hΓ
    rcases ih hΓ with hall | ⟨vs, e, rest, o, rfl, hvs, hs⟩
    · obtain ⟨_, hi⟩ := instantiate_some _ hargs
      exact Or.inr ⟨_, Step.mutCall hf hr hall hi⟩
    · exact Or.inr ⟨_, Step.mutCallArg hvs hs⟩
  | cell _ _ _ _ ihe =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact Or.inr ⟨_, Step.cellValue hv⟩
    · exact Or.inr ⟨_, Step.cellBody hs⟩
  | method _ _ _ _ _ ihe =>
    intro hΓ
    rcases ihe hΓ with hv | ⟨o, hs⟩
    · exact Or.inr ⟨_, Step.methodValue hv⟩
    · exact Or.inr ⟨_, Step.methodBody hs⟩

/-- **Progress.** A closed, well-typed expression of Oska Core is a value
or takes a step: to an expression, a panic, an effect or a return. -/
theorem progress {e : Expr} {t : Ty} (h : Typed P R [] M (.expr e t)) :
    IsValue e ∨ ∃ o, Step P e o :=
  progress_goal h rfl

/-! ## Preservation -/

/-- The outcome of a body's step, typed in its own function or closure, is
typed in its frame: a return gives the value it returns. -/
theorem OutcomeTyped.inFrame {o : Outcome} {t : Ty} (h : OutcomeTyped P t [] o t) :
    OutcomeTyped P R M o.inFrame t := by
  cases o with
  | next e => exact Typed.frame h
  | panic _ => trivial
  | perform req k => exact fun r hr => Typed.frame (h r hr)
  | ret v => exact value_typed h.1 h.2
  | read i is k => obtain ⟨_, hu, _⟩ := h; simp [placeTy_nil] at hu
  | write i is w e => obtain ⟨_, hu, _⟩ := h; simp [placeTy_nil] at hu

/-- The outcome of a step of a body that holds var 0, typed with that var
of type `u` and the vars `Mi` beyond it, in a function or closure whose
result type is `Ri`, is typed once its binder (`wrap`, holding `v`)
answers it: the binder answers a read or a write of var 0 with the value
it holds and keeps its type, and passes one of var `i + 1` on as one of
var `i`, which `hpass` places in `M`. -/
theorem OutcomeTyped.under {o : Outcome} {v : Expr} {u t Ri : Ty} {Mi : List Ty}
    {wrap : Expr → Expr → Expr} {onRet : Expr → Outcome}
    (h : OutcomeTyped P Ri (u :: Mi) o t) (hv : IsValue v) (hvt : Typed P R [] M (.expr v u))
    (hwrap : ∀ {v e}, IsValue v → Typed P R [] M (.expr v u) →
      Typed P Ri [] (u :: Mi) (.expr e t) → Typed P R [] M (.expr (wrap v e) t))
    (hret : ∀ {r}, IsValue r → Typed P Ri [] (u :: Mi) (.expr r Ri) →
      OutcomeTyped P R M (onRet r) t)
    (hpass : ∀ {i is u'}, P.placeTy Mi ⟨i, is⟩ = some u' → P.placeTy M ⟨i, is⟩ = some u') :
    OutcomeTyped P R M (o.under v wrap onRet) t := by
  cases o with
  | next e => exact hwrap hv hvt h
  | panic _ => trivial
  | perform req k => exact fun r hr => hwrap hv hvt (h r hr)
  | ret r => exact hret h.1 h.2
  | read i is k =>
    obtain ⟨w, hw, hk⟩ := h
    cases i with
    | zero =>
      rw [placeTy_zero] at hw
      obtain ⟨hgv, hgt⟩ := getAt_typed is hv hvt hw
      exact hwrap hv hvt (hk _ hgv (value_typed hgv hgt))
    | succ i =>
      rw [placeTy_succ] at hw
      exact ⟨w, hpass hw, fun r hr hrt => hwrap hv hvt (hk r hr (value_typed hr hrt))⟩
  | write i is w e =>
    obtain ⟨u', hu', hw, hwt, he⟩ := h
    cases i with
    | zero =>
      rw [placeTy_zero] at hu'
      obtain ⟨hsv, hst⟩ := setAt_typed is hv hvt hu' hw (value_typed hw hwt)
      exact hwrap hsv hst he
    | succ i =>
      rw [placeTy_succ] at hu'
      exact ⟨u', hpass hu', hw, value_typed hw hwt, hwrap hv hvt he⟩

/-- The outcome of a step of the rest of a block, typed with its var
holding a value of the var's type, is typed in the block's cell: the cell
answers a read or a write of its var with the value it holds, and keeps
the type of what it holds. -/
theorem OutcomeTyped.inCell {o : Outcome} {v : Expr} {u t : Ty}
    (h : OutcomeTyped P R (u :: M) o t) (hv : IsValue v) (hvt : Typed P R [] M (.expr v u)) :
    OutcomeTyped P R M (o.inCell v) t :=
  h.under hv hvt (fun hv hvt he => Typed.cell hv hvt he)
    (fun hr hrt => ⟨hr, value_typed hr hrt⟩) id

/-- The outcome of a step of a mutating method's body, typed with its
receiver as its one var, is typed in the running call: the call answers a
read or a write of its receiver with the value it holds, and a return
ends the body. -/
theorem OutcomeTyped.inMethod {o : Outcome} {pl : Place} {v : Expr} {u t : Ty}
    (h : OutcomeTyped P t [u] o t) (hpl : P.placeTy M pl = some u) (hv : IsValue v)
    (hvt : Typed P R [] M (.expr v u)) : OutcomeTyped P R M (o.inMethod pl v) t :=
  h.under hv hvt (fun hv hvt he => Typed.method hpl hv hvt he)
    (fun hr hrt => Typed.method hpl hv hvt (value_typed hr hrt))
    (fun hu => by simp [placeTy_nil] at hu)

/-- **Preservation.** In a well-typed program, a step of a closed
expression of type `t`, in a function or closure whose result type is
`R`, leads to a closed expression of type `t`, a panic, an effect after
which the run goes on at type `t`, or a return of a value of type `R`. -/
theorem preservation (hP : ProgramTyped P) {e : Expr} {o : Outcome} (hs : Step P e o) :
    ∀ {R t : Ty} {M : List Ty}, Typed P R [] M (.expr e t) → OutcomeTyped P R M o t := by
  induction hs with
  | negArg _ ih =>
    intro R t M ht
    cases ht with
    | neg he => exact (ih he).map fun _ h => Typed.neg h
  | neg =>
    intro R t M ht
    cases ht with
    | neg _ => exact intResult_typed
  | notArg _ ih =>
    intro R t M ht
    cases ht with
    | not he => exact (ih he).map fun _ h => Typed.not h
  | not =>
    intro R t M ht
    cases ht with
    | not _ => exact Typed.bool
  | binaryLhs _ ih =>
    intro R t M ht
    cases ht with
    | binary hop hl hr => exact (ih hl).map fun _ h => Typed.binary hop h hr
  | binaryRhs _ _ _ ih =>
    intro R t M ht
    cases ht with
    | binary hop hl hr => exact (ih hr).map fun _ h => Typed.binary hop hl h
  | binary hl hr he =>
    intro R t M ht
    cases ht with
    | binary hop htl htr => exact evalBinary_typed hop hl hr htl htr he
  | andFalse | orTrue | impliesFalse =>
    intro R t M ht
    cases ht with
    | binary hop _ _ => cases hop; exact Typed.bool
  | andTrue | orFalse | impliesTrue =>
    intro R t M ht
    cases ht with
    | binary hop _ hr => cases hop; exact hr
  | callArg _ _ ih =>
    intro R t M ht
    cases ht with
    | call hf hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.call hf ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | call hf hvs hi =>
    intro R t M ht
    cases ht with
    | call hf' hargs =>
      rw [hf] at hf'
      cases hf'
      exact Typed.frame (typed_enter (hP _ _ hf) hargs hvs hi)
  | ifCond _ ih =>
    intro R t M ht
    cases ht with
    | ifElse hc hth hel => exact (ih hc).map fun _ h => Typed.ifElse h hth hel
    | ifNoElse hc hth => exact (ih hc).map fun _ h => Typed.ifNoElse h hth
  | ifTrue =>
    intro R t M ht
    cases ht with
    | ifElse _ hth _ => exact hth
  | ifFalse =>
    intro R t M ht
    cases ht with
    | ifElse _ _ hel => exact hel
  | ifTrueNoElse =>
    intro R t M ht
    cases ht with
    | ifNoElse _ hth => exact Typed.block (Typed.blockExpr hth Typed.blockNone)
  | ifFalseNoElse =>
    intro R t M ht
    cases ht with
    | ifNoElse _ _ => exact Typed.unit
  | blockNone =>
    intro R t M ht
    cases ht with
    | block hb => cases hb; exact Typed.unit
  | blockSome =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockSome he => exact he
  | blockExprArg _ ih =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockExpr he hrest => exact (ih he).map fun _ h => Typed.block (Typed.blockExpr h hrest)
  | blockExpr _ =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockExpr _ hrest => exact Typed.block hrest
  | blockLetArg _ ih =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetBind he hrest =>
        exact (ih he).map fun _ h => Typed.block (Typed.blockLetBind h hrest)
      | blockLetDrop he hrest =>
        exact (ih he).map fun _ h => Typed.block (Typed.blockLetDrop h hrest)
  | blockLetBind hv =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetBind he hrest => exact Typed.block (typed_subst hv he hrest [] rfl)
  | blockLetDrop _ =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockLetDrop _ hrest => exact Typed.block hrest
  | constructArg _ _ ih =>
    intro R t M ht
    cases ht with
    | construct hf hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.construct hf ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | fieldBase _ ih =>
    intro R t M ht
    cases ht with
    | field hf hi hb => exact (ih hb).map fun _ h => Typed.field hf hi h
  | field _ he =>
    intro R t M ht
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
    intro R t M ht
    cases ht with
    | builtin hb hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.builtin hb ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | builtinPure hvs he =>
    intro R t M ht
    cases ht with
    | builtin hb hargs => exact evalPure_typed hb hargs hvs he
  | builtinEffect _ _ =>
    intro R t M ht
    cases ht with
    | builtin hb hargs =>
      cases hargs with
      | argsCons h₁ _ =>
        cases h₁
        exact fun r hr => allows_typed hb hr
  | matchScrutinee hstep ih =>
    intro R t M ht
    cases ht with
    | match_ hs harms hc =>
      exact (ih hs).map fun _ h =>
        Typed.match_ h harms (hc.elim Or.inl fun ⟨hv, _⟩ => absurd hstep (value_no_step hv))
  | @matchSkip _ _ _ _ rest hv hm =>
    intro R t M ht
    cases ht with
    | @match_ _ _ _ _ _ u _ hs harms hc =>
      have hcov := match_covered hc hv hs harms
      have hrest : Typed P R [] M (.arms rest u t) := by
        cases harms with
        | armsCons _ _ hr => exact hr
        | armsConsGuard _ _ _ hr => exact hr
      exact Typed.match_ hs hrest (Or.inr ⟨hv, covered_skip hcov hm⟩)
  | matchArm hv hm =>
    intro R t M ht
    cases ht with
    | match_ hs harms hc =>
      cases harms with
      | armsCons hp hbody _ =>
        obtain ⟨hargs, hvals⟩ := matchPat_typed hp hv hs hm
        exact typed_substAll _ (by simpa using hbody) hargs hvals
  | matchGuard hv hm =>
    intro R t M ht
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
    intro R t M ht
    cases ht with
    | callValue hc hargs => exact (ih hc).map fun _ h => Typed.callValue h hargs
  | callValueArg _ _ _ ih =>
    intro R t M ht
    cases ht with
    | callValue hc hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.callValue hc ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | callValue hvs hi =>
    intro R t M ht
    cases ht with
    | callValue hc hargs =>
      obtain ⟨ps, cs, body, heq, rfl, hb⟩ := canonical_fn (IsValue.closure _ _ _ _ _) hc
      cases heq
      exact Typed.frame (typed_instantiate _ hb hargs hvs hi)
  | resultArg _ ih =>
    intro R t M ht
    cases ht with
    | ok hf he => exact (ih he).map fun _ h => Typed.ok hf h
    | err hf he => exact (ih he).map fun _ h => Typed.err hf h
  | tryArg _ ih =>
    intro R t M ht
    cases ht with
    | try_ hf he hc => exact (ih he).map fun _ h => Typed.try_ hf h hc
  | tryOk _ =>
    intro R t M ht
    cases ht with
    | try_ _ he _ =>
      cases he with
      | ok _ hw => exact hw
  | tryErr hv =>
    intro R t M ht
    cases ht with
    | try_ hf he hc =>
      cases hc
      cases he with
      | err _ hw => exact ⟨IsValue.result _ hv, Typed.err hf hw⟩
  | tryConvert hv =>
    intro R t M ht
    cases ht with
    | try_ hf he hc =>
      cases hc with
      | some hfind hps hret =>
        subst hret
        cases he with
        | err hf' hw =>
          refine Typed.try_ hf (Typed.err ?_ (Typed.call hfind ?_)) ConvTyped.none
          · simp only [Ty.fieldCapFree, Bool.and_eq_true] at hf hf' ⊢
            exact ⟨hf'.1, hf.2⟩
          · rw [hps]
            exact Typed.argsCons hw Typed.argsNil
  | frame _ ih =>
    intro R t M ht
    cases ht with
    | frame he => exact (ih he).inFrame
  | frameValue hv =>
    intro R t M ht
    cases ht with
    | frame he => exact value_typed hv he
  | var =>
    intro R t M ht
    cases ht with
    | var_ hl => exact ⟨t, by simp [Program.placeTy, hl, Program.pathTy], fun w _ hw => hw⟩
  | blockVarArg _ ih =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockVar he hrest => exact (ih he).map fun _ h => Typed.block (Typed.blockVar h hrest)
  | blockVar hv =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockVar he hrest => exact Typed.cell hv (value_typed hv he) (Typed.block hrest)
  | blockAssignArg _ ih =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockAssign hpl he hrest =>
        exact (ih he).map fun _ h => Typed.block (Typed.blockAssign hpl h hrest)
  | blockAssign hv =>
    intro R t M ht
    cases ht with
    | block hb =>
      cases hb with
      | blockAssign hpl he hrest => exact ⟨_, hpl, hv, he, Typed.block hrest⟩
  | cellBody _ ih =>
    intro R t M ht
    cases ht with
    | cell hv hvt he => exact (ih he).inCell hv (value_typed hv hvt)
  | cellValue hw =>
    intro R t M ht
    cases ht with
    | cell _ _ he => exact value_typed hw he
  | mutCallArg _ _ ih =>
    intro R t M ht
    cases ht with
    | mutCall hf hr hpl hargs =>
      obtain ⟨ts₁, _, ts₂, hts, h₁, he, h₂⟩ := args_split _ hargs
      refine (ih he).map fun _ h => Typed.mutCall hf hr hpl ?_
      rw [hts]
      exact args_append _ h₁ (Typed.argsCons h h₂)
  | mutCall hf hr hvs hi =>
    intro R t M ht
    cases ht with
    | mutCall hf' hr' hpl hargs =>
      rw [hf] at hf'
      cases hf'
      rw [hr] at hr'
      cases hr'
      have hb := hP _ _ hf
      rw [hr] at hb
      exact ⟨_, hpl, fun w hw hwt => Typed.method hpl hw hwt (typed_instantiate _ hb hargs hvs hi)⟩
  | methodBody _ ih =>
    intro R t M ht
    cases ht with
    | method hpl hv hvt he => exact (ih he).inMethod hpl hv (value_typed hv hvt)
  | methodValue hw =>
    intro R t M ht
    cases ht with
    | method hpl hv hvt he => exact ⟨_, hpl, hv, value_typed hv hvt, value_typed hw he⟩

/-! ## Type safety -/

/-- **Type safety** of a program as the checker hands it over. In a
program whose functions are source typed, every expression a closed,
source-typed expression steps to is typed at its type, and is a value or
takes a step (to an expression, a panic or an effect): a run never gets
stuck, whatever answers the host gives that the semantics allows. -/
theorem type_safety (hP : ProgramSourceTyped P) {e e' : Expr} {t : Ty} {tr : List Request}
    (he : SourceTyped P R [] [] e t) (hs : Steps P e tr e') :
    Typed P R [] [] (.expr e' t) ∧ (IsValue e' ∨ ∃ o, Step P e' o) := by
  have typed : Typed P R [] [] (.expr e' t) := by
    have := he.1
    clear he
    induction hs with
    | refl => exact this
    | next h _ ih => exact ih (preservation hP.typed h this)
    | perform h hr _ ih => exact ih (preservation hP.typed h this _ hr)
  exact ⟨typed, progress typed⟩

end OskaFormal.Core
