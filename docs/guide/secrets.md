# Secrets and constant-time code

A key, a password or a token must not leak: not into a log, not into an error message, and not through how long the code takes. A **secret value** in Oska is a `Secret<T>`. The checker keeps it out of text, and for the secret types that code computes on, it keeps the secret's value from deciding a branch, a memory address, a division or a shift amount. The native backend then checks that the code it writes branches and addresses memory without regard to the secret, so the parts of **constant time** that the [last section](#what-constant-time-means-here) lists hold in the executable. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints.

## The secret types

| Type | Comes from | For |
|---|---|---|
| `Secret<String>` | `env.secret("NAME")` only | passwords, API keys, tokens |
| `Secret<U8>`, `Secret<U32>`, `Secret<U64>` | `Secret.new(value)`, or reading a `Secret<Bytes>` | the words a cipher or a MAC computes on |
| `Secret<Bool>` | `ct_eq`, and `Secret.new` | the result of a comparison, kept secret |
| `Secret<Bytes>` | `Secret.new(bytes)` | key material |

`Secret.new(value)` is not an **audit site**: adding secrecy never weakens a guarantee. It makes the four kinds of type above other than `Secret<String>`, which code cannot make itself (`E0271`); for any other type it is `E0223`.

Every secret type keeps these rules:

- It has no `Display`, so it cannot be printed or turned into text (`E0352`), and it cannot be interpolated, nor can a value that holds one (`E0350`).
- It cannot be compared with `==` or `!=` (`E0351`). The fix rewrites the comparison to the constant-time one for the type.
- `dbg` prints it as `<redacted>`, in generic code too.
- It satisfies no `Eq`, `Hash` or `Display` bound: it is not `Hash`, and it cannot become text through a generic function either.
- `.expose()` is the only way out. It returns the public value, and each call is an audit site that `oskac check` lists, so a reviewer sees every place a secret becomes public.

