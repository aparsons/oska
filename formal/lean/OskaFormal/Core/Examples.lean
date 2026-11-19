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

/-- A call with the right arity steps into the body. -/
example : Step [oneParam] (.call "f" [.int 1]) (.next (.int 1)) :=
  Step.call rfl (AllValues.cons (IsValue.int 1) AllValues.nil) rfl

/-- A call of `f` with no argument, or with two, takes no step. -/
example (o : Outcome) : ¬ Step [oneParam] (.call "f" []) o := by
  intro h
  obtain ⟨f, e, hf, hi, _⟩ := call_step AllValues.nil h
  simp [Program.find?, oneParam] at hf
  cases hf
  cases hi

example (o : Outcome) : ¬ Step [oneParam] (.call "f" [.int 1, .int 2]) o := by
  intro h
  have hvs := AllValues.cons (IsValue.int 1) (AllValues.cons (IsValue.int 2) AllValues.nil)
  obtain ⟨f, e, hf, hi, _⟩ := call_step hvs h
  simp [Program.find?, oneParam] at hf
  cases hf
  cases hi

end OskaFormal.Core
