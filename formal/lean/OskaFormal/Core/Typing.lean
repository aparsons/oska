import OskaFormal.Core.Syntax

/-!
# Oska Core: typing

The typing rules of Oska Core, version 2. A context lists the types of the
slots in scope, slot 0 first; a `let` that binds adds its slot at the end,
and it leaves scope with its block. A `match` arm's pattern adds its
bindings' slots at the end, for its guard and body.

The rules for expressions, argument lists, blocks and `match` arms form
one inductive judgment, `Typed`, indexed by a `Goal`, so that a proof about
typing is one induction. Patterns (`PatTyped`) and values (`ValueTyped`)
have judgments of their own, indexed the same way.

Exhaustiveness is checked statically, as the checker does
(`crates/oska-check/src/usefulness.rs`): `exhaustiveArms` asks whether the
patterns of the arms with no guard cover every value of the type their
heads name (a wildcard or binding, every variant of an enum, the struct,
both `Bool`s, `()`; never all of `Int`), field by field. A program as the
checker hands it over is `SourceTyped`: `Typed`, with every `match` passing
`exhaustiveArms`. `Typed` alone also types the states a run reaches, where
a `match` whose scrutinee is already a value may have lost the arms it
skipped: its rule takes either `exhaustiveArms` or, for a value scrutinee
only, that this value matches an arm with no guard (`Covered`).
`exhaustiveArms_sound` (`Soundness.lean`) proves that the static check
gives the second, for every value of the scrutinee's type.
-/

namespace OskaFormal.Core

/-- The type of slot `slot` in a context, if it is in scope. -/
def lookup : List Ty → Nat → Option Ty
  | [], _ => none
  | t :: _, 0 => some t
  | _ :: ts, n + 1 => lookup ts n

/-- The types of the slots a function's parameters bind, slot 0 first. -/
def bindingTypes : List Param → List Ty
  | [] => []
  | p :: ps => if p.binds then p.type :: bindingTypes ps else bindingTypes ps

/-- `op` takes two operands of type `arg` and gives a result of type `res`:
arithmetic on `Int`, comparison of `Int`s, `==` and `!=` on two values of
one type, and `&&`, `||` and `implies` on `Bool`. -/
inductive BinOpTyped : BinOp → Ty → Ty → Prop where
  | add : BinOpTyped .add .int .int
  | sub : BinOpTyped .sub .int .int
  | mul : BinOpTyped .mul .int .int
  | div : BinOpTyped .div .int .int
  | rem : BinOpTyped .rem .int .int
  | lt : BinOpTyped .lt .int .bool
  | le : BinOpTyped .le .int .bool
  | gt : BinOpTyped .gt .int .bool
  | ge : BinOpTyped .ge .int .bool
  | eq (t : Ty) : t.isBase = true → BinOpTyped .eq t .bool
  | ne (t : Ty) : t.isBase = true → BinOpTyped .ne t .bool
  | and : BinOpTyped .and .bool .bool
  | or : BinOpTyped .or .bool .bool
  | implies : BinOpTyped .implies .bool .bool

/-- What a value typing is about: a value and its type, or a list of
values and their types. -/
inductive VGoal where
  | one (v : Expr) (t : Ty)
  | many (vs : List Expr) (ts : List Ty)

/-- The types of the values of Oska Core in program `P`: an `Int` in
range, a boolean, `()`, and a struct or variant `P` declares whose fields
have its field types. -/
inductive ValueTyped (P : Program) : VGoal → Prop where
  | unit : ValueTyped P (.one .unit .unit)
  | int {n : Int} : fits n = true → ValueTyped P (.one (.int n) .int)
  | bool {b : Bool} : ValueTyped P (.one (.bool b) .bool)
  | construct {type : String} {variant : Option String} {fs : List Expr} {fts : List Ty} :
      P.fieldsOf type variant = some fts → ValueTyped P (.many fs fts) →
      ValueTyped P (.one (.construct type variant fs) (.named type))
  | nil : ValueTyped P (.many [] [])
  | cons {v : Expr} {vs : List Expr} {t : Ty} {ts : List Ty} :
      ValueTyped P (.one v t) → ValueTyped P (.many vs ts) → ValueTyped P (.many (v :: vs) (t :: ts))

