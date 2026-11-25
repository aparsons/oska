# SPDX-License-Identifier: Apache-2.0
"""Writes the reference vectors for `std_math_float.rs` (spec #590, #596).

Run once: `python3 generate.py` (needs `mpmath`; nothing here uses the host's
libm). Each input is evaluated with mpmath at 400 bits, so the reference is
the exact value to far more than 106 bits, then stored as a pair of doubles
`hi + lo` (`hi` is the correctly rounded double, `lo` the rest, rounded).
The special values (NaN, infinities, zeros, domain edges) are written out by
hand from C99 Annex F and IEEE 754, and marked `x` (the bits must match
exactly); a finite result is marked `u` (within the stored ulp bound). The
last column, the bits the Oska function gave when the vector was written, is
filled in by the test with `OSKA_BLESS_FLOAT=1`.
"""
import math
import random
import struct
from fractions import Fraction

import mpmath

mpmath.mp.prec = 400

NAN = 0x7FF8000000000000
INF = 0x7FF0000000000000
SIGN = 1 << 63


def bits(x):
    return struct.unpack(">Q", struct.pack(">d", x))[0]


def flt(b):
    return struct.unpack(">d", struct.pack(">Q", b))[0]


def is_nan(b):
    return (b & ~SIGN) > INF


def is_inf(b):
    return (b & ~SIGN) == INF


def is_zero(b):
    return (b & ~SIGN) == 0


def neg(b):
    return b ^ SIGN


def to_fraction(v):
    if isinstance(v, mpmath.mpf):
        if v == 0:
            return Fraction(0)
        sign, man, exp, bc = v._mpf_
        if exp + bc > 1100:
            # Far beyond the largest double: any such value rounds to infinity.
            return Fraction(2) ** 1100 * (-1 if sign else 1)
        if exp + bc < -1200:
            # Far below half the smallest subnormal: it rounds to zero.
            return Fraction(0)
        return Fraction(-1 if sign else 1) * Fraction(int(man)) * (Fraction(2) ** exp)
    return Fraction(v)


def rounded(frac):
    try:
        return bits(float(frac))
    except OverflowError:
        return INF if frac > 0 else INF | SIGN


def pair(v):
    """`hi`, `lo` bits of the exact value `v`."""
    frac = to_fraction(v)
    hi = rounded(frac)
    if is_inf(hi):
        return hi, 0
    lo = bits(float(frac - Fraction(flt(hi))))
    return hi, lo


def nextafter(x, up):
    b = bits(x)
    if x == 0:
        return flt(1) if up else -flt(1)
    if (x > 0) == up:
        return flt(b + 1)
    return flt(b - 1)


# -- inputs ------------------------------------------------------------

rng = random.Random(596)


def log_uniform(lo_exp, hi_exp):
    e = rng.uniform(lo_exp, hi_exp)
    return 2.0 ** e * rng.uniform(1, 2) / 1.5


def signed(v):
    return v if rng.random() < 0.5 else -v


MIN_NORMAL = flt(0x0010000000000000)
MAX = flt(0x7FEFFFFFFFFFFFFF)
TINY = flt(1)


def unary_inputs():
    pass


# -- special values (C99 Annex F) --------------------------------------


def special_exp(a):
    if is_nan(a):
        return NAN
    if a == INF:
        return INF
    if a == INF | SIGN:
        return 0
    if is_zero(a):
        return bits(1.0)
    return None


def special_ln(a):
    if is_nan(a):
        return NAN
    if a & SIGN and not is_zero(a):
        return NAN
    if is_zero(a):
        return INF | SIGN
    if a == INF:
        return INF
    if a == bits(1.0):
        return 0
    return None


def special_sin(a):
    if is_nan(a) or is_inf(a):
        return NAN
    if is_zero(a):
        return a
    return None


def special_cos(a):
    if is_nan(a) or is_inf(a):
        return NAN
    if is_zero(a):
        return bits(1.0)
    return None


