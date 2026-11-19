import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics
import OskaFormal.Core.Eval

/-!
# Oska Core: the panics, checked on examples

`Int` arithmetic in Oska Core panics where the interpreter does
(`crates/oska-run/src/ops.rs`): on a result outside `-2^63 ..= 2^63 - 1`,
and on a divisor of 0, which is checked before overflow.

The closure examples type programs the checker rejects, and show the
typing rejects them too: a closure called with the wrong number of
arguments or an argument of the wrong type, a value called that is not a
function, and a closure that escapes its function at a type it does not
have. So do the `Result` examples at the end: `?` in a function that does
not return a `Result` (`E0243`), and `?` on an error of another type with
no conversion (`E0242`), beside the same `?` through the conversion the
checker chose, which types and runs. So do the var examples: a mutating
call on a `let` (`E0230`) and an assignment to one (`E0229`), each beside
the same on a `var`, which types; and `let b = a; a.bump()` runs to a
changed `a` and an unchanged `b`. `crates/oska-check/tests/core_ir.rs`
checks that the checker rejects the same programs.
-/

namespace OskaFormal.Core

example : evalBinary .add (.int intMax) (.int 1) = some (.panic .overflow) := rfl
example : evalBinary .sub (.int intMin) (.int 1) = some (.panic .overflow) := rfl
example : evalBinary .mul (.int (2 ^ 32)) (.int (2 ^ 31)) = some (.panic .overflow) := rfl
example : evalBinary .add (.int (intMax - 1)) (.int 1) = some (.next (.int intMax)) := rfl
example : evalBinary .div (.int 7) (.int 0) = some (.panic .divisionByZero) := rfl
example : evalBinary .rem (.int 7) (.int 0) = some (.panic .divisionByZero) := rfl
example : evalBinary .div (.int intMin) (.int (-1)) = some (.panic .overflow) := rfl
example : evalBinary .rem (.int intMin) (.int (-1)) = some (.next (.int 0)) := rfl
example : evalBinary .div (.int (-7)) (.int 2) = some (.next (.int (-3))) := rfl
example : evalBinary .rem (.int (-7)) (.int 2) = some (.next (.int (-1))) := rfl
example : evalBinary .rem (.int 7) (.int (-2)) = some (.next (.int 1)) := rfl
example : intResult (-intMin) = .panic .overflow := rfl
example : intResult (-intMax) = .next (.int (-intMax)) := rfl

/-- `-intMin` overflows: `Neg` of the smallest `Int` panics. -/
example (P : Program) : Step P (.neg (.int intMin)) (.panic .overflow) := Step.neg

/-- `fun f(x: Int) -> Int { x }`. -/
def oneParam : Function :=
  { name := "f", params := [{ type := .int, binds := true }], ret := .int, body := .local_ 0 }

/-- A program of `oneParam` alone. -/
def oneParamProgram : Program := { types := [], functions := [oneParam] }

/-- A call with the right arity steps into the body, in a frame. -/
example : Step oneParamProgram (.call "f" [.int 1]) (.next (.frame (.int 1))) :=
  Step.call rfl (AllValues.cons (IsValue.int 1) AllValues.nil) rfl

/-- A call of `f` with no argument, or with two, takes no step. -/
example (o : Outcome) : ¬ Step oneParamProgram (.call "f" []) o := by
  intro h
  obtain ⟨f, e, hf, hi, _⟩ := call_step AllValues.nil h
  simp [oneParamProgram, Program.find?, findFunction, oneParam] at hf
  cases hf
  cases hi

example (o : Outcome) : ¬ Step oneParamProgram (.call "f" [.int 1, .int 2]) o := by
  intro h
  have hvs := AllValues.cons (IsValue.int 1) (AllValues.cons (IsValue.int 2) AllValues.nil)
  obtain ⟨f, e, hf, hi, _⟩ := call_step hvs h
  simp [oneParamProgram, Program.find?, findFunction, oneParam] at hf
  cases hf
  cases hi

