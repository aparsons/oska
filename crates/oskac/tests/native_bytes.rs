//! `Bytes` (ticket #222), run on both backends through the differential
//! harness: every method against fixed byte strings, copy-on-write (a
//! changed copy leaves the original unchanged), the endian reads and
//! pushes, each bounds and length panic, hex and UTF-8 round trips, `Eq`,
//! `Hash` and `dbg`; and, natively, that buffers are freed and grow in
//! place.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, in_parallel, Case};

/// A program whose `main` runs `body` with `check(ok, what)`, which
/// prints `bad <what>` for each check that does not hold, and `hex(b)`,
/// then prints `done`.
fn checking(body: &str) -> String {
    format!(
        "fn check(io: Stdout, ok: Bool, what: String) {{\n    \
         if !ok {{\n        let _ = io.println(\"bad {{what}}\")\n    }}\n}}\n\n\
         fn hex(text: String) -> Bytes {{\n    \
         match Bytes.from_hex(text) {{\n        \
         Some(b) => b\n        \
         None => Bytes.zeroed(1000)\n    }}\n}}\n\n\
         fn main(io: Stdout) {{\n{body}    let _ = io.println(\"done\")\n}}\n"
    )
}

fn assert_all_hold(name: &str, body: &str) {
    let outcome = assert_same(&Case::file(name, &checking(body)));
    assert_eq!(outcome.stdout, "done\n", "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// One `check` line: `expr` must hold.
fn check(expr: &str) -> String {
    let what = expr.replace('"', "'").replace('{', "(").replace('}', ")");
    format!("    check(io, {expr}, \"{what}\")\n")
}

#[test]
fn constructors_length_and_indexing() {
    let mut body = String::from(
        "    let empty = Bytes.new()\n    \
         let z = Bytes.zeroed(5)\n    \
         let b = hex(\"00017f80ff\")\n",
    );
    for expr in [
        "empty.len() == 0",
        "z.len() == 5",
        "z.to_hex() == \"0000000000\"",
        "Bytes.zeroed(0).len() == 0",
        "Bytes.zeroed(70000).at(69999) == 0",
        "b.len() == 5",
        "b.at(0) == 0",
        "b.at(1) == 1",
        "b.at(2) == 127",
        "b.at(3) == 128",
        "b.at(4) == 255",
        "b.at(4).to_int() == 255",
        "b.get(4) == Some(255)",
        "b.get(5) == None",
        "b.get(-1) == None",
        "empty.get(0) == None",
    ] {
        body.push_str(&check(expr));
    }
    assert_all_hold("bytes_basics", &body);
}

#[test]
fn hex_and_utf8_round_trip() {
    let mut body = String::from("    let b = hex(\"00017f80ff\")\n");
    for expr in [
        "b.to_hex() == \"00017f80ff\"",
        "Bytes.from_hex(\"DEADbeef\") == Some(hex(\"deadbeef\"))",
        "hex(\"DEADBEEF\").to_hex() == \"deadbeef\"",
        "Bytes.from_hex(\"\") == Some(Bytes.new())",
        "Bytes.from_hex(\"abc\") == None",
        "Bytes.from_hex(\"0g\") == None",
        "Bytes.from_hex(\"g0\") == None",
        "Bytes.from_hex(\" 0\") == None",
        "Bytes.new().to_hex() == \"\"",
        "\"héllo\".to_bytes().to_hex() == \"68c3a96c6c6f\"",
        "\"héllo\".to_bytes().len() == 6",
        "\"\".to_bytes().len() == 0",
        "\"héllo\".to_bytes().to_text() == Some(\"héllo\")",
        "hex(\"f09f9880\").to_text() == Some(\"😀\")",
        "Bytes.new().to_text() == Some(\"\")",
        // Not UTF-8: a lone continuation byte, an overlong encoding, a
        // surrogate, a truncated sequence, a byte never used, and a code
        // point past U+10FFFF.
        "hex(\"80\").to_text() == None",
        "hex(\"c0af\").to_text() == None",
        "hex(\"eda080\").to_text() == None",
        "hex(\"e282\").to_text() == None",
        "hex(\"61ff\").to_text() == None",
        "hex(\"f4908080\").to_text() == None",
    ] {
        body.push_str(&check(expr));
    }
    assert_all_hold("bytes_hex_utf8", &body);
}

#[test]
fn slices_and_endian_reads_against_fixed_bytes() {
    let mut body = String::from(
        "    let b = hex(\"0102030405060708090a\")\n    let ones = hex(\"ffffffffffffffff\")\n",
    );
    for expr in [
        "b.slice(0, 0).len() == 0",
        "b.slice(10, 10).len() == 0",
        "b.slice(2, 5) == hex(\"030405\")",
        "b.slice(0, 10) == b",
        "b.read_u16_be(0) == 258",
        "b.read_u16_le(0) == 513",
        "b.read_u16_be(8) == 2314",
        "b.read_u32_be(0) == 16909060",
        "b.read_u32_le(0) == 67305985",
        "b.read_u32_be(6) == 117967114",
        "b.read_u64_be(0) == 72623859790382856",
        "b.read_u64_le(0) == 578437695752307201",
        "b.read_u64_be(2) == 217304205466536202",
        "b.read_u64_le(2) == 723118041428460547",
        "ones.read_u16_be(6) == 65535",
        "ones.read_u32_le(4) == 4294967295",
        "ones.read_u64_be(0) == 18446744073709551615",
        "ones.read_u64_le(0) == 18446744073709551615",
        "hex(\"80000000000000ff\").read_u64_be(0) == 9223372036854776063",
        "hex(\"80000000000000ff\").read_u64_le(0) == 18374686479671623808",
    ] {
        body.push_str(&check(expr));
    }
    assert_all_hold("bytes_reads", &body);
}

#[test]
fn appends_and_endian_pushes_build_the_bytes_read_back() {
    let mut body = String::from(
        "    var b = Bytes.new()\n    \
         b.push(1)\n    \
         b.push(255)\n    \
         let two: U16 = 4660\n    \
         let four: U32 = 305419896\n    \
         let eight: U64 = 1311768467463790320\n    \
         let top: U64 = 18446744073709551615\n    \
         b.push_u16_be(two)\n    \
         b.push_u16_le(two)\n    \
         b.push_u32_be(four)\n    \
         b.push_u32_le(four)\n    \
         b.push_u64_be(eight)\n    \
         b.push_u64_le(eight)\n    \
         b.push_u64_le(top)\n    \
         b.set(0, 7)\n    \
         var c = hex(\"aabb\")\n    \
         c.append(Bytes.new())\n    \
         c.append(hex(\"cc\"))\n    \
         c.append(c)\n",
    );
    for expr in [
        "b.to_hex() == \"07ff12343412123456787856341212345678\
         9abcdef0f0debc9a78563412ffffffffffffffff\"",
        "b.read_u16_be(2) == two",
        "b.read_u16_le(4) == two",
        "b.read_u32_be(6) == four",
        "b.read_u32_le(10) == four",
        "b.read_u64_be(14) == eight",
        "b.read_u64_le(22) == eight",
        "b.read_u64_le(30) == top",
        "b.len() == 38",
        "c == hex(\"aabbccaabbcc\")",
    ] {
        body.push_str(&check(expr));
    }
    assert_all_hold("bytes_pushes", &body);
}

#[test]
fn equality_and_hash() {
    let mut body = String::from(
        "    let a = hex(\"0102\")\n    \
         let b = hex(\"0102\")\n",
    );
    for expr in [
        "a == b",
        "a != hex(\"0103\")",
        "a != hex(\"010200\")",
        "Bytes.new() == Bytes.zeroed(0)",
        "Bytes.zeroed(1) != Bytes.new()",
        "[a, b] == [b, a]",
        "Some(a) == Some(b)",
        // FNV-1a's own test vectors, as signed words.
        "Bytes.new().hash() == -3750763034362895579",
        "\"a\".to_bytes().hash() == -5808556873153909620",
        "\"foobar\".to_bytes().hash() == -8821353812377114648",
        "a.hash() == b.hash()",
        "same(a, b)",
    ] {
        body.push_str(&check(expr));
    }
    let src = checking(&body).replace(
        "fn main(",
        "fn same<T: Eq + Hash>(x: T, y: T) -> Bool {\n    x == y && x.hash() == y.hash()\n}\n\nfn main(",
    );
    let outcome = assert_same(&Case::file("bytes_eq_hash", &src));
    assert_eq!(outcome.stdout, "done\n", "{outcome:#?}");
}

#[test]
fn a_changed_copy_never_changes_the_original() {
    // Each mutating method on a copy, in a local, a struct field, a list
    // item and a parameter: the original keeps its bytes (ADR 0001).
    let src = r#"struct Packet { header: Bytes, body: Bytes }

fn grown(b: Bytes) -> Bytes {
    var c = b
    c.push(9)
    c
}

fn show(io: Stdout, b: Bytes) {
    let _ = io.println(b.to_hex())
}

fn main(io: Stdout) {
    let one: U16 = 1
    let original = Bytes.from_hex("0a0b0c").unwrap_or(Bytes.new())
    var a = original
    a.push(1)
    var b = original
    b.set(0, 255)
    var c = original
    c.append(original)
    var d = original
    d.push_u16_be(one)
    var e = original
    e.push_u16_le(one)
    let f = grown(original)
    show(io, original)
    show(io, a)
    show(io, b)
    show(io, c)
    show(io, d)
    show(io, e)
    show(io, f)
    var p = Packet(header: original, body: Bytes.new())
    let q = p
    p.header.set(1, 0)
    p.body.push(7)
    show(io, p.header)
    show(io, q.header)
    show(io, p.body)
    show(io, q.body)
    var list = [original, original]
    var item = list.at(0)
    item.push(5)
    show(io, item)
    show(io, list.at(0))
    var g = original
    let h = g
    g.set(2, 1)
    g.set(1, 1)
    show(io, g)
    show(io, h)
    let _ = dbg(original)
    let _ = dbg(Bytes.new())
    let _ = dbg([g, h])
    let _ = dbg(Some(q.header))
}
"#;
    let outcome = assert_same(&Case::file("bytes_copy_on_write", src));
    assert_eq!(
        outcome.stdout,
        "0a0b0c\n0a0b0c01\nff0b0c\n0a0b0c0a0b0c\n0a0b0c0001\n0a0b0c0100\n0a0b0c09\n\
         0a000c\n0a0b0c\n07\n\n0a0b0c05\n0a0b0c\n0a0101\n0a0b0c\n",
        "{outcome:#?}"
    );
    assert_eq!(
        outcome.stderr,
        "[dbg] Bytes(0a0b0c)\n[dbg] Bytes()\n[dbg] [Bytes(0a0101), Bytes(0a0b0c)]\n\
         [dbg] Option.Some(Bytes(0a0b0c))\n",
        "{outcome:#?}"
    );
}

#[test]
fn every_bounds_and_length_panic_matches_the_interpreter() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "P0002",
            "let v = b.at(4)",
            "index 4 is out of bounds for bytes of length 4",
        ),
        (
            "P0002",
            "let v = b.at(-1)",
            "index -1 is out of bounds for bytes of length 4",
        ),
        (
            "P0002",
            "var c = b\n    c.set(4, 1)",
            "index 4 is out of bounds for bytes of length 4",
        ),
        (
            "P0002",
            "var c = b\n    c.set(-2, 1)",
            "index -2 is out of bounds for bytes of length 4",
        ),
        (
            "P0042",
            "let v = b.slice(3, 2)",
            "cannot slice bytes 3..2 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.slice(0, 5)",
            "cannot slice bytes 0..5 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.slice(-1, 2)",
            "cannot slice bytes -1..2 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u16_be(3)",
            "cannot read 2 bytes at offset 3 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u16_le(5)",
            "cannot read 2 bytes at offset 5 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u32_be(1)",
            "cannot read 4 bytes at offset 1 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u32_le(-1)",
            "cannot read 4 bytes at offset -1 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u64_be(0)",
            "cannot read 8 bytes at offset 0 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = b.read_u64_le(9223372036854775807)",
            "cannot read 8 bytes at offset 9223372036854775807 of a buffer of 4 bytes",
        ),
        (
            "P0042",
            "let v = Bytes.zeroed(-3)",
            "cannot make a buffer of -3 bytes",
        ),
        (
            "P0043",
            "let v = Bytes.zeroed(268435457)",
            "a buffer of 268435457 bytes is too long",
        ),
        (
            "P0043",
            "var c = Bytes.zeroed(268435456)\n    c.push(1)",
            "a buffer of 268435457 bytes is too long",
        ),
        (
            "P0043",
            "var c = Bytes.zeroed(268435452)\n    let w: U64 = 1\n    c.push_u64_be(w)",
            "a buffer of 268435460 bytes is too long",
        ),
        (
            "P0043",
            "var c = Bytes.zeroed(268435455)\n    c.append(b)",
            "a buffer of 268435459 bytes is too long",
        ),
    ];
    let numbered: Vec<(usize, &(&str, &str, &str))> = cases.iter().enumerate().collect();
    in_parallel(&numbered, |&(i, &(code, body, message))| {
        let src = format!(
            "fn main(io: Stdout) {{\n    \
             let b = Bytes.from_hex(\"01020304\").unwrap_or(Bytes.new())\n    \
             let _ = io.println(\"before\")\n    {body}\n    \
             let _ = io.println(\"after\")\n}}\n"
        );
        let outcome = assert_same(&Case::file(&format!("bytes_panic_{i}"), &src));
        assert_eq!(outcome.stdout, "before\n", "{code} {body}: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(101), "{code} {body}: {outcome:#?}");
        let report = common::frames::without_frames(&outcome.stderr);
        assert!(
            report.contains(&format!(": error[{code}]: {message} (bytes ")),
            "{code} {body}: {outcome:#?}"
        );
    });
}

/// Pushing a byte at a time onto a buffer nothing else holds writes in
/// place, so a long buffer is built in linear time; buffers made and
/// dropped in a loop are freed. (Native only: the process runs with an
/// address space far smaller than a leak, or a copy per push, would
/// need.)
#[cfg(unix)]
#[test]
fn buffers_grow_in_place_and_are_freed() {
    let src = r#"fn main(io: Stdout) {
    var b = Bytes.new()
    var i = 0
    while i < 20000000 {
        b.push(7)
        i = i + 1
    }
    var total = 0
    var j = 0
    while j < 200000 {
        var piece = Bytes.zeroed(1000)
        piece.set(999, 1)
        let copy = piece.slice(500, 1000)
        total = total + copy.len() + piece.to_hex().len()
        j = j + 1
    }
    let n = b.len()
    let _ = io.println("{n} {total}")
}
"#;
    let case = Case::file("bytes_grow_and_free", src);
    let (exe, dir) = differential::build_native(&case);
    // At most 256 MiB, `main`'s stack reserved included: the 20 MB of
    // bytes, doubling, need under 64 MiB, while 200,000 buffers of 3.5 KB
    // never freed would need 700 MB.
    let output = differential::run_within_memory(&exe, &dir, 256);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "20000000 500000000\n",
        "{output:?}"
    );
    assert!(output.status.success(), "{output:?}");
}
