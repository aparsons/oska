# SPDX-License-Identifier: Apache-2.0
"""Writes the hard-case vectors for `std_math_exact.rs` (spec #590, #606), and
the constants of the exact tier.

    python3 generate_exact.py            # writes <name>_hard.txt for each function
    python3 generate_exact.py --constants > constants.osk

Needs `mpmath`; nothing here uses the host's libm. A row has the same columns
as the vectors of `generate.py`: the inputs, the correctly rounded result as
bits `hi` and the rest `lo` (both from a 700-bit value), a kind, and the pinned
bits (`-`: the hard files pin nothing, the exact tier is checked against `hi`
with no tolerance).

A hard case is an input whose result lies within 2^-18 ulp of a point half
way between two doubles, close enough that the fast path of the exact tier
(an error margin of 2^-70 of the result, about 2^-17 ulp) cannot decide the
rounding and the slow path runs. The cases are found by searching a fixed
random sample of each function's domain (the seed is below, so the files are
reproducible): the distance to the nearest midpoint is uniform on [0, 1/2]
ulp, so about 1 input in 130000 qualifies. The published worst cases of
Lefevre and Muller (the ones with distance below 2^-50 ulp) could not be
fetched in the build environment; the cases below are far from the worst
case, but they exercise the same code. Three more kinds of case are
constructed, not searched, because they are exact or nearly so:

- `hypot`: Pythagorean triples whose hypotenuse has 54 bits, so the result is
  exactly half way between two doubles (a tie), and `hypot(b*b + 1, b)`,
  which lies within 2^-53 ulp of a tie;
- `powf`: an integer power with 54 significant bits (a tie), `z^2` to the
  power 1.5 and 2.5 (a tie, `z^3` and `z^5`), an odd power of a negative
  base, and a tie in the subnormal range, `(3 * 2^-215)^5`;
- every function: the rows must also hold for a result in the exact
  functions' special cases, which `generate.py` writes.
"""
import math
import random
import struct
import sys
from fractions import Fraction

import mpmath

mpmath.mp.prec = 700

NAN = 0x7FF8000000000000
INF = 0x7FF0000000000000
SIGN = 1 << 63


def bits(x):
    return struct.unpack(">Q", struct.pack(">d", x))[0]


def flt(b):
    return struct.unpack(">d", struct.pack(">Q", b))[0]


def to_fraction(v):
    if isinstance(v, mpmath.mpf):
        if v == 0:
            return Fraction(0)
        sign, man, exp, bc = v._mpf_
        return Fraction(-1 if sign else 1) * Fraction(int(man)) * (Fraction(2) ** exp)
    return Fraction(v)


def pair(v):
    """`hi`, `lo` bits of the exact value `v`, with the sign of a zero."""
    frac = to_fraction(v)
    hi = bits(float(frac))
    if frac < 0 and hi == 0:
        hi = SIGN
    lo = bits(float(frac - Fraction(flt(hi))))
    return hi, lo


def row(inputs, value):
    hi, lo = pair(value)
    ins = " ".join(f"{i:016x}" for i in inputs)
    return f"{ins} {hi:016x} {lo:016x} u -"


def distance_to_midpoint(v):
    """Distance of `v` (an mpf) from the nearest midpoint of two doubles, in
    ulps of `v` (0 to 1/2); `None` outside the normal range."""
    a = abs(v)
    if a < mpmath.mpf(2) ** -1000 or a > mpmath.mpf(2) ** 1020:
        return None
    e = int(mpmath.floor(mpmath.log(a, 2)))
    # Correct a binade the logarithm put wrongly at its edge.
    while mpmath.mpf(2) ** e > a:
        e -= 1
    while mpmath.mpf(2) ** (e + 1) <= a:
        e += 1
    t = a / mpmath.mpf(2) ** (e - 52)
    return abs(float(mpmath.frac(t)) - 0.5)


THRESHOLD = 2.0**-18


