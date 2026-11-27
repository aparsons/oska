-- SPDX-License-Identifier: Apache-2.0
import OskaFormal.Float.Model

/-!
# Checks of the `Float` model: conversions, literals, signed zero, NaN

The same cases as `crates/oska-run/tests/float_text.rs` (`to_int`,
`to_float`, `to_float_rounded`, `total_cmp`) and the literal cases of
`crates/oska-run/tests/float_arith.rs`, each as a `#guard` that fails the
build. The arithmetic vectors are in `Vectors.lean`.
-/

namespace OskaFormal.Float.Checks

open OskaFormal.Float

-- The canonical NaN, and every NaN pattern becomes it on the way in.
#guard fromBits 0xfff8000000000001 == NAN
#guard fromBits 0x7ff0000000000001 == NAN
#guard fromBits 0x7ff8000000000000 == NAN
#guard add (fromBits 0x3ff0000000000000) NAN == NAN
#guard mul (fromBits 0) (fromBits 0x7ff0000000000000) == NAN
#guard sub (fromBits 0x7ff0000000000000) (fromBits 0x7ff0000000000000) == NAN
#guard div 0 0 == NAN
#guard sqrt (fromBits 0xbff0000000000000) == NAN
#guard neg NAN == NAN
#guard fma (fromBits 0x7ff0000000000000) 0 (fromBits 0x3ff0000000000000) == NAN

-- A NaN is not equal to itself; the two zeros are equal.
#guard eq NAN NAN == false
#guard ne NAN NAN == true
#guard lt NAN NAN == false && ge NAN NAN == false
#guard eq 0 SIGN == true

-- Rounding: ties go to even, and nothing is fused.
#guard add 0x3ff0000000000000 0x3ca0000000000000 == 0x3ff0000000000000
#guard add 0x3ff0000000000001 0x3ca0000000000000 == 0x3ff0000000000002
-- `0.1 * 10.0 + -1.0` is 0 with two roundings and 2^-54 with `fma`.
#guard add (mul 0x3fb999999999999a 0x4024000000000000) 0xbff0000000000000 == 0
#guard fma 0x3fb999999999999a 0x4024000000000000 0xbff0000000000000 == 0x3c90000000000000
-- Overflow is infinity, underflow keeps the sign.
#guard mul 0x7fefffffffffffff 0x4000000000000000 == 0x7ff0000000000000
#guard mul 0x0000000000000001 0xbfe0000000000000 == SIGN
-- Exact zero sums: +0 unless both are -0.
#guard add 0x3ff0000000000000 0xbff0000000000000 == 0
#guard add SIGN SIGN == SIGN
#guard add SIGN 0 == 0

-- Literals: the nearest value, and a literal beyond `Float` is `none` (E0222).
#guard ofDecimal false 15 (-1) == some 0x3ff8000000000000
#guard ofDecimal false 1 (-1) == some 0x3fb999999999999a
#guard ofDecimal false 2 (-3) == some 0x3f60624dd2f1a9fc
#guard ofDecimal false 100025 (-2) == some 0x408f420000000000
#guard ofDecimal false 1 308 == some 0x7fe1ccf385ebc8a0
#guard ofDecimal false 5 (-324) == some 1
#guard ofDecimal false 25 (-325) == some 1
#guard ofDecimal false 24 (-325) == some 0
#guard ofDecimal true 0 0 == some SIGN
#guard ofDecimal false 17976931348623157 292 == some 0x7fefffffffffffff
#guard ofDecimal false 17976931348623159 292 == none
#guard ofDecimal false 1 999 == none

-- `to_int()`: toward zero; none (P0046) for NaN, infinity, out of range.
#guard toInt 0x3feccccccccccccd == some 0
#guard toInt 0xbfeccccccccccccd == some 0
#guard toInt 0x3ff8000000000000 == some 1
#guard toInt 0xbff8000000000000 == some (-1)
#guard toInt 0x4004000000000000 == some 2
#guard toInt 0x43abc16d674ec800 == some 1000000000000000000
#guard toInt 0x43dfffffffffffff == some 9223372036854774784
#guard toInt 0xc3e0000000000000 == some (-9223372036854775808)
#guard toInt 0x43e0000000000000 == none
#guard toInt 0x4415af1d78b58c40 == none
#guard toInt 0xc3e0000000000001 == none
#guard toInt NAN == none
#guard toInt 0x7ff0000000000000 == none
#guard toInt 0xfff0000000000000 == none

-- `to_float()`: exact up to 2^53, none (P0049) beyond.
#guard intToFloat 0 == some 0
#guard intToFloat (-1) == some 0xbff0000000000000
#guard intToFloat 9007199254740992 == some 0x4340000000000000
#guard intToFloat (-9007199254740992) == some 0xc340000000000000
#guard intToFloat 9007199254740993 == none
#guard intToFloat (-9007199254740993) == none
#guard intToFloat 9223372036854775807 == none
#guard intToFloat (-9223372036854775808) == none

-- `to_float_rounded()`: nearest, ties to even.
#guard intToFloatRounded 9007199254740993 == 0x4340000000000000
#guard intToFloatRounded 9007199254740995 == 0x4340000000000002
#guard intToFloatRounded (-9007199254740993) == 0xc340000000000000
#guard intToFloatRounded 9223372036854775807 == 0x43e0000000000000
#guard intToFloatRounded (-9223372036854775808) == 0xc3e0000000000000
#guard intToFloatRounded 4611686018427387905 == 0x43d0000000000000
#guard intToFloatRounded 4611686018427388416 == 0x43d0000000000000
#guard intToFloatRounded 4611686018427388417 == 0x43d0000000000001
#guard intToFloatRounded 4611686018427389440 == 0x43d0000000000002

-- `total_cmp`: -NaN, -inf, -0, +0, +inf, NaN in this order.
#guard totalCmp SIGN 0 == .lt
#guard totalCmp 0 SIGN == .gt
#guard totalCmp 0x7ff0000000000000 NAN == .lt
#guard totalCmp 0xfff8000000000000 0xfff0000000000000 == .lt
#guard totalCmp NAN NAN == .eq

end OskaFormal.Float.Checks