PI = bits(math.pi)
PI_2 = bits(math.pi / 2)


def special_atan2(y, x):
    if is_nan(x) or is_nan(y):
        return NAN
    ys = y & SIGN
    xs = x & SIGN
    if is_zero(y):
        if is_zero(x) or xs:
            return (PI | ys) if xs else ys
        return ys
    if is_zero(x):
        return PI_2 | ys
    if is_inf(x):
        if is_inf(y):
            q = bits(math.pi / 4) if not xs else bits(3 * math.pi / 4)
            return q | ys
        return (PI | ys) if xs else ys
    if is_inf(y):
        return PI_2 | ys
    return None


def special_hypot(a, b):
    if is_inf(a) or is_inf(b):
        return INF
    if is_nan(a) or is_nan(b):
        return NAN
    if is_zero(a):
        return b & ~SIGN
    if is_zero(b):
        return a & ~SIGN
    return None


def parity(b):
    x = abs(flt(b))
    if x >= 2.0**53:
        return 2
    if x != math.floor(x):
        return 0
    return 1 if int(x) % 2 else 2


def special_powf(x, y):
    if is_zero(y) or x == bits(1.0):
        return bits(1.0)
    if is_nan(x) or is_nan(y):
        return NAN
    par = parity(y)
    xs = x & SIGN
    ax = x & ~SIGN
    neg_odd = bool(xs) and par == 1
    ysign = y & SIGN
    if is_inf(y):
        if ax == bits(1.0):
            return bits(1.0)
        grows = (ax > bits(1.0)) == (not ysign)
        return INF if grows else 0
    if is_zero(x):
        r = INF if ysign else 0
        return r | (SIGN if neg_odd else 0)
    if is_inf(x):
        r = 0 if ysign else INF
        return r | (SIGN if neg_odd else 0)
    if xs and par == 0:
        return NAN
    if ax == bits(1.0):
        return bits(-1.0) if neg_odd else bits(1.0)
    return None


# -- evaluation ---------------------------------------------------------


def exact_unary(name, a):
    x = mpmath.mpf(flt(a))
    return {
        "exp": mpmath.exp,
        "ln": mpmath.log,
        "sin": mpmath.sin,
        "cos": mpmath.cos,
    }[name](x)


def exact_binary(name, a, b):
    x, y = mpmath.mpf(flt(a)), mpmath.mpf(flt(b))
    if name == "atan2":
        return mpmath.atan2(x, y)
    if name == "hypot":
        return mpmath.sqrt(x * x + y * y)
    if name == "powf":
        if x < 0:
            r = mpmath.power(-x, y)
            return -r if parity(b) == 1 else r
        return mpmath.power(x, y)
    raise KeyError(name)


def row(inputs, special, exact):
    s = special(*inputs)
    ins = " ".join(f"{i:016x}" for i in inputs)
    if s is not None:
        return f"{ins} {s:016x} {0:016x} x -"
    hi, lo = pair(exact(*inputs))
    return f"{ins} {hi:016x} {lo:016x} u -"


def row_exact_fn(inputs, fn):
    a = flt(inputs[0])
    if math.isnan(a):
        r = NAN
    elif math.isinf(a) or a == 0 or abs(a) >= 2.0**52:
        r = inputs[0]
    else:
        f = Fraction(a)
        q = {"floor": math.floor(f), "ceil": math.ceil(f)}.get(fn)
        if fn == "round_half_even":
            q = round(f)
        r = bits(float(q))
        if q == 0 and a < 0:
            r |= SIGN
    return f"{inputs[0]:016x} {r:016x} {0:016x} x -"


def write(name, header, rows):
    with open(f"{name}.txt", "w") as f:
        f.write(f"# SPDX-License-Identifier: Apache-2.0\n# {header}\n")
        f.write("# columns: inputs, reference hi, reference lo, kind (x exact bits, u ulp bound), pinned\n")
        for r in rows:
            f.write(r + "\n")