def search(name, sample, evaluate, wanted, tries, seed):
    rng = random.Random(seed)
    found = []
    for _ in range(tries):
        inputs = sample(rng)
        try:
            v = evaluate(*[mpmath.mpf(flt(i)) for i in inputs])
        except Exception:
            continue
        d = distance_to_midpoint(v)
        if d is not None and d < THRESHOLD:
            found.append((d, inputs, v))
            if len(found) >= wanted:
                break
    found.sort(key=lambda f: f[0])
    return [row(i, v) for _, i, v in found]


def log_uniform(rng, lo_exp, hi_exp):
    return 2.0 ** rng.uniform(lo_exp, hi_exp) * rng.uniform(1, 2) / 1.5


def signed(rng, v):
    return v if rng.random() < 0.5 else -v


def powf_value(x, y):
    if x < 0:
        r = mpmath.power(-x, y)
        return -r  # y is an odd integer
    return mpmath.power(x, y)


def write(name, header, rows):
    with open(f"{name}_hard.txt", "w") as f:
        f.write(f"# SPDX-License-Identifier: Apache-2.0\n# {header}\n")
        f.write("# columns: inputs, reference hi, reference lo, kind (u), pinned (-: none)\n")
        for r in rows:
            f.write(r + "\n")


def hard_exp():
    rows = search("exp", lambda r: (bits(r.uniform(-745.0, 709.7)),), mpmath.exp, 14, 6_000_000, 6061)
    # exp(2^-53) = 1 + 2^-53 + 2^-107: just above a tie
    rows.append(row((bits(2.0**-53),), mpmath.exp(mpmath.mpf(2.0**-53))))
    rows.append(row((bits(-(2.0**-54)),), mpmath.exp(mpmath.mpf(-(2.0**-54)))))
    write("exp", "exp(x): inputs whose result is within 2^-18 ulp of a tie, and two near-ties of tiny x", rows)


def hard_ln():
    rows = search("ln", lambda r: (bits(log_uniform(r, -1074, 1023)),), mpmath.log, 14, 6_000_000, 6062)
    rows += search("ln", lambda r: (bits(r.uniform(0.5, 2.0)),), mpmath.log, 6, 6_000_000, 6063)
    write("ln", "ln(x): inputs whose result is within 2^-18 ulp of a tie", rows)


def hard_sin():
    rows = search("sin", lambda r: (bits(signed(r, r.uniform(0.0, 1647098.0))),), mpmath.sin, 12, 6_000_000, 6064)
    rows += search("sin", lambda r: (bits(signed(r, log_uniform(r, -27, 3))),), mpmath.sin, 6, 6_000_000, 6065)
    write("sin", "sin(x): inputs whose result is within 2^-18 ulp of a tie", rows)


def hard_cos():
    rows = search("cos", lambda r: (bits(signed(r, r.uniform(0.0, 1647098.0))),), mpmath.cos, 12, 6_000_000, 6066)
    rows += search("cos", lambda r: (bits(signed(r, log_uniform(r, -27, 3))),), mpmath.cos, 6, 6_000_000, 6067)
    write("cos", "cos(x): inputs whose result is within 2^-18 ulp of a tie", rows)


def hard_atan2():
    def sample(r):
        return (bits(signed(r, log_uniform(r, -40, 40))), bits(signed(r, log_uniform(r, -40, 40))))

    def near(r):
        return (bits(signed(r, r.uniform(0.01, 5))), bits(signed(r, r.uniform(0.01, 5))))

    rows = search("atan2", sample, mpmath.atan2, 10, 6_000_000, 6068)
    rows += search("atan2", near, mpmath.atan2, 10, 6_000_000, 6069)
    write("atan2", "atan2(y, x): inputs whose result is within 2^-18 ulp of a tie", rows)


