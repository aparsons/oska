-- SPDX-License-Identifier: Apache-2.0
/-!
# `Float` in the Lean model (ADR 0027, spec #590)

`Float` is IEEE 754 binary64 and the spec fixes every result down to the
bits (ADR 0027). This module is the Lean reference for those results: the
arithmetic, the comparisons, the conversions and the one canonical NaN. It is
written on exact integers, with no hardware float behind it, so it is a
specification that the interpreter and both native backends are checked
against, not a copy of one of them.

A `Float` is its 64 bits, a `Nat` below `2 ^ 64`. `decode` reads the bits as
a `Value`, an operation works on `Value`s, and `encode` writes the answer
back. `encode .nan` is the canonical NaN, `0x7ff8000000000000`, whatever
NaN went in, so no operation can make another pattern.

Rounding is to nearest, ties to even, and nothing else (`roundPos`): a result
is the exact value of the operation, rounded once. `fma` is one such
rounding; `a * b + c` is two, and nothing here fuses them.

`Vectors.lean` holds the golden vectors of `crates/oska-run/tests/float_arith.rs`
and checks this model against each of them.

Not modelled here: the text forms (`to_string`, `parse_float`, `fmt`) beyond
the correctly rounded read of a decimal (`ofDecimal`), and the `std.math`
functions, which are Oska code that the interpreter runs. This module is not
yet part of the core IR (`Syntax.lean`), which refuses a `Float` (see
`crates/oska-check/src/core_ir.rs`), so a program with a `Float` is not yet
run by the Lean reference.
-/

namespace OskaFormal.Float

/-- The canonical NaN: the only NaN any operation makes (ADR 0027). -/
def NAN : Nat := 0x7ff8000000000000

/-- The sign bit. -/
def SIGN : Nat := 2 ^ 63

/-- What a pattern of 64 bits means. A finite number is `m * 2 ^ e` with a
sign, and a zero has `m = 0`, so `+0` and `-0` are apart. -/
inductive Value where
  | nan
  | inf (neg : Bool)
  | fin (neg : Bool) (m : Nat) (e : Int)
  deriving DecidableEq, Repr

/-- Read 64 bits (the bits are taken modulo `2 ^ 64`). -/
def decode (b : Nat) : Value :=
  let b := b % 2 ^ 64
  let neg := decide (2 ^ 63 ≤ b)
  let ex : Nat := (b / 2 ^ 52) % 2048
  let fr := b % 2 ^ 52
  if ex = 2047 then (if fr = 0 then .inf neg else .nan)
  else if ex = 0 then .fin neg fr (-1074)
  else .fin neg (fr + 2 ^ 52) ((ex : Int) - 1075)

/-- Write a value. A finite value must be one that `roundPos` makes: a
mantissa below `2 ^ 52` only at the exponent `-1074`, and in range. -/
def encode : Value → Nat
  | .nan => NAN
  | .inf neg => (if neg then SIGN else 0) + 2047 * 2 ^ 52
  | .fin neg m e =>
    let s := if neg then SIGN else 0
    if m < 2 ^ 52 then s + m
    else
      let m' := if m = 2 ^ 53 then 2 ^ 52 else m
      let e' := if m = 2 ^ 53 then e + 1 else e
      s + (e' + 1075).toNat * 2 ^ 52 + (m' - 2 ^ 52)

/-- The value of a `Float` made from any 64 bits (`Float.from_bits`): a NaN
pattern becomes the canonical NaN on its way in. -/
def fromBits (b : Nat) : Nat := encode (decode b)

/-- The bits of a `Float` (`Float.to_bits`). -/
def toBits (x : Nat) : Nat := x

/-! ## Rounding -/

/-- `2 ^ e <= num / den`. -/
def geTwoPow (num den : Nat) (e : Int) : Bool :=
  if 0 ≤ e then decide (den * 2 ^ e.toNat ≤ num)
  else decide (den ≤ num * 2 ^ (-e).toNat)

