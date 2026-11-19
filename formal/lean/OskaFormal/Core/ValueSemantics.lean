import OskaFormal.Core.Soundness

/-!
# Oska Core: value semantics of mutating methods

A `var` lives in the `cell` its block runs in, and nowhere else. A `let`
that copies a var took its value when it bound, so a later write cannot
reach the copy; a mutating method sees its receiver as its only var, and
its caller's vars only through the write-back of its receiver at its end.

`mutating_call_changes_only_its_receiver` states that for a whole run.
In a well-typed program, take the vars `s` (var 0 first, each in its
`cell`) around the block `{ pl.f(vs); c }`, a mutating call on the place
`pl` followed by a value `c`, such as a `let` copy of a var taken before
the call. Every run of it that ends in the same cells, around a value, has:

- the value `c`, unchanged: the copy is the value it was;
- every var other than `pl.var` as it was;
- `pl.var` changed only at `pl.fields`, to the receiver the method's body
  ended with, when run on its own (`ReceiverRun`) from the receiver read
  at `pl` before the call: a read of the receiver sees the last write to
  it, and the final write-back reaches the caller's cell.
-/

namespace OskaFormal.Core

variable {P : Program} {R : Ty}

/-! ## Cells -/

/-- `e` run in the cells of the vars `s`, var 0 (the innermost) first. -/
def Expr.cells : List Expr → Expr → Expr
  | [], e => e
  | v :: vs, e => Expr.cells vs (.cell v e)

/-- A step's outcome as the cells of the vars `s` answer it. -/
def Outcome.inCells : List Expr → Outcome → Outcome
  | [], o => o
  | v :: vs, o => Outcome.inCells vs (o.inCell v)

/-- How many cells `e` is in, outermost in. -/
def Expr.cellDepth : Expr → Nat
  | .cell _ e => e.cellDepth + 1
  | _ => 0

theorem step_cells {e : Expr} {o : Outcome} :
    ∀ (s : List Expr), Step P e o → Step P (Expr.cells s e) (o.inCells s) := by
  intro s
  induction s generalizing e o with
  | nil => exact id
  | cons v vs ih => exact fun h => ih (Step.cellBody h)

theorem cell_not_value {v e : Expr} : ¬ IsValue (.cell v e) := fun h => nomatch h

/-- A step of `e` in cells, with `e` not a value, is a step of `e` the
cells answer. -/
theorem step_cells_inv :
    ∀ (s : List Expr) {e : Expr} {o : Outcome}, Step P (Expr.cells s e) o → ¬ IsValue e →
      ∃ o', Step P e o' ∧ o = o'.inCells s := by
  intro s
  induction s with
  | nil => exact fun h _ => ⟨_, h, rfl⟩
  | cons v vs ih =>
    intro e o h he
    obtain ⟨o', h', rfl⟩ := ih h cell_not_value
    cases h' with
    | cellBody h'' => exact ⟨_, h'', rfl⟩
    | cellValue hw => exact absurd hw he

theorem inCells_next : ∀ (s : List Expr) (e : Expr),
    (Outcome.next e).inCells s = .next (Expr.cells s e)
  | [], _ => rfl
  | v :: vs, e => inCells_next vs (.cell v e)

theorem inCells_panic : ∀ (s : List Expr) (k : PanicKind),
    (Outcome.panic k).inCells s = .panic k
  | [], _ => rfl
  | _ :: vs, k => inCells_panic vs k

theorem inCells_ret : ∀ (s : List Expr) (r : Expr), (Outcome.ret r).inCells s = .ret r
  | [], _ => rfl
  | _ :: vs, r => inCells_ret vs r

theorem inCells_perform : ∀ (s : List Expr) (req : Request) (k : Expr → Expr),
    (Outcome.perform req k).inCells s = .perform req fun x => Expr.cells s (k x)
  | [], _, _ => rfl
  | v :: vs, req, k => inCells_perform vs req fun x => .cell v (k x)