/-- `enum Shape { Dot, Square(Int) }` and `struct Pair { a: Int, b: Bool }`. -/
def shapes : Program :=
  { types := [.enum_ "Shape" [("Dot", []), ("Square", [.int])], .struct_ "Pair" [.int, .bool]],
    functions := [] }

/-- A field access steps to the field. -/
example : Step shapes (.field (.construct "Pair" none [.int 1, .bool true]) 1) (.next (.bool true)) :=
  Step.field (.cons (.int 1) (.cons (.bool true) .nil)) rfl

/-- A pattern of another variant does not match, and its arm is skipped. -/
example : matchPat (.construct "Shape" (some "Dot") []) (.construct "Shape" (some "Square") [.int 3]) =
    none := rfl

/-- A pattern binds the fields it names, in order. -/
example : matchPat (.construct "Shape" (some "Square") [.bind .int])
    (.construct "Shape" (some "Square") [.int 3]) = some [.int 3] := rfl

/-- A struct names no variant, and an enum's constructor names one. -/
example : shapes.fieldsOf "Pair" none = some [.int, .bool] := rfl
example : shapes.fieldsOf "Pair" (some "Dot") = none := rfl
example : shapes.fieldsOf "Shape" none = none := rfl
example : shapes.fieldsOf "Shape" (some "Square") = some [.int] := rfl

/-! ## Static exhaustiveness -/

/-- An `Int` is never covered by its literals: `3` alone covers nothing. -/
theorem covers_int_literal (P : Program) : ∀ fuel, covers P fuel [[.int 3]] [.int] = false := by
  intro fuel
  cases fuel with
  | zero => rfl
  | succ fuel =>
    cases fuel <;> simp [covers, defaultRows, Pattern.isCatchAll]

/-- `match 3 { 3 => 1 }` is not source typed in any program: its one arm
does not cover every `Int`, though it covers the scrutinee. -/
example (P : Program) : ¬ SourceTyped P .int [] [] (.match_ (.int 3) [.mk (.int 3) none (.int 1)]) .int := by
  intro ⟨_, h⟩
  simp [Expr.staticMatches, exhaustiveArms, unguarded, Pattern.headType, covers_int_literal] at h

/-- With a catch-all arm after it, it is. -/
example : exhaustiveArms shapes [.mk (.int 3) none (.int 1), .mk .wildcard none (.int 0)] = true := by
  decide

/-- A guarded arm does not count: `Square(n) if n > 0` leaves `Square`
uncovered. -/
example : exhaustiveArms shapes
    [.mk (.construct "Shape" (some "Dot") []) none .unit,
     .mk (.construct "Shape" (some "Square") [.bind .int]) (some (.bool true)) .unit] = false := by
  decide

/-- Every variant of `Shape` covers it, nested patterns and all. -/
example : exhaustiveArms shapes
    [.mk (.construct "Shape" (some "Dot") []) none .unit,
     .mk (.construct "Shape" (some "Square") [.int 0]) none .unit,
     .mk (.construct "Shape" (some "Square") [.bind .int]) none .unit] = true := by
  decide

/-! ## Closures -/

/-- `|x: Int| x + 1`, a plain `fn`, written where `h` slots are in scope. -/
def addOne (h : Nat) : Expr :=
  .closure [{ type := .int, binds := true }] h [] false .int (.binary .add (.local_ h) (.int 1))

/-- `let g = |x: Int| x + 1` in an empty function, then `g(41)`: the
closure has height 0, so it is a value, and the call steps into its body
with `41` for its parameter. -/
example (P : Program) (R : Ty) :
    Typed P R [] [] (.expr (.callValue (addOne 0) [.int 41]) .int) :=
  Typed.callValue (Typed.closure (Nat.le_refl _) (fun _ i _ hi => absurd hi (Nat.not_lt_zero i))
      (fun _ => rfl) (Typed.binary BinOpTyped.add (Typed.local_ rfl) (Typed.int rfl)))
    (Typed.argsCons (Typed.int rfl) Typed.argsNil)

example (P : Program) :
    Step P (.callValue (addOne 0) [.int 41]) (.next (.frame (.binary .add (.int 41) (.int 1)))) :=
  Step.callValue (.cons (.int 41) .nil) rfl