def hard_hypot():
    rows = []
    # Pythagorean ties: legs a, b below 2^53, hypotenuse c odd with 54 bits.
    seen = 0
    for m in range(2, 60):
        for n in range(1, m):
            if (m - n) % 2 == 0 or math.gcd(m, n) != 1:
                continue
            a, b, c = m * m - n * n, 2 * m * n, m * m + n * n
            legs = max(a, b)
            k = (2**53) // c + 1
            if k % 2 == 0:
                k += 1
            if c * k >= 2**54 or legs * k >= 2**53 or c * k <= 2**53:
                continue
            rows.append(row((bits(float(a * k)), bits(float(b * k))), mpmath.sqrt(mpmath.mpf(a * k) ** 2 + mpmath.mpf(b * k) ** 2)))
            seen += 1
            if seen >= 8:
                break
        if seen >= 8:
            break
    # Within 2^-53 ulp of a tie: hypot(b*b + 1, b), with the result above a + 1/2.
    for b in [67108865, 77777777, 94906265, 80000001, 85000001, 90000001]:
        x, y = float(b * b + 1), float(b)
        rows.append(row((bits(x), bits(y)), mpmath.sqrt(mpmath.mpf(x) ** 2 + mpmath.mpf(y) ** 2)))
    rows += search("hypot", lambda r: (bits(log_uniform(r, -30, 30)), bits(log_uniform(r, -30, 30))),
                   lambda a, b: mpmath.sqrt(a * a + b * b), 8, 6_000_000, 6070)
    write("hypot", "hypot(x, y): Pythagorean ties, near-ties and searched cases", rows)


def hard_powf():
    rows = []
    # Integer powers with 54 significant bits (ties); the sign of an odd power is kept.
    ties = []
    for m in range(3, 2000, 2):
        for y in range(2, 65):
            if 2**53 < m**y < 2**54:
                ties.append((m, y))
    for m, y in ties[:6]:
        rows.append(row((bits(float(m)), bits(float(y))), mpmath.mpf(m) ** y))
    for m, y in ties:
        if y % 2 == 1:
            rows.append(row((bits(-float(m)), bits(float(y))), -(mpmath.mpf(m) ** y)))
            break
    # z^2 to the power 1.5 and 2.5: z^3 and z^5, a tie when it has 54 bits.
    for z, y in [(208065, 1.5), (250001, 1.5), (1553, 2.5), (1801, 2.5), (193, 3.5)]:
        if 2**53 < z ** int(2 * y) < 2**54 and z * z < 2**53:
            rows.append(row((bits(float(z * z)), bits(y)), mpmath.mpf(z) ** int(2 * y)))
    # A tie in the subnormal range: (3 * 2^-215)^5 = 243 * 2^-1075.
    x = 3.0 * 2.0**-215
    rows.append(row((bits(x), bits(5.0)), mpmath.mpf(3) ** 5 * mpmath.mpf(2) ** -1075))
    rows += search("powf", lambda r: (bits(log_uniform(r, -5, 8)), bits(r.uniform(-30, 30))), mpmath.power, 10, 6_000_000, 6071)
    rows += search("powf", lambda r: (bits(r.uniform(0.5, 1.5)), bits(signed(r, log_uniform(r, -5, 30)))), mpmath.power, 6, 6_000_000, 6072)
    write("powf", "powf(x, y): integer ties, a subnormal tie and searched cases", rows)


