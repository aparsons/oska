import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics

/-!
# Oska Core: the panics, checked on examples

`Int` arithmetic in Oska Core panics where the interpreter does
(`crates/oska-run/src/ops.rs`): on a result outside `-2^63 ..= 2^63 - 1`,
and on a divisor of 0, which is checked before overflow.
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

/-- A call with the right arity steps into the body. -/
example : Step oneParamProgram (.call "f" [.int 1]) (.next (.int 1)) :=
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
example (P : Program) : ¬ SourceTyped P [] (.match_ (.int 3) [.mk (.int 3) none (.int 1)]) .int := by
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

end OskaFormal.Core
