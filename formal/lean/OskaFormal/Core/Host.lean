import OskaFormal.Core.Semantics

/-!
# Oska Core: the fake host

The host the Lean reference runs a program on: a model of the
interpreter's `FakeHost` (`crates/oska-run/src/host.rs`) as
`FakeHost::new()` makes it, with the confinement of
`crates/oska-run/src/confine.rs`. It answers each effect a run performs
(`Request`), and `perform_allows` proves that every answer is one the
semantics allows, so the theorems about `Steps` hold of the reference's
runs.

- Its stdout is the text written so far.
- Its filesystem is in memory: files by path (the components below `/`),
  every ancestor of a file a directory, and the directories `create_dir`
  made. `/` is a directory, and nothing else exists at first. There are no
  symlinks: `FakeHost::new()` has none, and Oska Core cannot make one, so
  the walks never meet one.
- Paths are split on `/` as Rust's `Path::components` splits them on Unix:
  empty components and `.` are dropped. A path with another separator
  (`\` on Windows) is not modelled; no program of the corpus has one.

A capability's root is a `CapRoot`. `restrict_to` from a root anywhere
opens the normalized path, as `FakeHost::open_root` does; from a
directory root it walks the path's names beneath it, as `confine::names`
and `FakeHost::open_beneath` do, so its result is the root followed by
those names, or nowhere. `attenuation_never_widens` (`Authority.lean`)
proves the new root reaches no path the old one does not.
-/

namespace OskaFormal.Core

/-- What the fake host holds: what the program printed, its files and
the directories it created. -/
structure Host where
  stdout : String := ""
  files : List (List String × String) := []
  dirs : List (List String) := []

namespace Host

/-- Whether `p` is a directory: `/`, a prefix of a directory made, or a
proper prefix of a file (`FakeHost::is_dir`). -/
def isDir (h : Host) (p : List String) : Bool :=
  p.isEmpty || h.dirs.any (p.isPrefixOf ·) || h.files.any fun (f, _) => p.isPrefixOf f && f != p

/-- Whether a file is at `p`. -/
def hasFile (h : Host) (p : List String) : Bool :=
  h.files.any (·.1 == p)

/-- A directory root at `p`, or nowhere when a file is there or nothing
is (`FakeHost::root_at`). -/
def rootAt (h : Host) (p : List String) : CapRoot :=
  if h.hasFile p then .nowhere else if h.isDir p then .beneath p else .nowhere

/-- The non-empty components of `s` between `/`s. -/
def parts (s : String) : List String :=
  (s.splitOn "/").filter (· ≠ "")

/-- `s` against `/`, with `.` dropped and `..` dropping the name before
it (`normalize` in `host.rs`). -/
def normalize (s : String) : List String :=
  (parts s).foldl (fun acc c => if c = "." then acc else if c = ".." then acc.dropLast else acc ++ [c]) []

/-- The plain names `s` walks beneath a root, or `none` when it is
refused: empty, absolute, or climbing above the root (`confine::names`).
-/
def names (s : String) : Option (List String) :=
  if s.isEmpty || s.startsWith "/" then none
  else
    (parts s).foldlM (fun acc c =>
      if c = "." then some acc
      else if c = ".." then (if acc.isEmpty then none else some acc.dropLast)
      else some (acc ++ [c])) []

/-- Whether `s` ends in a plain name: not in `/`, `.` or `..`
(`confine::ends_in_a_name`). -/
def endsInAName (s : String) : Bool :=
  !s.endsWith "/" &&
    match (s.splitOn "/").getLast? with
    | some n => n != "" && n != "." && n != ".."
    | none => false

/-- The names a write or `create_dir` of `s` walks (`confine::names_to_create`). -/
def namesToCreate (s : String) : Option (List String) :=
  if endsInAName s then names s else none

/-- The path `ns` reach beneath `root`, or `none` when a name before the
last is not a directory (`FakeHost::walk`). -/
def walk (h : Host) (root : List String) : List String → Option (List String)
  | [] => some root
  | [n] => some (root ++ [n])
  | n :: m :: rest => if h.isDir (root ++ [n]) then h.walk (root ++ [n]) (m :: rest) else none

/-- The root `restrict_to(s)` gives on a capability whose root is `root`
(`restricted` in `intrinsics.rs`). -/
def restrictTo (h : Host) (root : CapRoot) (s : String) : CapRoot :=
  match root with
  | .anywhere => if s.isEmpty then .nowhere else h.rootAt (normalize s)
  | .beneath p =>
    match names s with
    | none => .nowhere
    | some [] => .beneath p
    | some ns =>
      match h.walk p ns with
      | some q => h.rootAt q
      | none => .nowhere
  | .nowhere => .nowhere

/-- The text of the file at `s` beneath `root` (`fs_read_to_string`). -/
def read (h : Host) (root : CapRoot) (s : String) : Expr :=
  match root, names s with
  | .beneath p, some (n :: ns) =>
    match h.walk p (n :: ns) with
    | some q =>
      match h.files.lookup q with
      | some text => .okStr text
      | none => .err
    | none => .err
  | _, _ => .err

/-- The host with the file at `q` holding `text`. -/
def putFile (h : Host) (q : List String) (text : String) : Host :=
  { h with files := h.files.filter (·.1 != q) ++ [(q, text)] }

/-- Write `text` to the file at `s` beneath `root`, in a directory that
exists (`fs_write`, `FakeHost::put_file`). -/
def write (h : Host) (root : CapRoot) (s text : String) : Expr × Host :=
  match root, namesToCreate s with
  | .beneath p, some (n :: ns) =>
    match h.walk p (n :: ns) with
    | some q =>
      if h.isDir q.dropLast && !h.isDir q then (.okUnit, h.putFile q text) else (.err, h)
    | none => (.err, h)
  | _, _ => (.err, h)

/-- Create the directory at `s` beneath `root`, in a directory that
exists; one already there is kept (`fs_create_dir`, `FakeHost::put_dir`). -/
def createDir (h : Host) (root : CapRoot) (s : String) : Expr × Host :=
  match root, namesToCreate s with
  | .beneath p, some (n :: ns) =>
    match h.walk p (n :: ns) with
    | some q =>
      if h.hasFile q then (.err, h)
      else if h.isDir q then (.okUnit, h)
      else if h.isDir q.dropLast then (.okUnit, { h with dirs := h.dirs ++ [q] })
      else (.err, h)
    | none => (.err, h)
  | _, _ => (.err, h)

/-- The text a `String` or a `Path` argument holds. -/
def textOf : List Expr → Option String
  | [.str s] | [.path s] => some s
  | _ => none

/-- The host's answer to an effect, and the host after it. -/
def perform (h : Host) (req : Request) : Expr × Host :=
  match req.op with
  | .println =>
    match req.args with
    | [.str s] => (.okUnit, { h with stdout := h.stdout ++ s ++ "\n" })
    | _ => (.err, h)
  | .restrictTo _ =>
    match req.kind.restrictTo with
    | some (_, k') =>
      (.cap k' (match textOf req.args with
        | some s => h.restrictTo req.root s
        | none => .nowhere), h)
    | none => (.err, h)
  | .readToString =>
    match req.args with
    | [.path s] => (h.read req.root s, h)
    | _ => (.err, h)
  | .write =>
    match req.args with
    | [.path s, .str text] => h.write req.root s text
    | _ => (.err, h)
  | .createDir =>
    match req.args with
    | [.path s] => h.createDir req.root s
    | _ => (.err, h)
  | _ => (.err, h)

/-! ## The host's answers are the semantics' -/

theorem walk_prefix {h : Host} : ∀ {root q : List String} {ns : List String},
    h.walk root ns = some q → root <+: q := by
  intro root q ns
  induction ns generalizing root with
  | nil => intro hw; simp [walk] at hw; subst hw; exact List.prefix_refl _
  | cons n rest ih =>
    intro hw
    cases rest with
    | nil => simp [walk] at hw; subst hw; exact List.prefix_append _ _
    | cons m rest =>
      simp only [walk] at hw
      split at hw
      · exact List.IsPrefix.trans (List.prefix_append _ _) (ih hw)
      · cases hw

theorem rootAt_within {h : Host} {p q : List String} (hpq : p <+: q) :
    (h.rootAt q).Within (.beneath p) := by
  intro x hx
  unfold rootAt at hx
  split at hx
  · exact hx.elim
  · split at hx
    · exact List.IsPrefix.trans hpq hx
    · exact hx.elim

/-- `restrict_to` on the fake host never widens: the new root reaches no
path the old one does not. -/
theorem restrictTo_within (h : Host) (root : CapRoot) (s : String) :
    (h.restrictTo root s).Within root := by
  intro q hq
  cases root with
  | anywhere => trivial
  | nowhere => simp [restrictTo, CapRoot.Reaches] at hq
  | beneath p =>
    simp only [restrictTo] at hq
    split at hq
    · exact hq.elim
    · exact hq
    · split at hq
      · next q' hw => exact rootAt_within (walk_prefix hw) q hq
      · exact hq.elim

/-- A read on the fake host reads a file its capability's root reaches. -/
theorem read_beneath (h : Host) (root : CapRoot) (s : String) {t : String}
    (hr : h.read root s = .okStr t) : ∃ q, root.Reaches q ∧ h.files.lookup q = some t := by
  unfold read at hr
  split at hr
  · next p n ns _ =>
    split at hr
    · next q hw =>
      split at hr
      · next text hl => cases hr; exact ⟨q, walk_prefix hw, hl⟩
      · cases hr
    · cases hr
  · cases hr

/-- A write on the fake host leaves every file it did not write as it
was, and writes only a file its capability's root reaches. -/
theorem write_beneath (h : Host) (root : CapRoot) (s t : String) :
    ∀ q x, (q, x) ∈ (h.write root s t).2.files → (q, x) ∈ h.files ∨ root.Reaches q := by
  intro q x hm
  unfold write at hm
  split at hm
  · next p n ns _ =>
    split at hm
    · next q' hw =>
      split at hm
      · simp only [putFile, List.mem_append, List.mem_filter, List.mem_singleton] at hm
        rcases hm with ⟨hm, -⟩ | hm
        · exact Or.inl hm
        · cases hm; exact Or.inr (walk_prefix hw)
      · exact Or.inl hm
    · exact Or.inl hm
  · exact Or.inl hm

/-- `create_dir` on the fake host makes only a directory its
capability's root reaches. -/
theorem createDir_beneath (h : Host) (root : CapRoot) (s : String) :
    ∀ q ∈ (h.createDir root s).2.dirs, q ∈ h.dirs ∨ root.Reaches q := by
  intro q hm
  unfold createDir at hm
  split at hm
  · next p n ns _ =>
    split at hm
    · next q' hw =>
      split at hm
      · exact Or.inl hm
      · split at hm
        · exact Or.inl hm
        · split at hm
          · simp only [List.mem_append, List.mem_singleton] at hm
            rcases hm with hm | hm
            · exact Or.inl hm
            · subst hm; exact Or.inr (walk_prefix hw)
          · exact Or.inl hm
    · exact Or.inl hm
  · exact Or.inl hm

theorem read_allowed (h : Host) (root : CapRoot) (s : String) :
    h.read root s = .err ∨ ∃ t, h.read root s = .okStr t := by
  unfold read
  split
  · split
    · split
      · exact Or.inr ⟨_, rfl⟩
      · exact Or.inl rfl
    · exact Or.inl rfl
  · exact Or.inl rfl

theorem write_allowed (h : Host) (root : CapRoot) (s t : String) :
    (h.write root s t).1 = .okUnit ∨ (h.write root s t).1 = .err := by
  unfold write
  split
  · split
    · split
      · exact Or.inl rfl
      · exact Or.inr rfl
    · exact Or.inr rfl
  · exact Or.inr rfl

theorem createDir_allowed (h : Host) (root : CapRoot) (s : String) :
    (h.createDir root s).1 = .okUnit ∨ (h.createDir root s).1 = .err := by
  unfold createDir
  split
  · split
    · split
      · exact Or.inr rfl
      · split
        · exact Or.inl rfl
        · split
          · exact Or.inl rfl
          · exact Or.inr rfl
    · exact Or.inr rfl
  · exact Or.inr rfl

/-- **The fake host answers as the semantics allows.** For an effect a
step performs (its receiver has the kind the call takes), the host's
answer is one `Request.Allows`. -/
theorem perform_allows (h : Host) {req : Request} (hk : req.op.receiver = some req.kind) :
    req.Allows (h.perform req).1 := by
  obtain ⟨op, kind, root, args⟩ := req
  cases op with
  | println =>
    simp only [perform, Request.Allows]
    split
    · exact Or.inl rfl
    · exact Or.inr rfl
  | restrictTo k =>
    simp only [Builtin.receiver] at hk
    cases hr : k.restrictTo with
    | none => simp [hr] at hk
    | some ak =>
      obtain ⟨a, k'⟩ := ak
      simp [hr] at hk
      subst hk
      simp only [perform, Request.Allows, hr]
      refine ⟨a, k', _, rfl, rfl, ?_⟩
      split
      · exact restrictTo_within _ _ _
      · intro q hq; exact hq.elim
  | readToString =>
    simp only [perform, Request.Allows]
    split
    · rcases read_allowed h root _ with h' | ⟨t, h'⟩
      · exact Or.inl h'
      · exact Or.inr ⟨t, h'⟩
    · exact Or.inl rfl
  | write =>
    simp only [perform, Request.Allows]
    split
    · exact write_allowed _ _ _ _
    · exact Or.inr rfl
  | createDir =>
    simp only [perform, Request.Allows]
    split
    · exact createDir_allowed _ _ _
    · exact Or.inr rfl
  | _ => simp [Builtin.receiver] at hk

end Host

end OskaFormal.Core