/-- `floor (log2 (num / den))` for a positive `num` and `den`. -/
def expOf (num den : Nat) : Int :=
  let e0 : Int := (Nat.log2 num : Int) - (Nat.log2 den : Int)
  if geTwoPow num den e0 then e0 else e0 - 1

/-- `num / den`, a non-negative rational with `den > 0`, rounded to nearest
with ties to even into binary64 and signed `neg`: an underflow keeps the sign
as a signed zero, an overflow is an infinity. This is the only rounding in
the model. -/
def roundPos (neg : Bool) (num den : Nat) : Value :=
  if num = 0 then .fin neg 0 0
  else
    let e := expOf num den
    let u : Int := max (e - 52) (-1074)
    let n' := if 0 ≤ u then num else num * 2 ^ (-u).toNat
    let d' := if 0 ≤ u then den * 2 ^ u.toNat else den
    let q := n' / d'
    let r := n' % d'
    let m :=
      if 2 * r > d' then q + 1
      else if 2 * r = d' then (if q % 2 = 0 then q else q + 1)
      else q
    if m = 0 then .fin neg 0 0
    else if Nat.log2 m + u ≥ 1024 then .inf neg
    else .fin neg m u

/-- `mag * 2 ^ e` rounded. -/
def roundDyadic (neg : Bool) (mag : Nat) (e : Int) : Value :=
  if 0 ≤ e then roundPos neg (mag * 2 ^ e.toNat) 1
  else roundPos neg mag (2 ^ (-e).toNat)

/-! ## Arithmetic -/

/-- A signed zero. -/
def zero (neg : Bool) : Value := .fin neg 0 0

/-- `-x`; the negation of a NaN is the canonical NaN. -/
def negV : Value → Value
  | .nan => .nan
  | .inf neg => .inf (!neg)
  | .fin neg m e => .fin (!neg) m e

/-- The signed integer `±m`, for adding. -/
def signed (neg : Bool) (m : Nat) : Int := if neg then -(m : Int) else (m : Int)

/-- Round the exact sum of two finite values, `±ma * 2^ea + ±mb * 2^eb`. An
exact zero from numbers that are not both zeros is `+0`; the sum of two zeros
is `-0` only when both are. -/
def addFinite (sa : Bool) (ma : Nat) (ea : Int) (sb : Bool) (mb : Nat) (eb : Int) : Value :=
  let e := min ea eb
  let x := signed sa (ma * 2 ^ (ea - e).toNat)
  let y := signed sb (mb * 2 ^ (eb - e).toNat)
  let s := x + y
  if s = 0 then
    (if ma = 0 && mb = 0 then zero (sa && sb) else zero false)
  else roundDyadic (decide (s < 0)) s.natAbs e

def addV : Value → Value → Value
  | .nan, _ => .nan
  | _, .nan => .nan
  | .inf sa, .inf sb => if sa = sb then .inf sa else .nan
  | .inf sa, .fin _ _ _ => .inf sa
  | .fin _ _ _, .inf sb => .inf sb
  | .fin sa ma ea, .fin sb mb eb => addFinite sa ma ea sb mb eb

def subV (a b : Value) : Value := addV a (negV b)

def mulV : Value → Value → Value
  | .nan, _ => .nan
  | _, .nan => .nan
  | .inf sa, .inf sb => .inf (sa != sb)
  | .inf sa, .fin sb m _ => if m = 0 then .nan else .inf (sa != sb)
  | .fin sa m _, .inf sb => if m = 0 then .nan else .inf (sa != sb)
  | .fin sa ma ea, .fin sb mb eb =>
    if ma * mb = 0 then zero (sa != sb) else roundDyadic (sa != sb) (ma * mb) (ea + eb)

def divV : Value → Value → Value
  | .nan, _ => .nan
  | _, .nan => .nan
  | .inf _, .inf _ => .nan
  | .inf sa, .fin sb _ _ => .inf (sa != sb)
  | .fin sa _ _, .inf sb => zero (sa != sb)
  | .fin sa ma ea, .fin sb mb eb =>
    if mb = 0 then (if ma = 0 then .nan else .inf (sa != sb))
    else if ma = 0 then zero (sa != sb)
    else
      let d := ea - eb
      if 0 ≤ d then roundPos (sa != sb) (ma * 2 ^ d.toNat) mb
      else roundPos (sa != sb) ma (mb * 2 ^ (-d).toNat)

