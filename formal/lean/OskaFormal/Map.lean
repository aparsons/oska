/-!
# The order and update rules of `std.map`'s `Map`

ADR 0028 fixes what a `Map<K, V>` (`use std.map`) does with its entries:

- `entries()`, `keys()` and `values()` walk the entries in insertion order;
- `put` on a key the map holds keeps that key's place (an update);
- `put` on a new key goes last;
- `remove` and then `put` of the same key moves it to the end.

This file states those rules twice and proves they agree.

1. `Map` (the abstract map) is the list of entries in insertion order. Its
   `put` and `remove` are the rules above, and `put_new_goes_last`,
   `put_held_keeps_place` and `remove_then_put_moves_to_end` are the three
   rules as theorems.
2. `Layout` is the layout of `crates/oska-check/src/std/map.osk`: `log` is
   the array of entries, only ever added to, and `order` holds for each
   ordinal of the map the place of its current entry in `log`, or `none`
   once it is removed (the file keeps the place plus 1 in a `Bytes`, and 0
   for a removed entry). An update adds a new entry to `log` and points the
   key's ordinal at it. `Layout.entries` reads the entries in ordinal order,
   so no hash comes into it, and `Layout.put_entries` and
   `Layout.remove_entries` prove that a `put` and a `remove` on the layout
   are the abstract `put` and `remove`, for a layout that is well formed
   (`Layout.WF`: every place in `order` is in `log`), which `new`, `put`
   and `remove` all keep.

The slot table (`table` in `map.osk`), the probe limit (`P0045`), the entry
limit (`P0044`) and the rebuild only decide where a key is found, and a
rebuild keeps the live entries in the same order, so they are not modelled
here; the differential tests (`crates/oska-run/tests/map_order.rs`,
`map_limits.rs`) cover them.

The gap: `Layout` is copied by hand from `map.osk`; no proof links the Lean
`Layout` to the Oska code. The differential tests (`map_order.rs` checks
random `put` and `remove` sequences entry by entry against a plain list)
carry that link. `Layout` also does not state that its keys are unique; the
abstract `put_nodup` shows the rules keep keys unique, but it is not
proved for `Layout`.
-/

namespace OskaFormal.MapModel

variable {K V : Type} [DecidableEq K]

/-! ## The abstract map -/

/-- A map is its entries, in insertion order. -/
abbrev Map (K V : Type) := List (K × V)

/-- The keys, in insertion order. -/
def Map.keys (m : Map K V) : List K := m.map (·.1)

/-- The value of a key, or `none`: a missing key is never a panic. -/
def Map.get (m : Map K V) (k : K) : Option V :=
  (m.find? (fun e => decide (e.1 = k))).map (·.2)

/-- `put`: a held key keeps its place and takes the new value, a new key
goes last. -/
def Map.put : Map K V → K → V → Map K V
  | [], k, v => [(k, v)]
  | (k', v') :: rest, k, v =>
    if k' = k then (k, v) :: rest else (k', v') :: Map.put rest k v

/-- `remove`: the other entries keep their order. -/
def Map.remove (m : Map K V) (k : K) : Map K V :=
  m.filter (fun e => decide (e.1 ≠ k))

theorem put_new_goes_last {m : Map K V} {k : K} (v : V) (h : k ∉ Map.keys m) :
    m.put k v = m ++ [(k, v)] := by
  induction m with
  | nil => rfl
  | cons e rest ih =>
    obtain ⟨k', v'⟩ := e
    have hne : k' ≠ k := fun heq => h (by simp [Map.keys, heq])
    have hrest : k ∉ Map.keys rest := fun hm => h (by simp [Map.keys] at hm ⊢; exact Or.inr hm)
    simp [Map.put, hne, ih hrest]

theorem put_held_keeps_place {m : Map K V} {k : K} (v : V) (h : k ∈ Map.keys m) :
    Map.keys (m.put k v) = Map.keys m := by
  induction m with
  | nil => simp [Map.keys] at h
  | cons e rest ih =>
    obtain ⟨k', v'⟩ := e
    by_cases hk : k' = k
    · simp [Map.put, hk, Map.keys]
    · have hrest : k ∈ Map.keys rest := by
        simp [Map.keys] at h ⊢
        rcases h with h | h
        · exact absurd h.symm hk
        · exact h
      have := ih hrest
      simp [Map.put, hk, Map.keys] at this ⊢
      exact this

theorem put_new_keys {m : Map K V} {k : K} (v : V) (h : k ∉ Map.keys m) :
    Map.keys (m.put k v) = Map.keys m ++ [k] := by
  rw [put_new_goes_last v h]
  simp [Map.keys]

theorem put_get {m : Map K V} (k : K) (v : V) : (m.put k v).get k = some v := by
  induction m with
  | nil => simp [Map.put, Map.get]
  | cons e rest ih =>
    obtain ⟨k', v'⟩ := e
    by_cases hk : k' = k
    · simp [Map.put, hk, Map.get]
    · simp [Map.put, hk, Map.get] at ih ⊢
      exact ih