/-- What a pattern typing is about: a pattern, the type of the value it
matches and the types of the slots it binds, or the same for a list of
patterns. -/
inductive PGoal where
  | one (p : Pattern) (t : Ty) (binds : List Ty)
  | many (ps : List Pattern) (ts : List Ty) (binds : List Ty)

/-- The typing of patterns in program `P`. -/
inductive PatTyped (P : Program) : PGoal → Prop where
  | wildcard {t : Ty} : PatTyped P (.one .wildcard t [])
  | bind {t : Ty} : PatTyped P (.one (.bind t) t [t])
  | unit : PatTyped P (.one .unit .unit [])
  | int {n : Int} : PatTyped P (.one (.int n) .int [])
  | bool {b : Bool} : PatTyped P (.one (.bool b) .bool [])
  | construct {type : String} {variant : Option String} {ps : List Pattern} {fts bs : List Ty} :
      P.fieldsOf type variant = some fts → PatTyped P (.many ps fts bs) →
      PatTyped P (.one (.construct type variant ps) (.named type) bs)
  | nil : PatTyped P (.many [] [] [])
  | cons {p : Pattern} {ps : List Pattern} {t : Ty} {ts bs bss : List Ty} :
      PatTyped P (.one p t bs) → PatTyped P (.many ps ts bss) →
      PatTyped P (.many (p :: ps) (t :: ts) (bs ++ bss))

/-- The value `v` matches an arm of `arms` that has no guard, so a `match`
of `v` on `arms` takes an arm. -/
def Covered (arms : List Arm) (v : Expr) : Prop :=
  ∃ p body, Arm.mk p none body ∈ arms ∧ (matchPat p v).isSome = true

/-! ## Static exhaustiveness -/

/-- A pattern that matches every value: `_` or a name. -/
def Pattern.isCatchAll : Pattern → Bool
  | .wildcard | .bind _ => true
  | _ => false

/-- The type a pattern's head shows; `none` for a catch-all. -/
def Pattern.headType : Pattern → Option Ty
  | .wildcard | .bind _ => none
  | .unit => some .unit
  | .int _ => some .int
  | .bool _ => some .bool
  | .construct type _ _ => some (.named type)

mutual
  /-- The number of nodes of a pattern. -/
  def Pattern.size : Pattern → Nat
    | .construct _ _ ps => 1 + Pattern.sizes ps
    | _ => 1

  def Pattern.sizes : List Pattern → Nat
    | [] => 0
    | p :: ps => p.size + Pattern.sizes ps
end

/-- A head of a pattern that a value's head is tested against: `()`, a
boolean, or a struct (no variant) or enum variant. -/
inductive Head where
  | unit
  | bool (b : Bool)
  | ctor (variant : Option String)

/-- The sub-patterns of `p` when its head is `h` with `k` fields; `none`
when it is not. -/
def Head.args : Head → Nat → Pattern → Option (List Pattern)
  | .unit, 0, .unit => some []
  | .bool b, 0, .bool c => if b = c then some [] else none
  | .ctor v, k, .construct _ v' ps => if v = v' ∧ ps.length = k then some ps else none
  | _, _, _ => none

/-- A row of a pattern matrix: one pattern per column. -/
abbrev Row := List Pattern

/-- The rows whose first pattern is a catch-all, without it. -/
def defaultRows : List Row → List Row
  | [] => []
  | [] :: rows => defaultRows rows
  | (p :: rest) :: rows => if p.isCatchAll then rest :: defaultRows rows else defaultRows rows