def constants():
    """The Oska source of the constants of the exact tier."""
    frac_bits, limbs = 256, 12

    def big_limbs(v):
        n = int(mpmath.floor(v * mpmath.mpf(2) ** frac_bits))
        return [(n >> (32 * i)) & 0xFFFFFFFF for i in range(limbs)]

    def lst(items):
        return "[" + ", ".join(str(i) for i in items) + "]"

    def dd(v):
        hi = float(v)
        return hi, float(v - mpmath.mpf(hi))

    out = ["// Written by `crates/oska-run/tests/float_reference/generate_exact.py --constants`.", ""]
    out.append("// Fixed-point constants: the value times 2^256, as twelve 32-bit limbs, least")
    out.append("// significant first.")
    for name, v in [("ln2", mpmath.log(2)), ("pio2", mpmath.pi / 2), ("pi", mpmath.pi)]:
        out.append(f"fn big_{name}() -> List<U64> {{\n    {lst(big_limbs(v))}\n}}\n")
    out.append("// `atan(j / 8)` for j from 1 to 8.")
    out.append("fn big_atan_eighths(j: Int) -> List<U64> {")
    for j in range(1, 9):
        out.append(f"    {'if' if j == 1 else '} else if'} j == {j} {{\n        {lst(big_limbs(mpmath.atan(mpmath.mpf(j) / 8)))}")
    out.append("    } else {\n        big_zero()\n    }\n}\n")
    for name, v in [("DD_LN2", mpmath.log(2)), ("DD_PI", mpmath.pi), ("DD_PIO2", mpmath.pi / 2)]:
        hi, lo = dd(v)
        out.append(f"const {name}_HI: Float = {hi!r}\nconst {name}_LO: Float = {lo!r}")
    out.append(f"const DD_INV_LN2: Float = {float(1 / mpmath.log(2))!r}")
    out.append(f"const DD_TWO_OVER_PI: Float = {float(2 / mpmath.pi)!r}")
    x = mpmath.pi / 2
    for i in range(1, 5):
        p = float(x)
        out.append(f"const PIO2_{i}: Float = {p!r}")
        x = x - mpmath.mpf(p)
    out.append("")

    def coefficients(name, head, tail):
        his, los = zip(*[dd(v) for v in head])
        s = f"fn {name}_head_hi() -> List<Float> {{\n    [{', '.join(repr(h) for h in his)}]\n}}\n\n"
        s += f"fn {name}_head_lo() -> List<Float> {{\n    [{', '.join(repr(l) for l in los)}]\n}}\n\n"
        s += f"fn {name}_tail() -> List<Float> {{\n    [{', '.join(repr(float(v)) for v in tail)}]\n}}\n"
        return s

    fact = mpmath.factorial
    one = mpmath.mpf(1)
    out.append(coefficients("exp", [one / fact(n) for n in range(0, 8)], [one / fact(n) for n in range(8, 17)]))
    out.append(coefficients("ln", [one / (2 * j + 1) for j in range(0, 6)], [one / (2 * j + 1) for j in range(6, 17)]))
    out.append(coefficients("sin", [(-1) ** j / fact(2 * j + 1) for j in range(0, 6)], [(-1) ** j / fact(2 * j + 1) for j in range(6, 12)]))
    out.append(coefficients("cos", [(-1) ** j / fact(2 * j) for j in range(0, 6)], [(-1) ** j / fact(2 * j) for j in range(6, 12)]))
    out.append(coefficients("atan", [(-1) ** j * one / (2 * j + 1) for j in range(0, 3)], [(-1) ** j * one / (2 * j + 1) for j in range(3, 10)]))
    his = [0.0]
    los = [0.0]
    for j in range(1, 9):
        h, l = dd(mpmath.atan(mpmath.mpf(j) / 8))
        his.append(h)
        los.append(l)
    out.append(f"fn atan_eighths_hi() -> List<Float> {{\n    [{', '.join(repr(h) for h in his)}]\n}}\n")
    out.append(f"fn atan_eighths_lo() -> List<Float> {{\n    [{', '.join(repr(l) for l in los)}]\n}}")
    return "\n".join(out)


if __name__ == "__main__":
    if "--constants" in sys.argv:
        print(constants())
    else:
        from multiprocessing import Pool

        with Pool(4) as pool:
            jobs = [pool.apply_async(f) for f in (hard_exp, hard_ln, hard_sin, hard_cos, hard_atan2, hard_hypot, hard_powf)]
            for j in jobs:
                j.get()