theorem remove_keys (m : Map K V) (k : K) :
    Map.keys (m.remove k) = (Map.keys m).filter (fun x => decide (x ≠ k)) := by
  simp [Map.remove, Map.keys, List.filter_map, Function.comp_def]

theorem not_mem_remove_keys (m : Map K V) (k : K) : k ∉ Map.keys (m.remove k) := by
  simp [Map.remove, Map.keys]

/-- `remove` then `put` of the same key puts the key last, and the other
entries keep their order. -/
theorem remove_then_put_moves_to_end (m : Map K V) (k : K) (v : V) :
    ((m.remove k).put k v) = m.remove k ++ [(k, v)] :=
  put_new_goes_last v (not_mem_remove_keys m k)

theorem remove_then_put_last_key (m : Map K V) (k : K) (v : V) :
    Map.keys ((m.remove k).put k v) = (Map.keys m).filter (fun x => decide (x ≠ k)) ++ [k] := by
  rw [remove_then_put_moves_to_end]
  simp [Map.keys, Map.remove, List.filter_map, Function.comp_def]

/-- `put` keeps the keys distinct. -/
theorem put_nodup {m : Map K V} (k : K) (v : V) (h : (Map.keys m).Nodup) :
    (Map.keys (m.put k v)).Nodup := by
  by_cases hk : k ∈ Map.keys m
  · rw [put_held_keeps_place v hk]; exact h
  · rw [put_new_keys v hk]
    exact List.nodup_append.mpr ⟨h, by simp, by
      intro a ha b hb heq
      simp at hb
      subst hb
      subst heq
      exact hk ha⟩

/-! ## The layout of `map.osk` -/

/-- The log of entries (only ever added to) and, for each ordinal, the place
of its current entry in the log, `none` once it is removed. -/
structure Layout (K V : Type) where
  log : List (K × V)
  order : List (Option Nat)

/-- The entry an `order` slot names. -/
def ent (log : List (K × V)) : Option Nat → Option (K × V)
  | none => none
  | some i => log[i]?

/-- The entries, in ordinal order. -/
def Layout.entries (l : Layout K V) : Map K V := l.order.filterMap (ent l.log)

/-- Every place in `order` is in `log`. -/
def Layout.WF (l : Layout K V) : Prop := ∀ i, some i ∈ l.order → i < l.log.length

/-- Whether an `order` slot holds key `k`. -/
def hit (log : List (K × V)) (k : K) (o : Option Nat) : Bool :=
  match ent log o with
  | some e => decide (e.1 = k)
  | none => false

/-- The slots after a `put` of `k`, whose new entry is at place `n`: the
slot of a held key points at `n`, and a new key gets a slot at the end. -/
def putOrder (log : List (K × V)) (k : K) (n : Nat) : List (Option Nat) → List (Option Nat)
  | [] => [some n]
  | o :: os => if hit log k o then some n :: os else o :: putOrder log k n os

/-- The slots after a `remove` of `k`: its slot is `none`. -/
def removeOrder (log : List (K × V)) (k : K) : List (Option Nat) → List (Option Nat)
  | [] => []
  | o :: os => (if hit log k o then none else o) :: removeOrder log k os

def Layout.empty : Layout K V := ⟨[], []⟩

/-- `put`: add the entry to the log, then point the key's slot at it (or add
a slot at the end). -/
def Layout.put (l : Layout K V) (k : K) (v : V) : Layout K V :=
  ⟨l.log ++ [(k, v)], putOrder l.log k l.log.length l.order⟩

/-- `remove`: the key's slot becomes `none`; the log is untouched. -/
def Layout.remove (l : Layout K V) (k : K) : Layout K V :=
  ⟨l.log, removeOrder l.log k l.order⟩

omit [DecidableEq K] in
theorem ent_append {log : List (K × V)} (x : K × V) {o : Option Nat}
    (h : ∀ i, o = some i → i < log.length) : ent (log ++ [x]) o = ent log o := by
  cases o with
  | none => rfl
  | some i =>
    have := h i rfl
    simp [ent, List.getElem?_append_left this]

omit [DecidableEq K] in
theorem filterMap_ent_append {log : List (K × V)} (x : K × V) {os : List (Option Nat)}
    (h : ∀ i, some i ∈ os → i < log.length) :
    os.filterMap (ent (log ++ [x])) = os.filterMap (ent log) := by
  induction os with
  | nil => rfl
  | cons o rest ih =>
    have ho : ∀ i, o = some i → i < log.length := fun i hi => h i (by simp [hi])
    have hr : ∀ i, some i ∈ rest → i < log.length := fun i hi => h i (by simp [hi])
    simp [List.filterMap_cons, ent_append x ho, ih hr]

omit [DecidableEq K] in
theorem Layout.WF.empty : (Layout.empty : Layout K V).WF := by
  intro i h
  simp [Layout.empty] at h

omit [DecidableEq K] in
theorem empty_entries : (Layout.empty : Layout K V).entries = [] := rfl