def specials_f():
    nan2 = 0xFFF8000000000001
    return [0, SIGN, 1, SIGN | 1, 0x000FFFFFFFFFFFFF, 0x0010000000000000,
            bits(1.0), bits(-1.0), INF, INF | SIGN, NAN, nan2]


def build():
    # exp
    edge = [0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 709.782712893384, 709.78271289338397,
            709.7827128933841, 710.0, -745.1332191019411, -745.2, -745.0, -744.0,
            -708.3964185322641, -708.4, 1e-10, -1e-10, 2.0**-28, 2.0**-29,
            0.34657359027997264, 0.34657359027997275, 1.0397207708399179, math.log(2),
            -math.log(2), 100.0, -100.0, 700.0, 2.0**-1000, 5e-324]
    xs = [bits(v) for v in edge] + specials_f()
    for _ in range(300):
        xs.append(bits(rng.uniform(-745.0, 709.7)))
    for _ in range(100):
        xs.append(bits(signed(log_uniform(-40, 4))))
    write("exp", "exp(x), x from the edges and 400 random points", [row((a,), special_exp, lambda a: exact_unary("exp", a)) for a in xs])

    # ln
    edge = [1.0, nextafter(1.0, True), nextafter(1.0, False), 2.0, math.e, 0.5,
            math.sqrt(2), nextafter(math.sqrt(2), True), nextafter(math.sqrt(2), False),
            1 + 2.0**-20, 1 - 2.0**-20, 1 + 2.0**-21, 10.0, 0.1, 1e-300, 1e300, 5e-324,
            MIN_NORMAL, nextafter(MIN_NORMAL, False), MAX, 3.0, 1024.0, 0.7071067811865476, 0.7071067811865475]
    xs = [bits(v) for v in edge] + specials_f()
    for _ in range(250):
        xs.append(bits(log_uniform(-1074, 1023)))
    for _ in range(150):
        xs.append(bits(rng.uniform(0.5, 2.0)))
    write("ln", "ln(x), x from the edges and 400 random points", [row((a,), special_ln, lambda a: exact_unary("ln", a)) for a in xs])

    # sin, cos
    edge = [0.0, -0.0, 5e-324, 2.0**-27, 2.0**-26, 2.0**-28, math.pi / 4,
            nextafter(math.pi / 4, True), nextafter(math.pi / 4, False), math.pi / 2, math.pi,
            3 * math.pi / 2, 2 * math.pi, 1.0, 0.5, 100.0, 1e5, 1647098.0, -1647098.0,
            710.0, 355.0, 1e-5, 0.3, 0.7853981633974483, 0.6, 1.1, 1.6, 3.0, 6.0, 12345.678]
    xs = [bits(v) for v in edge] + [bits(-v) for v in edge[4:]] + specials_f()
    for _ in range(250):
        xs.append(bits(signed(log_uniform(-30, 20.6))))
    for _ in range(150):
        k = rng.randint(1, 1000000)
        xs.append(bits(k * (math.pi / 2) + rng.uniform(-1e-3, 1e-3)))
    xs = [a for a in xs if (a & ~SIGN) <= INF and (is_inf(a) or is_nan(a) or abs(flt(a)) <= 1647098.0)]
    write("sin", "sin(x), x from the edges and random points up to the limit 1647098", [row((a,), special_sin, lambda a: exact_unary("sin", a)) for a in xs])
    write("cos", "cos(x), x from the edges and random points up to the limit 1647098", [row((a,), special_cos, lambda a: exact_unary("cos", a)) for a in xs])

    # atan2 (y, x)
    vals = [0.0, -0.0, 1.0, -1.0, 5e-324, -5e-324, MAX, -MAX, 1e-300, 1e300, 2.0**60, 2.0**-60, 2.0**70]
    ys = [bits(v) for v in vals] + specials_f()
    pairs = [(a, b) for a in ys for b in ys]
    for _ in range(250):
        pairs.append((bits(signed(log_uniform(-60, 60))), bits(signed(log_uniform(-60, 60)))))
    for _ in range(100):
        pairs.append((bits(signed(rng.uniform(0.01, 5))), bits(signed(rng.uniform(0.01, 5)))))
    write("atan2", "atan2(y, x), the edges crossed with each other and random pairs", [row(p, special_atan2, lambda a, b: exact_binary("atan2", a, b)) for p in pairs])

    # hypot
    vals = [0.0, -0.0, 3.0, 4.0, 5e-324, 1e-320, MIN_NORMAL, 1.0, 1e300, 1e308, MAX, 2.0**500, 2.0**-500, 2.0**600, 1e-200]
    ys = [bits(v) for v in vals] + specials_f()
    pairs = [(a, b) for a in ys for b in ys]
    for _ in range(250):
        pairs.append((bits(signed(log_uniform(-1070, 1020))), bits(signed(log_uniform(-1070, 1020)))))
    for _ in range(150):
        e = rng.uniform(-30, 30)
        pairs.append((bits(signed(2.0**e * rng.uniform(1, 2))), bits(signed(2.0**(e + rng.uniform(-3, 0)) * rng.uniform(1, 2)))))
    pairs = [p for p in pairs if True]
    write("hypot", "hypot(x, y), the edges crossed with each other and random pairs", [row(p, special_hypot, lambda a, b: exact_binary("hypot", a, b)) for p in pairs])

    # powf
    xv = [0.0, -0.0, 1.0, -1.0, 2.0, 0.5, -2.0, 10.0, 5e-324, MAX, MIN_NORMAL, 1.0000000000000002, 0.9999999999999999]
    yv = [0.0, -0.0, 1.0, -1.0, 2.0, 3.0, 0.5, -0.5, 1e10, -1e10, 2.0**53, 1075.0, -1075.0, 1024.0, 1023.5, 3.5, 0.3333333333333333]
    xs = [bits(v) for v in xv] + specials_f()
    ys = [bits(v) for v in yv] + specials_f()
    pairs = [(a, b) for a in xs for b in ys]
    for _ in range(200):
        pairs.append((bits(log_uniform(-40, 40)), bits(rng.uniform(-30, 30))))
    for _ in range(100):
        pairs.append((bits(rng.uniform(0.5, 1.5)), bits(signed(log_uniform(-5, 30)))))
    for _ in range(60):
        pairs.append((bits(-log_uniform(-5, 10)), bits(float(rng.randint(-40, 40)))))
    for _ in range(60):
        pairs.append((bits(log_uniform(-1000, 1000)), bits(rng.uniform(-1, 1))))
    write("powf", "powf(x, y), the edges crossed with each other and random pairs", [row(p, special_powf, lambda a, b: exact_binary("powf", a, b)) for p in pairs])

    # the exact functions
    edge = [0.0, -0.0, 0.5, -0.5, 1.5, -1.5, 2.5, -2.5, 3.5, 0.49999999999999994,
            -0.49999999999999994, 4503599627370495.5, 4503599627370496.0, -4503599627370495.5,
            1e300, 5e-324, -5e-324, MAX, 1.0, -1.0, 0.1, -0.1, 123456.789, -123456.789,
            0.5000000000000001, 2.0**52 - 0.5, 9007199254740993.0]
    xs = [bits(v) for v in edge] + specials_f()
    for _ in range(100):
        xs.append(bits(signed(log_uniform(-5, 55))))
    for _ in range(60):
        xs.append(bits(math.floor(rng.uniform(-100, 100)) + rng.choice([0.5, 0.25, 0.75, 0.0, 1e-9])))
    for name in ("floor", "ceil", "round_half_even"):
        write(name, f"{name}(x), exact: the edges and random points", [row_exact_fn((a,), name) for a in xs])
    write("abs", "abs(x), exact: the edges", [f"{a:016x} {(NAN if is_nan(a) else a & ~SIGN):016x} {0:016x} x -" for a in xs])


build()