`Secret<String>`, read from the environment, is covered in [untrusted data and secrets](untrusted-data.md#secrets). The rest of this page is about the secret types code computes on.

## Computing on secret integers

Secret integers have the operations of `U8`, `U32` and `U64`, changed so that none can branch or panic on the secret:

- `+`, `-` and `*` wrap. An overflow check would branch on the value.
- `&`, `|`, `^`, `~`, `<<` and `>>` work as on the public type, and so do `rotl(n)` and `rotr(n)`.
- A public operand of the same type, a literal included, joins to a secret result: `k ^ 92` is a `Secret<U8>` when `k` is.
- `to_u32()` and `to_u64()` widen. `wrapping_to_u8()` and `wrapping_to_u32()` narrow and keep the low bits. There is no checked narrowing, because it would panic on a secret.
- `a.ct_eq(b)` compares two secret integers of one type into a `Secret<Bool>`.

`Secret<Bool>` combines with `and`, `or`, `xor` and `not`, which always evaluate both sides. `c.ct_select(a, b)` gives `a` when `c` holds and `b` otherwise, between two secret integers of one type, two `Secret<Bool>`s, or two `Secret<Bytes>` of the same length. It evaluates both, and the native backend lowers it to a conditional select or a mask, never a branch.

```oska
fn mix(key: Secret<U32>, word: U32) -> Secret<U32> {
    let x = key ^ word
    (x + 2654435769).rotl(7)
}

fn pick(a: Secret<U32>, b: Secret<U32>, want_a: Secret<Bool>) -> Secret<U32> {
    want_a.ct_select(a, b)
}

fn main(io: Stdout) -> Result<(), IoError> {
    let key: Secret<U32> = Secret.new(305419896)
    let mixed = mix(key, 42)
    let same = mixed.ct_eq(mix(key, 42))
    let chosen = pick(mixed, key, same)
    let shown = chosen.expose().to_int()
    io.println("tag {shown}")?
    Ok(())
}
```

```console cmd=run
tag 904398296
```

## Secret bytes

`Secret<Bytes>` holds a key. Its length is public; its contents are not.

- `len()` is a public `Int`.
- `at(i)`, `read_u32_be(offset)`, `read_u64_be(offset)` and their `_le` forms read at a public offset and give secret integers. `slice(start, end)` takes public bounds and gives `Secret<Bytes>`.
- `push`, `append` and the endian pushes take secrets.
- `constant_time_eq(bytes)` compares it with trusted or untrusted bytes, and `constant_time_eq_secret(other)` with another secret. Both give a public `Bool` and look at every byte, so the time they take reveals only the result and the lengths.

A panic in a `Secret<Bytes>` method, such as a read past the end (`P0042`), depends only on the public length and offset.

```oska
fn tag_matches(key: Secret<Bytes>, offered: Untrusted<Bytes>) -> Bool {
    key.constant_time_eq(offered)
}

fn first_word(key: Secret<Bytes>) -> Secret<U32> {
    key.read_u32_le(0)
}

fn main(io: Stdout) -> Result<(), IoError> {
    let raw = Bytes.from_hex("000102030405060708090a0b0c0d0e0f").unwrap_or(Bytes.new())
    let key = Secret.new(raw)
    let size = key.len()
    let word = first_word(key).expose().to_int()
    io.println("{size} bytes, first word {word}")?
    let offered = Bytes.from_hex("00").unwrap_or(Bytes.new())
    if tag_matches(key, offered) {
        io.println("match")?
    } else {
        io.println("no match")?
    }
    Ok(())
}
```

```console cmd=run
16 bytes, first word 50462976
no match
```

## What the checker refuses

A secret may not decide anything observable. Each kind of use has its own code, reported in place of a plain type error, and the rules follow the types through a generic function, a closure, a struct field and a `List` element.

**A branch (`E0353`).** A secret as the condition of an `if` or a `while`, a `requires` or `ensures` clause, a `match` scrutinee or guard, a range bound, or an operand of `&&`, `||`, `implies` or `!`. Combine conditions with `and`, `or` and `not`, and choose with `ct_select`. The fix rewrites `!a` to `a.not()`, and `a && b` and `a || b` to `a.and(b)` and `a.or(b)` when both operands are secret; with one public `Bool` operand there is no fix.

```oska error=E0353
fn choose(c: Secret<Bool>, a: Secret<U32>, b: Secret<U32>) -> Secret<U32> {
    if c {
        a
    } else {
        b
    }
}
```

```oska
fn choose(c: Secret<Bool>, a: Secret<U32>, b: Secret<U32>) -> Secret<U32> {
    c.ct_select(a, b)
}
```

**A memory address (`E0354`).** A secret as an index, an offset, a slice bound, a length or a count, or as a hash key. Index with a public value.

```oska error=E0354
fn lookup(table: List<U8>, k: Secret<U8>) -> U8 {
    table.at(k)
}
```

**A variable-time instruction or a panic (`E0355`).** A secret operand of `/` or `%`, or a secret shift or rotation amount. Division takes longer for some values, and a shift amount out of range panics (`P0008`).

```oska error=E0355
fn spin(x: Secret<U32>, n: Secret<U32>) -> Secret<U32> {
    x << n
}
```

**An ordering (`E0356`).** A secret on either side of `<`, `<=`, `>` or `>=`. Compare for equality with `ct_eq`.

```oska error=E0356
fn below(a: Secret<U32>, b: Secret<U32>) -> Bool {
    a < b
}
```

**An equality (`E0351`).** `==` and `!=` on secrets. The fix rewrites `a == b` to `a.ct_eq(b)` for secret integers, and to `constant_time_eq` or `constant_time_eq_secret` for secret bytes and text.

```oska error=E0351
fn same(a: Secret<U32>, b: Secret<U32>) -> Bool {
    a == b
}
```

`oskac explain <code>` gives each code's rule, an example and its fix.

## What constant time means here

The checker's rules mean that no operation on a secret takes a path, reads memory, or panics depending on its value, and that no secret is an operand of `/` or `%` or a shift or rotation amount (`E0355`).

The native backend checks part of that again in the machine code it writes, on every target platform. After optimizing, it follows each value read out of a secret, and each value computed from one, through the functions it is passed to and returned from. It refuses to build (`E0398`, exit code 2) if one would be the condition of a branch, an operand of checked arithmetic (which branches on overflow), the address of a load, a store or a call, or an argument of an indirect call. No program is meant to reach `E0398`; if one does, it is a bug in `oskac`, and the build stops instead of writing an executable that leaks.

What the guarantee does not cover:

- Variable-time instructions. The backend does not check which instructions a secret reaches; only the checker's `E0355` keeps secrets out of division and shift amounts.
- A secret stored to memory and loaded back. The backend's check does not follow it there; it marks secrets again where they are read out of their `Secret`.
- The interpreter (`oskac run --interpret`). It computes the same values and prints the same output, but makes no timing promise.
- What the hardware does with the instructions, and memory that a secret leaves behind when it is freed. Nothing in the toolchain measures timing.