/-- `fn add(n: Int) -> fn(Int) -> Int { |x: Int| x + n }`: a closure
escapes the function that made it, at its own type, a plain `fn`, with
`n` (slot 0, an `Int`) captured; its parameter is slot 1. -/
def addN : Function where
  name := "add"
  params := [{ type := .int, binds := true }]
  ret := .fn false [.int] .int
  body := .closure [{ type := .int, binds := true }] 1 [0] false .int
    (.binary .add (.local_ 1) (.local_ 0))

example (P : Program) : Typed P addN.ret (bindingTypes addN.params) [] (.expr addN.body addN.ret) := by
  refine Typed.closure (by decide) (fun _ i t hi _ hl => ?_) (fun _ => rfl)
    (Typed.binary BinOpTyped.add (Typed.local_ rfl) (Typed.local_ rfl))
  cases i with
  | zero => simp [bindingTypes, addN, lookup] at hl; subst hl; rfl
  | succ i => omega

/-- A call of `add(2)` puts `2` in the closure's body, which is then a
value: what it captured travels with it. -/
example : Step { types := [], functions := [addN] } (.call "add" [.int 2])
    (.next (.frame (.closure [{ type := .int, binds := true }] 0 [] false .int
      (.binary .add (.local_ 0) (.int 2))))) :=
  Step.call rfl (.cons (.int 2) .nil) rfl

/-- `g(1, 2)` with `g = |x: Int| x + 1`: the checker rejects a call of
the wrong arity, and so does the typing. -/
example (P : Program) (R : Ty) (Γ : List Ty) (h : Nat) (t : Ty) :
    ¬ Typed P R Γ [] (.expr (.callValue (addOne h) [.int 1, .int 2]) t) := by
  intro ht
  cases ht with
  | callValue hc hargs =>
    cases hc with
    | closure _ _ _ _ =>
      cases hargs with
      | argsCons _ hr => cases hr

/-- `g(true)`: an argument of the wrong type. -/
example (P : Program) (R : Ty) (Γ : List Ty) (h : Nat) (t : Ty) :
    ¬ Typed P R Γ [] (.expr (.callValue (addOne h) [.bool true]) t) := by
  intro ht
  cases ht with
  | callValue hc hargs =>
    cases hc with
    | closure _ _ _ _ =>
      cases hargs with
      | argsCons hb _ => cases hb

/-- `let n = 1` then `n(2)`: a value that is not a function is not
called. -/
example (P : Program) (R : Ty) (Γ : List Ty) (t : Ty) :
    ¬ Typed P R Γ [] (.expr (.callValue (.int 1) [.int 2]) t) := by
  intro ht
  cases ht with
  | callValue hc _ => cases hc

/-- `fn make() -> fn(Int) -> Bool { |x: Int| x + 1 }`: the closure would
escape at a type it does not have, whatever result type its node names. -/
def makeWrong (ret : Ty) : Function where
  name := "make"
  params := []
  ret := .fn false [.int] .bool
  body := .closure [{ type := .int, binds := true }] 0 [] false ret
    (.binary .add (.local_ 0) (.int 1))

example (P : Program) (ret : Ty) :
    ¬ Typed P (makeWrong ret).ret (bindingTypes (makeWrong ret).params) []
      (.expr (makeWrong ret).body (makeWrong ret).ret) := by
  intro ht
  cases ht with
  | closure _ _ _ hb =>
    cases hb with
    | binary hop _ _ => cases hop

/-- So no program with `make` in it is well typed. -/
example (ret : Ty) : ¬ ProgramTyped { types := [], functions := [makeWrong ret] } := by
  intro hP
  have ht := hP "make" (makeWrong ret) rfl
  cases ht with
  | closure _ _ _ hb =>
    cases hb with
    | binary hop _ _ => cases hop