/-- The rows that can match a first value with head `h` and `k` fields,
its fields' patterns put first: a catch-all as `k` wildcards. -/
def specialize (h : Head) (k : Nat) : List Row → List Row
  | [] => []
  | [] :: rows => specialize h k rows
  | (p :: rest) :: rows =>
    if p.isCatchAll then (List.replicate k .wildcard ++ rest) :: specialize h k rows
    else
      match h.args k p with
      | some ps => (ps ++ rest) :: specialize h k rows
      | none => specialize h k rows

/-- Whether the rows cover every list of values of the types `tys`, within
`fuel` steps (running out answers `false`, never a wrong `true`). A column
is covered by its catch-all rows alone, or else head by head: `()`, both
`Bool`s, the struct, or every variant of the enum; never `Int` by its
literals. -/
def covers (P : Program) : Nat → List Row → List Ty → Bool
  | 0, _, _ => false
  | _ + 1, rows, [] => rows.any List.isEmpty
  | fuel + 1, rows, t :: ts =>
    covers P fuel (defaultRows rows) ts ||
      match t with
      | .unit => covers P fuel (specialize .unit 0 rows) ts
      | .bool =>
        covers P fuel (specialize (.bool true) 0 rows) ts &&
          covers P fuel (specialize (.bool false) 0 rows) ts
      | .int => false
      | .named n =>
        match findType P.types n with
        | some (.struct_ _ fs) => covers P fuel (specialize (.ctor none) fs.length rows) (fs ++ ts)
        | some (.enum_ _ vs) =>
          vs.all fun (v, fs) => covers P fuel (specialize (.ctor (some v)) fs.length rows) (fs ++ ts)
        | none => false

/-- The patterns of the arms with no guard, in order. -/
def unguarded : List Arm → List Pattern
  | [] => []
  | .mk p none _ :: arms => p :: unguarded arms
  | .mk _ (some _) _ :: arms => unguarded arms

/-- The most fields a declared struct or variant has. -/
def Program.maxArity (P : Program) : Nat :=
  P.types.foldl (fun m d => match d with
    | .struct_ _ fs => max m fs.length
    | .enum_ _ vs => vs.foldl (fun m v => max m v.2.length) m) 0

/-- **Static exhaustiveness.** Whether the arms with no guard cover every
value of the type their patterns' heads name, or, when no pattern has a
head, whether there is an arm with no guard (whose pattern is then a
catch-all). The fuel is enough for any match the checker accepts: each
step drops a column or a constructor of the patterns, and a constructor
adds at most `maxArity` columns. -/
def exhaustiveArms (P : Program) (arms : List Arm) : Bool :=
  let ps := unguarded arms
  match ps.findSome? Pattern.headType with
  | none => !ps.isEmpty
  | some u => covers P ((Pattern.sizes ps + 1) * (P.maxArity + 2)) (ps.map ([·])) [u]

/-- What a typing judgment is about: an expression and its type, a list of
arguments and their types, a block's statements and value and the block's
type, or a `match`'s arms, the type of the value they match and the type
of their bodies. -/
inductive Goal where
  | expr (e : Expr) (t : Ty)
  | args (es : List Expr) (ts : List Ty)
  | block (stmts : List Stmt) (value : Option Expr) (t : Ty)
  | arms (arms : List Arm) (scrutinee : Ty) (t : Ty)