/-- The integer square root, by Newton's method from above. The fuel is far
more than the 80 or so iterations that numbers of 300 bits take. -/
def isqrt (n : Nat) : Nat :=
  if n = 0 then 0
  else
    let rec go : Nat → Nat → Nat
      | 0, x => x
      | fuel + 1, x =>
        let y := (x + n / x) / 2
        if y < x then go fuel y else x
    go 400 (2 ^ (Nat.log2 n / 2 + 1))

/-- Extra bits under the root, so the root has at least 70 bits and a
half-unit marks "not exact" without crossing a rounding boundary. -/
def sqrtGuard : Nat := 70

def sqrtV : Value → Value
  | .nan => .nan
  | .inf neg => if neg then .nan else .inf false
  | .fin neg m e =>
    if m = 0 then .fin neg 0 0
    else if neg then .nan
    else
      let odd := e % 2 ≠ 0
      let m' := if odd then 2 * m else m
      let e' := if odd then e - 1 else e
      let big := m' * 4 ^ sqrtGuard
      let s := isqrt big
      let half := e' / 2 - (sqrtGuard : Int)
      if s * s = big then roundDyadic false s half
      else roundDyadic false (2 * s + 1) (half - 1)

/-- `a * b + c` rounded once. -/
def fmaV : Value → Value → Value → Value
  | .nan, _, _ => .nan
  | _, .nan, _ => .nan
  | _, _, .nan => .nan
  | a, b, c =>
    match a, b with
    | .inf sa, .inf sb => addV (.inf (sa != sb)) c
    | .inf sa, .fin sb m _ => if m = 0 then .nan else addV (.inf (sa != sb)) c
    | .fin sa m _, .inf sb => if m = 0 then .nan else addV (.inf (sa != sb)) c
    | .fin sa ma ea, .fin sb mb eb =>
      match c with
      | .inf sc => .inf sc
      | .fin sc mc ec => addFinite (sa != sb) (ma * mb) (ea + eb) sc mc ec
      | .nan => .nan
    | _, _ => .nan

/-! ## Comparisons -/

/-- The order of two values, `none` when either is a NaN (unordered). The
two zeros are equal. -/
def cmpV : Value → Value → Option Ordering
  | .nan, _ => none
  | _, .nan => none
  | .inf sa, .inf sb => some (compare (signed sa 1) (signed sb 1))
  | .inf sa, .fin _ _ _ => some (if sa then .lt else .gt)
  | .fin _ _ _, .inf sb => some (if sb then .gt else .lt)
  | .fin sa ma ea, .fin sb mb eb =>
    let e := min ea eb
    some (compare (signed sa (ma * 2 ^ (ea - e).toNat)) (signed sb (mb * 2 ^ (eb - e).toNat)))

/-- `total_cmp`: every pair is ordered, `-NaN < -inf < ... < -0 < +0 < ...
< +inf < NaN`, and the canonical NaN is last. It is the order of the bit
patterns read as sign and magnitude. -/
def totalKey (b : Nat) : Nat :=
  if 2 ^ 63 ≤ b then 2 ^ 64 - 1 - b else b + 2 ^ 63

def totalCmp (a b : Nat) : Ordering := compare (totalKey a) (totalKey b)

/-! ## `Float` on bits: the operations of the language -/

def add (a b : Nat) : Nat := encode (addV (decode a) (decode b))
def sub (a b : Nat) : Nat := encode (subV (decode a) (decode b))
def mul (a b : Nat) : Nat := encode (mulV (decode a) (decode b))
def div (a b : Nat) : Nat := encode (divV (decode a) (decode b))
def sqrt (a : Nat) : Nat := encode (sqrtV (decode a))
def neg (a : Nat) : Nat := encode (negV (decode a))
def fma (a b c : Nat) : Nat := encode (fmaV (decode a) (decode b) (decode c))