/-- `fn run(io: Stdout) -> fn() -> Result<(), IoError> { || io.println("hi") }`:
a closure that captures `Stdout` does not escape as a plain `fn`, which
the checker rejects (`E0270`); `Authority.lean`'s
`capturing_closure_not_plain` shows the same where it is written. -/
example (P : Program) (R : Ty) :
    ¬ Typed P R [.cap .stdout] [] (.expr (.closure [] 1 [0] false (.result .unit .ioError)
      (.builtin .println [.local_ 0, .str "hi"])) (.fn false [] (.result .unit .ioError))) := by
  intro h
  cases h with
  | closure _ hpure _ _ =>
    have := hpure rfl 0 (.cap .stdout) (by decide) (by decide) rfl
    simp [Program.pureTy] at this

/-! ## `Result` and `?` -/

/-- A call has the result type of the function it calls. -/
theorem call_typed_ret {P : Program} {R : Ty} {Γ : List Ty} {name : String} {args : List Expr}
    {t : Ty} (h : Typed P R Γ [] (.expr (.call name args) t)) : ∃ f, P.find? name = some f ∧ t = f.ret := by
  cases h with
  | call hf _ => exact ⟨_, hf, rfl⟩

/-- The name the checker gives `From`'s `from` for `AppError`, from `Int`. -/
def appErrorFrom : String := "<AppError as From<Int>>.from"

/-- `fn g() -> Result<Int, Int> { Err(1) }`. -/
def gErr : Function :=
  { name := "g", params := [], ret := .result .int .int, body := .result false (.int 1) }

/-- `struct AppError { code: Int }`, `gErr`, and
`impl From<Int> for AppError { fn from(value: Int) -> AppError { AppError { code: value } } }`,
whose `from` the export names `appErrorFrom`. -/
def appError : Program :=
  { types := [.struct_ "AppError" [.int]],
    functions := [gErr,
      { name := appErrorFrom, params := [{ type := .int, binds := true }], ret := .named "AppError",
        body := .construct "AppError" none [.local_ 0] }] }

/-- `fn f() -> Int { g()? }`: `?` where the function does not return a
`Result` (`E0243`). No `?` types in a function or closure whose result
type is not a `Result`, whatever it is on and however it converts. -/
example (P : Program) (R : Ty) (hR : ∀ t u, R ≠ .result t u) (Γ : List Ty) (e : Expr)
    (c : Option String) (t : Ty) : ¬ Typed P R Γ [] (.expr (.try_ e c) t) := by
  intro h
  cases h
  exact hR _ _ rfl

/-- `fn f() -> Result<Int, AppError> { let x = g()?; Ok(x) }` with no
`From` conversion: `g`'s error, an `Int`, is not an `AppError`
(`E0242`). -/
example (Γ : List Ty) (t : Ty) :
    ¬ Typed appError (.result .int (.named "AppError")) Γ [] (.expr (.try_ (.call "g" []) none) t) := by
  intro h
  cases h with
  | try_ _ he hc =>
    cases hc
    obtain ⟨f, hf, ht⟩ := call_typed_ret he
    simp [appError, Program.find?, findFunction, gErr] at hf
    subst hf
    simp at ht

/-- Nor does a conversion of another type stand in for it: `g()?` through
a function that gives an `Int`, not an `AppError`. -/
example (Γ : List Ty) (t : Ty) :
    ¬ Typed appError (.result .int (.named "AppError")) Γ [] (.expr (.try_ (.call "g" []) (some "g")) t) := by
  intro h
  cases h with
  | try_ _ _ hc =>
    cases hc with
    | some hf hps _ =>
      simp [appError, Program.find?, findFunction, gErr] at hf
      subst hf
      simp [Function.argTypes] at hps

/-- With the conversion the checker chose, the same `?` types... -/
example (Γ : List Ty) :
    Typed appError (.result .int (.named "AppError")) Γ []
      (.expr (.try_ (.call "g" []) (some appErrorFrom)) .int) :=
  Typed.try_ rfl (Typed.call (f := gErr) rfl Typed.argsNil) (ConvTyped.some rfl rfl rfl)