theorem putOrder_entries (log : List (K × V)) (k : K) (v : V) :
    ∀ os : List (Option Nat), (∀ i, some i ∈ os → i < log.length) →
      (putOrder log k log.length os).filterMap (ent (log ++ [(k, v)])) =
        Map.put (os.filterMap (ent log)) k v := by
  intro os
  induction os with
  | nil =>
    intro _
    simp [putOrder, Map.put, ent]
  | cons o rest ih =>
    intro h
    have hr : ∀ i, some i ∈ rest → i < log.length := fun i hi => h i (by simp [hi])
    have ho : ∀ i, o = some i → i < log.length := fun i hi => h i (by simp [hi])
    cases o with
    | none =>
      simp [putOrder, hit, ent, List.filterMap_cons, ih hr]
    | some i =>
      have hi := ho i rfl
      obtain ⟨e, he⟩ : ∃ e, log[i]? = some e := ⟨log[i], by simp [hi]⟩
      obtain ⟨k', v'⟩ := e
      by_cases hk : k' = k
      · simp [putOrder, hit, ent, he, hk, Map.put, 
          filterMap_ent_append (k, v) hr]
      · simp [putOrder, hit, ent, he, hk, Map.put, 
          List.getElem?_append_left hi, ih hr]

theorem removeOrder_entries (log : List (K × V)) (k : K) :
    ∀ os : List (Option Nat),
      (removeOrder log k os).filterMap (ent log) = Map.remove (os.filterMap (ent log)) k := by
  intro os
  induction os with
  | nil => rfl
  | cons o rest ih =>
    cases o with
    | none => simp [removeOrder, hit, ent, List.filterMap_cons, ih]
    | some i =>
      cases he : log[i]? with
      | none => simp [removeOrder, hit, ent, he, ih]
      | some e =>
        by_cases hk : e.1 = k
        · simp [removeOrder, hit, ent, he, hk, Map.remove] at ih ⊢
          exact ih
        · simp [removeOrder, hit, ent, he, hk, Map.remove] at ih ⊢
          exact ih

theorem Layout.WF.put {l : Layout K V} (h : l.WF) (k : K) (v : V) : (l.put k v).WF := by
  have key : ∀ os : List (Option Nat), (∀ i, some i ∈ os → i < l.log.length) →
      ∀ i, some i ∈ putOrder l.log k l.log.length os → i < (l.log ++ [(k, v)]).length := by
    intro os
    induction os with
    | nil => intro _ i hi; simp [putOrder] at hi; simp [hi]
    | cons o rest ih =>
      intro hos i hi
      have hr : ∀ i, some i ∈ rest → i < l.log.length := fun i hi => hos i (by simp [hi])
      have ho : ∀ i, o = some i → i < l.log.length := fun i hi => hos i (by simp [hi])
      by_cases hh : hit l.log k o = true
      · simp [putOrder, hh] at hi
        rcases hi with hi | hi
        · simp [hi]
        · have := hr i hi; simp; omega
      · simp [putOrder, hh] at hi
        rcases hi with hi | hi
        · have := ho i hi.symm; simp; omega
        · exact ih hr i (by simpa [putOrder] using hi)
  exact key l.order h

theorem Layout.WF.remove {l : Layout K V} (h : l.WF) (k : K) : (l.remove k).WF := by
  have key : ∀ os : List (Option Nat), (∀ i, some i ∈ os → i < l.log.length) →
      ∀ i, some i ∈ removeOrder l.log k os → i < l.log.length := by
    intro os
    induction os with
    | nil => intro _ i hi; simp [removeOrder] at hi
    | cons o rest ih =>
      intro hos i hi
      have hr : ∀ i, some i ∈ rest → i < l.log.length := fun i hi => hos i (by simp [hi])
      have ho : ∀ i, o = some i → i < l.log.length := fun i hi => hos i (by simp [hi])
      simp only [removeOrder, List.mem_cons] at hi
      rcases hi with hi | hi
      · by_cases hh : hit l.log k o = true
        · simp [hh] at hi
        · simp [hh] at hi; exact ho i hi.symm
      · exact ih hr i hi
  exact key l.order h

/-- A `put` on the layout is the abstract `put`: a held key keeps its
place, a new key goes last. -/
theorem Layout.put_entries {l : Layout K V} (h : l.WF) (k : K) (v : V) :
    (l.put k v).entries = Map.put l.entries k v :=
  putOrder_entries l.log k v l.order h

/-- A `remove` on the layout is the abstract `remove`. -/
theorem Layout.remove_entries (l : Layout K V) (k : K) :
    (l.remove k).entries = Map.remove l.entries k :=
  removeOrder_entries l.log k l.order

/-- On the layout itself: `remove` then `put` of the same key puts the key
last. -/
theorem Layout.remove_then_put_entries {l : Layout K V} (h : l.WF) (k : K) (v : V) :
    ((l.remove k).put k v).entries = Map.remove l.entries k ++ [(k, v)] := by
  rw [Layout.put_entries (h.remove k), Layout.remove_entries]
  exact remove_then_put_moves_to_end _ k v

end OskaFormal.MapModel