/-- The typing judgment of Oska Core, in program `P` and context `Γ`. -/
inductive Typed (P : Program) : List Ty → Goal → Prop where
  | unit {Γ : List Ty} : Typed P Γ (.expr .unit .unit)
  | int {Γ : List Ty} {n : Int} : fits n = true → Typed P Γ (.expr (.int n) .int)
  | bool {Γ : List Ty} {b : Bool} : Typed P Γ (.expr (.bool b) .bool)
  | local_ {Γ : List Ty} {i : Nat} {t : Ty} :
      lookup Γ i = some t → Typed P Γ (.expr (.local_ i) t)
  | neg {Γ : List Ty} {e : Expr} :
      Typed P Γ (.expr e .int) → Typed P Γ (.expr (.neg e) .int)
  | not {Γ : List Ty} {e : Expr} :
      Typed P Γ (.expr e .bool) → Typed P Γ (.expr (.not e) .bool)
  | binary {Γ : List Ty} {op : BinOp} {l r : Expr} {a t : Ty} :
      BinOpTyped op a t → Typed P Γ (.expr l a) → Typed P Γ (.expr r a) →
      Typed P Γ (.expr (.binary op l r) t)
  | call {Γ : List Ty} {name : String} {f : Function} {args : List Expr} :
      P.find? name = some f → Typed P Γ (.args args (f.params.map Param.type)) →
      Typed P Γ (.expr (.call name args) f.ret)
  | ifElse {Γ : List Ty} {c th el : Expr} {t : Ty} :
      Typed P Γ (.expr c .bool) → Typed P Γ (.expr th t) → Typed P Γ (.expr el t) →
      Typed P Γ (.expr (.if_ c th (some el)) t)
  /-- An `if` with no `else` has type `()` whatever the type of `then`,
  whose value is dropped. The checker (`crates/oska-check/src/body.rs`)
  types it the same way. -/
  | ifNoElse {Γ : List Ty} {c th : Expr} {t : Ty} :
      Typed P Γ (.expr c .bool) → Typed P Γ (.expr th t) →
      Typed P Γ (.expr (.if_ c th none) .unit)
  | block {Γ : List Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.block ss v t) → Typed P Γ (.expr (.block ss v) t)
  /-- A struct or enum variant has the type of its struct or enum. -/
  | construct {Γ : List Ty} {type : String} {variant : Option String} {fs : List Expr}
      {fts : List Ty} :
      P.fieldsOf type variant = some fts → Typed P Γ (.args fs fts) →
      Typed P Γ (.expr (.construct type variant fs) (.named type))
  | field {Γ : List Ty} {b : Expr} {type : String} {fts : List Ty} {i : Nat} {t : Ty} :
      P.fieldsOf type none = some fts → fts[i]? = some t → Typed P Γ (.expr b (.named type)) →
      Typed P Γ (.expr (.field b i) t)
  /-- A `match` whose arms match values of the scrutinee's type and have
  bodies of type `t`, and whose arms pass the static check
  `exhaustiveArms`, or, only once the scrutinee is a value (a state a run
  reaches after skipping arms), whose scrutinee matches an arm with no
  guard. `SourceTyped` rules the second out of a program as written. -/
  | match_ {Γ : List Ty} {s : Expr} {arms : List Arm} {u t : Ty} :
      Typed P Γ (.expr s u) → Typed P Γ (.arms arms u t) →
      (exhaustiveArms P arms = true ∨ (IsValue s ∧ Covered arms s)) →
      Typed P Γ (.expr (.match_ s arms) t)
  | armsNil {Γ : List Ty} {u t : Ty} : Typed P Γ (.arms [] u t)
  | armsCons {Γ : List Ty} {p : Pattern} {body : Expr} {rest : List Arm} {u t : Ty} {bs : List Ty} :
      PatTyped P (.one p u bs) → Typed P (Γ ++ bs) (.expr body t) → Typed P Γ (.arms rest u t) →
      Typed P Γ (.arms (.mk p none body :: rest) u t)
  | armsConsGuard {Γ : List Ty} {p : Pattern} {g body : Expr} {rest : List Arm} {u t : Ty}
      {bs : List Ty} :
      PatTyped P (.one p u bs) → Typed P (Γ ++ bs) (.expr g .bool) →
      Typed P (Γ ++ bs) (.expr body t) → Typed P Γ (.arms rest u t) →
      Typed P Γ (.arms (.mk p (some g) body :: rest) u t)
  | argsNil {Γ : List Ty} : Typed P Γ (.args [] [])
  | argsCons {Γ : List Ty} {e : Expr} {es : List Expr} {t : Ty} {ts : List Ty} :
      Typed P Γ (.expr e t) → Typed P Γ (.args es ts) → Typed P Γ (.args (e :: es) (t :: ts))
  | blockNone {Γ : List Ty} : Typed P Γ (.block [] none .unit)
  | blockSome {Γ : List Ty} {e : Expr} {t : Ty} :
      Typed P Γ (.expr e t) → Typed P Γ (.block [] (some e) t)
  | blockExpr {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P Γ (.block ss v t) →
      Typed P Γ (.block (.expr e :: ss) v t)
  | blockLetBind {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P (Γ ++ [u]) (.block ss v t) →
      Typed P Γ (.block (.let_ true u e :: ss) v t)
  | blockLetDrop {Γ : List Ty} {e : Expr} {u : Ty} {ss : List Stmt} {v : Option Expr} {t : Ty} :
      Typed P Γ (.expr e u) → Typed P Γ (.block ss v t) →
      Typed P Γ (.block (.let_ false u e :: ss) v t)

/-- A program is well typed when each function's body has its result type,
in a context of the slots its parameters bind. -/
def ProgramTyped (P : Program) : Prop :=
  ∀ name f, P.find? name = some f → Typed P (bindingTypes f.params) (.expr f.body f.ret)

mutual
  /-- Whether every `match` in an expression passes `exhaustiveArms`. -/
  def Expr.staticMatches (P : Program) : Expr → Bool
    | .unit | .int _ | .bool _ | .local_ _ => true
    | .neg e | .not e | .field e _ => Expr.staticMatches P e
    | .binary _ l r => Expr.staticMatches P l && Expr.staticMatches P r
    | .call _ args => Expr.staticMatchesList P args
    | .construct _ _ fs => Expr.staticMatchesList P fs
    | .if_ c th el =>
      Expr.staticMatches P c && Expr.staticMatches P th && Expr.staticMatchesOpt P el
    | .block ss val => Expr.staticMatchesStmts P ss && Expr.staticMatchesOpt P val
    | .match_ sc arms =>
      exhaustiveArms P arms && Expr.staticMatches P sc && Expr.staticMatchesArms P arms

  def Expr.staticMatchesList (P : Program) : List Expr → Bool
    | [] => true
    | e :: es => Expr.staticMatches P e && Expr.staticMatchesList P es

  def Expr.staticMatchesOpt (P : Program) : Option Expr → Bool
    | none => true
    | some e => Expr.staticMatches P e

  def Expr.staticMatchesStmts (P : Program) : List Stmt → Bool
    | [] => true
    | .let_ _ _ e :: ss => Expr.staticMatches P e && Expr.staticMatchesStmts P ss
    | .expr e :: ss => Expr.staticMatches P e && Expr.staticMatchesStmts P ss

  def Expr.staticMatchesArms (P : Program) : List Arm → Bool
    | [] => true
    | .mk _ g b :: arms =>
      Expr.staticMatchesOpt P g && Expr.staticMatches P b && Expr.staticMatchesArms P arms
end

/-- **Source typing.** An expression as the checker hands it over: typed,
with every `match` exhaustive by the static check. -/
def SourceTyped (P : Program) (Γ : List Ty) (e : Expr) (t : Ty) : Prop :=
  Typed P Γ (.expr e t) ∧ e.staticMatches P = true

/-- A program as the checker hands it over: each function's body is
source typed. -/
def ProgramSourceTyped (P : Program) : Prop :=
  ∀ name f, P.find? name = some f → SourceTyped P (bindingTypes f.params) f.body f.ret

theorem ProgramSourceTyped.typed {P : Program} (h : ProgramSourceTyped P) : ProgramTyped P :=
  fun name f hf => (h name f hf).1

end OskaFormal.Core