/-- `fn f() -> Result<Int, AppError> { let x = g()?; Ok(x + 1) }`, with
the conversion. -/
def fTry : Function :=
  { name := "f", params := [], ret := .result .int (.named "AppError"),
    body := .block [.let_ true .int (.try_ (.call "g" []) (some appErrorFrom))]
      (some (.result true (.binary .add (.local_ 0) (.int 1)))) }

/-- ...and runs: `f` returns `g`'s error through it, without reaching
`Ok(x + 1)`. -/
example :
    (match (run { appError with functions := appError.functions ++ [fTry] } 50 {}
        (.call "f" [])).1 with
      | .value (.result false (.construct "AppError" none [.int 1])) => true
      | _ => false) = true := by
  decide

/-! ## Vars and mutating methods -/

/-- `impl Counter { fn bump(var self) { self.n = self.n + 1 } }`. -/
def bump : Function :=
  { name := "Counter.bump", receiver := some (.named "Counter"), params := [], ret := .unit,
    body := .block [.assign ⟨0, [0]⟩ (.binary .add (.field (.var_ 0) 0) (.int 1))] none }

/-- `struct Counter { n: Int }`, with `bump`. -/
def counter : Program :=
  { types := [.struct_ "Counter" [.int]], functions := [bump] }

/-- `var a = Counter(n: 0); let b = a; a.bump(); read`: `a` is var 0 and
`b` slot 0. -/
def aliasing (read : Expr) : Expr :=
  .block [.var_ (.named "Counter") (.construct "Counter" none [.int 0]),
    .let_ true (.named "Counter") (.var_ 0),
    .expr (.mutCall "Counter.bump" ⟨0, []⟩ [])] (some read)

/-- `a.bump()` changes `a`... -/
example : (match (run counter 200 {} (aliasing (.field (.var_ 0) 0))).1 with
    | .value (.int 1) => true
    | _ => false) = true := by
  decide

/-- ...and not `b`, the copy `let b = a` took before the call. -/
example : (match (run counter 200 {} (aliasing (.field (.local_ 0) 0))).1 with
    | .value (.int 0) => true
    | _ => false) = true := by
  decide

/-- A mutating call types on a `var`... -/
example (R : Ty) :
    Typed counter R [] [] (.block [.var_ (.named "Counter") (.construct "Counter" none [.int 0]),
      .expr (.mutCall "Counter.bump" ⟨0, []⟩ [])] none .unit) :=
  .blockVar (.construct rfl (.argsCons (.int (by decide)) .argsNil))
    (.blockExpr (.mutCall (f := bump) rfl rfl rfl .argsNil) .blockNone)

/-- ...but not on a `let`, which is a slot and no var: `let a =
Counter(n: 0); a.bump()`, which the checker rejects (`E0230`). -/
example (P : Program) (R t : Ty) :
    ¬ Typed P R [] [] (.block [.let_ true (.named "Counter") (.construct "Counter" none [.int 0]),
      .expr (.mutCall "Counter.bump" ⟨0, []⟩ [])] none t) := by
  intro h
  cases h with
  | blockLetBind _ h =>
    cases h with
    | blockExpr h _ =>
      cases h with
      | mutCall _ _ hpl _ => simp [Program.placeTy, lookup] at hpl

/-- An assignment types on a `var`... -/
example (R : Ty) :
    Typed counter R [] [] (.block [.var_ (.named "Counter") (.construct "Counter" none [.int 0]),
      .assign ⟨0, [0]⟩ (.int 1)] none .unit) :=
  .blockVar (.construct rfl (.argsCons (.int (by decide)) .argsNil))
    (.blockAssign rfl (.int (by decide)) .blockNone)

/-- ...but not on a `let`: `let a = Counter(n: 0); a.n = 1`, which the
checker rejects (`E0229`). -/
example (P : Program) (R t : Ty) :
    ¬ Typed P R [] [] (.block [.let_ true (.named "Counter") (.construct "Counter" none [.int 0]),
      .assign ⟨0, [0]⟩ (.int 1)] none t) := by
  intro h
  cases h with
  | blockLetBind _ h =>
    cases h with
    | blockAssign hpl _ _ => simp [Program.placeTy, lookup] at hpl

end OskaFormal.Core