def eq (a b : Nat) : Bool := cmpV (decode a) (decode b) == some .eq
def ne (a b : Nat) : Bool := !eq a b
def lt (a b : Nat) : Bool := cmpV (decode a) (decode b) == some .lt
def le (a b : Nat) : Bool :=
  match cmpV (decode a) (decode b) with
  | some .lt | some .eq => true
  | _ => false
def gt (a b : Nat) : Bool := lt b a
def ge (a b : Nat) : Bool := le b a

/-! ## Conversions -/

/-- A decimal literal or text, `digits * 10 ^ exp10`, correctly rounded
(`parse_float`, a `Float` literal). A value that rounds to infinity is `none`:
`E0222` in a literal, `None` from `parse_float`. -/
def ofDecimal (neg : Bool) (digits : Nat) (exp10 : Int) : Option Nat :=
  let v :=
    if 0 ≤ exp10 then roundPos neg (digits * 10 ^ exp10.toNat) 1
    else roundPos neg digits (10 ^ (-exp10).toNat)
  match v with
  | .inf _ => none
  | v => some (encode v)

/-- The largest `Int` that `to_float()` takes, `2 ^ 53`. -/
def exactLimit : Nat := 2 ^ 53

/-- `Int.to_float()`: exact, or `none` (`P0047`) for a number it would round. -/
def intToFloat (n : Int) : Option Nat :=
  if n.natAbs ≤ exactLimit then some (encode (roundDyadic (decide (n < 0)) n.natAbs 0)) else none

/-- `Int.to_float_rounded()`: nearest, ties to even. -/
def intToFloatRounded (n : Int) : Nat := encode (roundDyadic (decide (n < 0)) n.natAbs 0)

/-- `Float.to_int()`: truncates toward zero, and is `none` (`P0044`) for a
NaN, an infinity or a value `Int` (64 bits) cannot hold. -/
def toInt (x : Nat) : Option Int :=
  match decode x with
  | .fin neg m e =>
    let mag : Nat := if 0 ≤ e then m * 2 ^ e.toNat else m / 2 ^ (-e).toNat
    let n := signed neg mag
    if -(2 ^ 63 : Int) ≤ n ∧ n < 2 ^ 63 then some n else none
  | _ => none

/-! ## Facts -/

theorem encode_nan : encode .nan = NAN := rfl

/-- The canonical NaN reads back as a NaN. -/
theorem decode_nan : decode NAN = .nan := by rfl

/-- A NaN that comes in as bits is the canonical NaN. -/
theorem fromBits_nan (b : Nat) (h : decode b = .nan) : fromBits b = NAN := by
  simp [fromBits, h, encode_nan]

/-- An operation with a NaN operand makes the canonical NaN. -/
theorem add_nan_left (b : Nat) (a : Nat) (h : decode a = .nan) : add a b = NAN := by
  simp [add, h, addV, encode_nan]

theorem add_nan_right (a : Nat) (b : Nat) (h : decode b = .nan) : add a b = NAN := by
  cases ha : decode a <;> simp [add, ha, h, addV, encode_nan]

theorem mul_nan_left (b : Nat) (a : Nat) (h : decode a = .nan) : mul a b = NAN := by
  simp [mul, h, mulV, encode_nan]

theorem div_nan_left (b : Nat) (a : Nat) (h : decode a = .nan) : div a b = NAN := by
  simp [div, h, divV, encode_nan]

theorem sqrt_nan (a : Nat) (h : decode a = .nan) : sqrt a = NAN := by
  simp [sqrt, h, sqrtV, encode_nan]

theorem neg_nan (a : Nat) (h : decode a = .nan) : neg a = NAN := by
  simp [neg, h, negV, encode_nan]

/-- A NaN is not equal to itself, and is neither below nor above anything. -/
theorem nan_not_eq (a : Nat) (h : decode a = .nan) : eq a a = false := by
  simp [eq, h, cmpV]

theorem nan_unordered_lt (a b : Nat) (h : decode a = .nan) : lt a b = false := by
  simp [lt, h, cmpV]

end OskaFormal.Float
