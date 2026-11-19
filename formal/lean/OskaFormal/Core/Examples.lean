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

end OskaFormal.Core