/-- The cells answer a read of one of their vars with the value at its
path. -/
theorem inCells_read : ∀ (s : List Expr) {i : Nat} (is : List Nat) (k : Expr → Expr),
    i < s.length →
      (Outcome.read i is k).inCells s = .next (Expr.cells s (k ((s.getD i .unit).getAt is)))
  | [], _, _, _, h => absurd h (Nat.not_lt_zero _)
  | _ :: vs, 0, _, _, _ => inCells_next vs _
  | v :: vs, _ + 1, is, k, h =>
    inCells_read vs is (fun r => .cell v (k r)) (Nat.lt_of_succ_lt_succ h)

/-- The cells take a write of one of their vars into that var alone, at
its path. -/
theorem inCells_write : ∀ (s : List Expr) {i : Nat} (is : List Nat) (w e : Expr),
    i < s.length →
      (Outcome.write i is w e).inCells s =
        .next (Expr.cells (s.set i ((s.getD i .unit).setAt is w)) e)
  | [], _, _, _, _, h => absurd h (Nat.not_lt_zero _)
  | _ :: vs, 0, _, _, _, _ => inCells_next vs _
  | v :: vs, _ + 1, is, w, e, h =>
    inCells_write vs is w (.cell v e) (Nat.lt_of_succ_lt_succ h)

theorem cellDepth_cells : ∀ (s : List Expr) (e : Expr),
    (Expr.cells s e).cellDepth = s.length + e.cellDepth
  | [], _ => by simp [Expr.cells]
  | v :: vs, e => by
    rw [Expr.cells, cellDepth_cells vs, Expr.cellDepth, List.length_cons]
    omega

theorem cellDepth_value {v : Expr} (hv : IsValue v) : v.cellDepth = 0 := by
  cases hv <;> rfl

theorem cells_inj : ∀ (a b : List Expr) {e e' : Expr}, a.length = b.length →
    Expr.cells a e = Expr.cells b e' → a = b ∧ e = e'
  | [], [], _, _, _, h => ⟨rfl, h⟩
  | v :: a, u :: b, _, _, hl, h => by
    obtain ⟨rfl, h'⟩ := cells_inj a b (Nat.succ.inj hl) h
    cases h'
    exact ⟨rfl, rfl⟩

/-- Cells around values are equal only with the same vars and value. -/
theorem cells_values_inj {a b : List Expr} {e e' : Expr} (he : IsValue e) (he' : IsValue e')
    (h : Expr.cells a e = Expr.cells b e') : a = b ∧ e = e' := by
  have hd := congrArg Expr.cellDepth h
  rw [cellDepth_cells, cellDepth_cells, cellDepth_value he, cellDepth_value he'] at hd
  exact cells_inj a b (by omega) h

/-! ## A mutating method's body, run on its own -/

/-- `ReceiverRun P r b r' w`: the body `b` of a mutating method, with the
receiver `r` as its only var, runs to the value `w` and leaves the
receiver `r'`. A read of the receiver gets the value at its path, a write
puts one there, and a return (`?`) ends the body with its value. -/
inductive ReceiverRun (P : Program) : Expr → Expr → Expr → Expr → Prop where
  | done {r w : Expr} : IsValue w → ReceiverRun P r w r w
  | next {r b b' r' w : Expr} : Step P b (.next b') → ReceiverRun P r b' r' w →
      ReceiverRun P r b r' w
  | read {r b r' w : Expr} {is : List Nat} {k : Expr → Expr} :
      Step P b (.read 0 is k) → ReceiverRun P r (k (r.getAt is)) r' w → ReceiverRun P r b r' w
  | write {r b b' r' w x : Expr} {is : List Nat} :
      Step P b (.write 0 is x b') → ReceiverRun P (r.setAt is x) b' r' w →
      ReceiverRun P r b r' w
  | perform {r b r' w x : Expr} {req : Request} {k : Expr → Expr} :
      Step P b (.perform req k) → req.Allows x → ReceiverRun P r (k x) r' w →
      ReceiverRun P r b r' w
  | ret {r b v : Expr} : Step P b (.ret v) → ReceiverRun P r b r v

theorem ReceiverRun.of_value {r v r' w : Expr} (hv : IsValue v) (h : ReceiverRun P r v r' w) :
    r' = r ∧ w = v := by
  cases h with
  | done _ => exact ⟨rfl, rfl⟩
  | next hs _ => exact absurd hs (value_no_step hv)
  | read hs _ => exact absurd hs (value_no_step hv)
  | write hs _ => exact absurd hs (value_no_step hv)
  | perform hs _ _ => exact absurd hs (value_no_step hv)
  | ret hs => exact absurd hs (value_no_step hv)

/-! ## The run of a mutating call -/

/-- The vars `s` have the types `Ms`, and each is a value. -/
inductive StoreTyped (P : Program) (R : Ty) : List Expr → List Ty → Prop where
  | nil : StoreTyped P R [] []
  | cons {v : Expr} {T : Ty} {s : List Expr} {Ms : List Ty} :
      IsValue v → Typed P R [] [] (.expr v T) → StoreTyped P R s Ms →
      StoreTyped P R (v :: s) (T :: Ms)

theorem StoreTyped.lookup {s : List Expr} {Ms : List Ty} (hs : StoreTyped P R s Ms) :
    ∀ {i : Nat} {T : Ty}, lookup Ms i = some T →
      i < s.length ∧ IsValue (s.getD i .unit) ∧ Typed P R [] [] (.expr (s.getD i .unit) T) := by
  induction hs with
  | nil => intro i T h; simp [OskaFormal.Core.lookup] at h
  | cons hv hvt _ ih =>
    intro i T h
    cases i with
    | zero =>
      simp [OskaFormal.Core.lookup] at h
      subst h
      exact ⟨by simp, hv, hvt⟩
    | succ i =>
      obtain ⟨hl, hv', ht⟩ := ih h
      exact ⟨by simp; omega, hv', ht⟩

section
variable (P) (pl : Place) (name : String) (vs : List Expr) (c : Expr) (s : List Expr)
  (body r₀ : Expr) (u : Ty)

/-- The vars after the call: `s`, with `pl.var` changed at `pl.fields`
to the receiver `r'`. -/
def afterCall (r' : Expr) : List Expr :=
  s.set pl.var ((s.getD pl.var .unit).setAt pl.fields r')

/-- Where a run of `{ pl.name(vs); c }` in the cells of `s` can be: about
to call; running the body `b` with the receiver `r`, from which the body's
own run ends as one from `r₀` and `body` does; written back, with the
call's value `w` or with the call done; and `c`, in fewer and fewer of the
cells as they end. -/
def CallState (X : Expr) : Prop :=
  X = Expr.cells s (.block [.expr (.mutCall name pl vs)] (some c)) ∨
  (∃ r b t, X = Expr.cells s (.block [.expr (.method pl r b)] (some c)) ∧
    IsValue r ∧ Typed P t [] [u] (.expr r u) ∧ Typed P t [] [u] (.expr b t) ∧
    ∀ r' w, ReceiverRun P r b r' w → ReceiverRun P r₀ body r' w) ∨
  (∃ r' w, ReceiverRun P r₀ body r' w ∧ IsValue w ∧
    X = Expr.cells (afterCall pl s r') (.block [.expr w] (some c))) ∨
  (∃ r' w, ReceiverRun P r₀ body r' w ∧
    X = Expr.cells (afterCall pl s r') (.block [] (some c))) ∨
  (∃ r' w k, ReceiverRun P r₀ body r' w ∧ X = Expr.cells ((afterCall pl s r').drop k) c)

/-- What a step from a `CallState` leads to is one too. -/
def CallOutcome (o : Outcome) : Prop :=
  match o with
  | .next X => CallState P pl name vs c s body r₀ u X
  | .perform req k => ∀ x, req.Allows x → CallState P pl name vs c s body r₀ u (k x)
  | _ => True

end

theorem block_not_value {ss : List Stmt} {v : Option Expr} : ¬ IsValue (.block ss v) :=
  fun h => nomatch h

theorem callState_step (hP : ProgramTyped P) {pl : Place} {name : String} {vs : List Expr}
    {c : Expr} {s : List Expr} {f : Function} {body : Expr} {u : Ty}
    (hc : IsValue c) (hvs : AllValues vs) (hlen : pl.var < s.length)
    (hf : P.find? name = some f) (hi : instantiate f.params vs f.body = some body)
    (hr₀ : IsValue ((s.getD pl.var .unit).getAt pl.fields) ∧
      Typed P f.ret [] [u] (.expr ((s.getD pl.var .unit).getAt pl.fields) u))
    (hbody : Typed P f.ret [] [u] (.expr body f.ret))
    {X : Expr} {o : Outcome}
    (hX : CallState P pl name vs c s body ((s.getD pl.var .unit).getAt pl.fields) u X)
    (hs : Step P X o) :
    CallOutcome P pl name vs c s body ((s.getD pl.var .unit).getAt pl.fields) u o := by
  rcases hX with rfl | ⟨r, b, t, rfl, hr, hrt, hbt, hrun⟩ | ⟨r', w, hrun, hw, rfl⟩ |
    ⟨r', w, hrun, rfl⟩ | ⟨r', w, k, hrun, rfl⟩
  · -- About to call: the call reads its receiver from its cell.
    obtain ⟨o', hs', rfl⟩ := step_cells_inv s hs block_not_value
    cases hs' with
    | blockExpr hv => exact absurd hv (fun h => nomatch h)
    | blockExprArg hm =>
      cases hm with
      | mutCallArg hvs' he => exact absurd he (value_no_step (AllValues.mid hvs))
      | mutCall hf' _ _ hi' =>
        rw [hf] at hf'
        cases hf'
        rw [hi] at hi'
        cases hi'
        show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.read pl.var pl.fields _).inCells s)
        rw [inCells_read s _ _ hlen]
        exact Or.inr (Or.inl ⟨_, _, f.ret, rfl, hr₀.1, hr₀.2, hbody, fun _ _ h => h⟩)
  · -- Running the body: its reads and writes stay in the receiver.
    obtain ⟨o', hs', rfl⟩ := step_cells_inv s hs block_not_value
    cases hs' with
    | blockExpr hv => exact absurd hv (fun h => nomatch h)
    | blockExprArg hm =>
      cases hm with
      | methodValue hw =>
        show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.write pl.var pl.fields r _).inCells s)
        rw [inCells_write s _ _ _ hlen]
        exact Or.inr (Or.inr (Or.inl ⟨r, b, hrun r b (.done hw), hw, rfl⟩))
      | methodBody hb =>
        have ho := preservation hP hb hbt
        rename_i ob
        have state : ∀ r' b', IsValue r' → Typed P t [] [u] (.expr r' u) →
            Typed P t [] [u] (.expr b' t) →
            (∀ r'' w, ReceiverRun P r' b' r'' w → ReceiverRun P r b r'' w) →
            CallState P pl name vs c s body ((s.getD pl.var .unit).getAt pl.fields) u
              (Expr.cells s (.block [.expr (.method pl r' b')] (some c))) :=
          fun r' b' hr' hrt' hbt' hrun' =>
            Or.inr (Or.inl ⟨r', b', t, rfl, hr', hrt', hbt', fun r'' w h => hrun _ _ (hrun' _ _ h)⟩)
        cases ob with
        | next b' =>
          show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells s)
          rw [inCells_next]
          exact state r b' hr hrt ho fun _ _ h => .next hb h
        | panic k =>
          show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.panic k).inCells s)
          rw [inCells_panic]
          trivial
        | perform req k =>
          show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.perform req _).inCells s)
          rw [inCells_perform]
          exact fun x hx => state r (k x) hr hrt (ho x hx) fun _ _ h => .perform hb hx h
        | ret v =>
          show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells s)
          rw [inCells_next]
          refine state r v hr hrt (value_typed ho.1 ho.2) fun r'' w h => ?_
          obtain ⟨rfl, rfl⟩ := h.of_value ho.1
          exact .ret hb
        | read i is k =>
          obtain ⟨w', hw', hk⟩ := ho
          cases i with
          | zero =>
            show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells s)
            rw [inCells_next]
            rw [placeTy_zero] at hw'
            obtain ⟨hgv, hgt⟩ := getAt_typed is hr hrt hw'
            exact state r _ hr hrt (hk _ hgv (value_typed hgv hgt)) fun _ _ h => .read hb h
          | succ i => simp [placeTy_succ, placeTy_nil] at hw'
        | write i is x b' =>
          obtain ⟨u', hu', hx, hxt, hb'⟩ := ho
          cases i with
          | zero =>
            show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells s)
            rw [inCells_next]
            rw [placeTy_zero] at hu'
            obtain ⟨hsv, hst⟩ := setAt_typed is hr hrt hu' hx (value_typed hx hxt)
            exact state _ b' hsv hst hb' fun _ _ h => .write hb h
          | succ i => simp [placeTy_succ, placeTy_nil] at hu'
  · -- Written back: the call's value is dropped.
    obtain ⟨o', hs', rfl⟩ := step_cells_inv _ hs block_not_value
    cases hs' with
    | blockExprArg hw' => exact absurd hw' (value_no_step hw)
    | blockExpr _ =>
      show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells _)
      rw [inCells_next]
      exact Or.inr (Or.inr (Or.inr (Or.inl ⟨r', w, hrun, rfl⟩)))
  · -- The block's value is `c`.
    obtain ⟨o', hs', rfl⟩ := step_cells_inv _ hs block_not_value
    cases hs' with
    | blockSome =>
      show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells _)
      rw [inCells_next]
      exact Or.inr (Or.inr (Or.inr (Or.inr ⟨r', w, 0, hrun, by simp⟩)))
  · -- The cells end, innermost first.
    cases hd : (afterCall pl s r').drop k with
    | nil =>
      rw [hd] at hs
      exact absurd hs (value_no_step hc)
    | cons v t =>
      rw [hd] at hs
      obtain ⟨o', hs', rfl⟩ := step_cells_inv t hs cell_not_value
      cases hs' with
      | cellBody hc' => exact absurd hc' (value_no_step hc)
      | cellValue _ =>
        show CallOutcome _ _ _ _ _ _ _ _ _ ((Outcome.next _).inCells _)
        rw [inCells_next]
        refine Or.inr (Or.inr (Or.inr (Or.inr ⟨r', w, k + 1, hrun, ?_⟩)))
        rw [← List.tail_drop, hd]
        rfl

/-- Every expression a run from a `CallState` reaches is one too. -/
theorem callState_steps (hP : ProgramTyped P) {pl : Place} {name : String} {vs : List Expr}
    {c : Expr} {s : List Expr} {f : Function} {body : Expr} {u : Ty}
    (hc : IsValue c) (hvs : AllValues vs) (hlen : pl.var < s.length)
    (hf : P.find? name = some f) (hi : instantiate f.params vs f.body = some body)
    (hr₀ : IsValue ((s.getD pl.var .unit).getAt pl.fields) ∧
      Typed P f.ret [] [u] (.expr ((s.getD pl.var .unit).getAt pl.fields) u))
    (hbody : Typed P f.ret [] [u] (.expr body f.ret))
    {X Y : Expr} {tr : List Request} (hrun : Steps P X tr Y) :
    CallState P pl name vs c s body ((s.getD pl.var .unit).getAt pl.fields) u X →
      CallState P pl name vs c s body ((s.getD pl.var .unit).getAt pl.fields) u Y := by
  induction hrun with
  | refl => exact id
  | next h _ ih => exact fun hX => ih (callState_step hP hc hvs hlen hf hi hr₀ hbody hX h)
  | perform h hx _ ih =>
    exact fun hX => ih (callState_step hP hc hvs hlen hf hi hr₀ hbody hX h _ hx)

/-- **A mutating method changes only its receiver.** In a well-typed
program, run `{ pl.name(vs); c }` (`vs` and `c` values, `c` perhaps a
`let` copy of a var taken before the call) in the cells of the typed vars
`s`, var 0 first. Every run that ends in the same cells, at a value, ends
at `c` itself, with every var but `pl.var` as it was and `pl.var` changed
only at `pl.fields`: to the receiver `r'` that the method's body leaves
when it runs on its own from the receiver read at `pl` before the call. -/
theorem mutating_call_changes_only_its_receiver (hP : ProgramTyped P) {pl : Place}
    {name : String} {vs : List Expr} {c : Expr} {s s' : List Expr} {Ms : List Ty}
    {t : Ty} {w : Expr} {tr : List Request}
    (hs : StoreTyped P R s Ms) (hvs : AllValues vs) (hc : IsValue c)
    (ht : Typed P R [] Ms (.expr (.block [.expr (.mutCall name pl vs)] (some c)) t))
    (hrun : Steps P (Expr.cells s (.block [.expr (.mutCall name pl vs)] (some c))) tr
      (Expr.cells s' w))
    (hw : IsValue w) (hlen : s'.length = s.length) :
    w = c ∧ ∃ f body r' w', P.find? name = some f ∧
      instantiate f.params vs f.body = some body ∧
      ReceiverRun P ((s.getD pl.var .unit).getAt pl.fields) body r' w' ∧
      s' = s.set pl.var ((s.getD pl.var .unit).setAt pl.fields r') := by
  cases ht with
  | block hb =>
    cases hb with
    | blockExpr hm _ =>
      cases hm with
      | @mutCall _ _ _ _ f _ u _ hf hr hpl hargs =>
        obtain ⟨body, hi⟩ := instantiate_some _ hargs
        have hb := hP _ _ hf
        rw [hr] at hb
        have hbody : Typed P f.ret [] [u] (.expr body f.ret) :=
          typed_instantiate _ hb hargs hvs hi
        unfold Program.placeTy at hpl
        cases hT : OskaFormal.Core.lookup Ms pl.var with
        | none => rw [hT] at hpl; cases hpl
        | some T =>
          rw [hT] at hpl
          obtain ⟨hlt, hv, hvt⟩ := hs.lookup hT
          obtain ⟨hgv, hgt⟩ := getAt_typed pl.fields hv hvt hpl
          have hY := callState_steps hP hc hvs hlt hf hi ⟨hgv, value_typed hgv hgt⟩ hbody hrun
            (Or.inl rfl)
          rcases hY with h | ⟨_, _, _, h, _⟩ | ⟨_, _, _, _, h⟩ | ⟨_, _, _, h⟩ |
            ⟨r', w', k, hrun', h⟩
          · exact absurd ((cells_inj _ _ hlen h).2 ▸ hw) block_not_value
          · exact absurd ((cells_inj _ _ hlen h).2 ▸ hw) block_not_value
          · exact absurd ((cells_inj _ _ (by simp [afterCall, hlen]) h).2 ▸ hw)
              block_not_value
          · exact absurd ((cells_inj _ _ (by simp [afterCall, hlen]) h).2 ▸ hw)
              block_not_value
          · obtain ⟨hd, rfl⟩ := cells_values_inj hw hc h
            refine ⟨rfl, f, body, r', w', hf, hi, hrun', ?_⟩
            have hl : ((afterCall pl s r').drop k).length = (afterCall pl s r').length := by
              rw [← hd]; simp [afterCall, hlen]
            rw [hd]
            show _ = afterCall pl s r'
            simp only [List.length_drop] at hl
            have : k = 0 ∨ (afterCall pl s r').length = 0 := by omega
            rcases this with rfl | h0
            · rfl
            · rw [List.length_eq_zero_iff] at h0
              rw [h0]
              simp

end OskaFormal.Core
