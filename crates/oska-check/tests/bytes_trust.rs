// SPDX-License-Identifier: Apache-2.0
//! Trust on bytes (#223): bytes from outside the program are
//! `Untrusted<Bytes>`, under the trust rules text has (ADR 0006).
//! Untrusted bytes never flow into a trusted `Bytes` slot (`E0300`), every
//! `Bytes` operation that gives bytes or text follows the trust of its
//! inputs, and `.assume_trusted()` on bytes is an audit site.

mod common;

use common::{assert_audit, assert_errors, assert_ok, check};

/// Assert that `src` reports one error, `code` at `text`, whose message
/// contains `fragment`.
fn assert_error(src: &str, code: &str, text: &str, fragment: &str) {
    assert_errors(src, &[(code, text)]);
    let message = &check(src).diagnostics[0].message;
    assert!(message.contains(fragment), "{src}: {message}");
}

/// The message fragment of `E0300` for untrusted bytes in a `Bytes` slot.
const LOST: &str =
    "expected `Bytes`, found `Untrusted<Bytes>`: untrusted bytes cannot be used where trusted bytes are required";

/// A program with `fn keep(b: Bytes)`, a trusted slot, and `fn f` whose
/// parameters are `params` and whose body is `body`.
fn with_keep(params: &str, body: &str) -> String {
    format!("fn keep(b: Bytes) {{\n}}\nfn f({params}) {{\n{body}\n}}")
}

// ---- Sources --------------------------------------------------------------

#[test]
fn attack_bytes_read_from_a_directory_cannot_fill_a_trusted_slot() {
    assert_error(
        &with_keep(
            "dir: DirRead",
            "    match dir.read_bytes(Path.from(\"blob\")) {\n        Ok(raw) => keep(raw)\n        Err(_) => {}\n    }",
        ),
        "E0300",
        "raw",
        LOST,
    );
}

#[test]
fn bytes_read_from_a_file_capability_are_untrusted() {
    assert_error(
        "fn f(file: FileRead) -> Result<Bytes, IoError> {\n    let raw = file.read_bytes()?\n    Ok(raw)\n}",
        "E0300",
        "raw",
        LOST,
    );
}

#[test]
fn bytes_received_on_a_connection_are_untrusted() {
    assert_error(
        "fn f(c: Connection) -> Result<Bytes, IoError> {\n    c.receive_bytes()\n}",
        "E0300",
        "c.receive_bytes()",
        "expected `Result<Bytes, IoError>`, found `Result<Untrusted<Bytes>, IoError>`",
    );
}

#[test]
fn untrusted_bytes_are_accepted_by_every_byte_write() {
    assert_ok(
        "fn f(dir: DirRead, out: DirWrite, file: FileWrite, c: Connection) -> Result<(), IoError> {\n    \
         let raw = dir.read_bytes(Path.from(\"in\"))?\n    \
         out.write_bytes(Path.from(\"out\"), raw)?\n    \
         file.write_bytes(raw)?\n    \
         c.send_bytes(raw)\n}",
    );
}

#[test]
fn trusted_bytes_are_accepted_where_untrusted_bytes_are() {
    assert_ok(
        "fn take(b: Untrusted<Bytes>) -> Int {\n    b.len()\n}\n\
         fn f(out: FileWrite) -> Result<Int, IoError> {\n    \
         out.write_bytes(\"ok\".to_bytes())?\n    Ok(take(Bytes.zeroed(4)))\n}",
    );
}

// ---- Operations follow trust -------------------------------------------------

#[test]
fn bytes_operations_on_trusted_bytes_give_trusted_bytes() {
    assert_ok(&with_keep(
        "",
        "    var b = \"abc\".to_bytes()\n    b.push(1)\n    b.append(Bytes.zeroed(2))\n    \
         b.push_u32_be(7)\n    keep(b.slice(0, 2))\n    keep(b)\n    \
         match Bytes.from_hex(b.to_hex()) {\n        Some(h) => keep(h)\n        None => {}\n    }",
    ));
}

/// The program `with_keep` makes with `raw`, untrusted bytes from a file,
/// in scope before `body`.
fn with_raw(body: &str) -> String {
    with_keep(
        "file: FileRead",
        &format!(
            "    match file.read_bytes() {{\n        Ok(raw) => {{\n{body}\n        }}\n        Err(_) => {{}}\n    }}"
        ),
    )
}

#[test]
fn a_slice_of_untrusted_bytes_is_untrusted() {
    assert_error(
        &with_raw("            keep(raw.slice(0, 1))"),
        "E0300",
        "raw.slice(0, 1)",
        LOST,
    );
}

#[test]
fn bytes_from_the_hex_of_untrusted_bytes_are_untrusted() {
    assert_error(
        &with_raw("            match Bytes.from_hex(raw.to_hex()) {\n                Some(h) => keep(h)\n                None => {}\n            }"),
        "E0300",
        "h",
        LOST,
    );
}

#[test]
fn the_text_of_untrusted_bytes_is_untrusted_text() {
    assert_error(
        "fn name(text: String) {\n}\n\
         fn f(file: FileRead) -> Result<(), IoError> {\n    let raw = file.read_bytes()?\n    \
         match raw.to_text() {\n        Some(text) => name(text)\n        None => {}\n    }\n    Ok(())\n}",
        "E0300",
        "text",
        "expected `String`, found `Untrusted<String>`",
    );
}

#[test]
fn the_hex_of_untrusted_bytes_is_untrusted_text() {
    assert_error(
        "fn name(text: String) {\n}\n\
         fn f(file: FileRead) -> Result<(), IoError> {\n    let raw = file.read_bytes()?\n    \
         name(raw.to_hex())\n    Ok(())\n}",
        "E0300",
        "raw.to_hex()",
        "expected `String`, found `Untrusted<String>`",
    );
}

#[test]
fn the_bytes_of_untrusted_text_are_untrusted() {
    assert_error(
        "fn keep(b: Bytes) {\n}\n\
         fn f(args: Args) -> Option<()> {\n    let arg = args.get(1)?\n    keep(arg.to_bytes())\n    Some(())\n}",
        "E0300",
        "arg.to_bytes()",
        LOST,
    );
}

#[test]
fn integers_read_from_untrusted_bytes_are_trusted() {
    assert_ok(
        "fn count(n: Int) -> Int {\n    n\n}\n\
         fn byte(b: U8) -> U8 {\n    b\n}\n\
         fn word(w: U32) -> U32 {\n    w\n}\n\
         fn yes(b: Bool) -> Bool {\n    b\n}\n\
         fn f(file: FileRead) -> Result<(), IoError> {\n    let raw = file.read_bytes()?\n    \
         let _ = count(raw.len())\n    let _ = byte(raw.at(0))\n    let _ = word(raw.read_u32_be(0))\n    \
         let _ = raw.get(0)\n    let _ = count(raw.hash())\n    let _ = yes(raw == Bytes.new())\n    \
         var mine = Bytes.new()\n    mine.push(raw.at(0))\n    mine.push_u32_le(raw.read_u32_le(0))\n    \
         keep(mine)\n    Ok(())\n}\n\
         fn keep(b: Bytes) {\n}",
    );
}

#[test]
fn appending_untrusted_bytes_to_trusted_bytes_is_refused() {
    assert_error(
        &with_raw("            var mine: Bytes = Bytes.new()\n            mine.append(raw)"),
        "E0300",
        "raw",
        LOST,
    );
}

#[test]
fn bytes_that_untrusted_bytes_were_appended_to_are_untrusted() {
    assert_error(
        &with_raw("            var mine = Bytes.new()\n            mine.append(raw)\n            keep(mine)"),
        "E0300",
        "mine",
        LOST,
    );
}

#[test]
fn a_var_started_empty_can_gather_untrusted_bytes() {
    // As `var s = ""` can later hold untrusted text, a `var` started from
    // `Bytes.new()` or `Bytes.zeroed(n)` takes untrusted bytes, and is
    // untrusted from then on.
    for start in ["Bytes.new()", "Bytes.zeroed(2)"] {
        assert_ok(&format!(
            "fn take(b: Untrusted<Bytes>) {{\n}}\n\
             fn f(file: FileRead) -> Result<(), IoError> {{\n    \
             let raw = file.read_bytes()?\n    var gathered = {start}\n    gathered.append(raw)\n    \
             take(gathered)\n    var swapped = {start}\n    swapped = raw\n    take(swapped)\n    Ok(())\n}}"
        ));
    }
}

// ---- The trust assertion ---------------------------------------------------

#[test]
fn assume_trusted_makes_untrusted_bytes_trusted() {
    assert_ok(&with_raw("            keep(raw.assume_trusted())"));
}

#[test]
fn assume_trusted_on_bytes_is_an_audit_site() {
    assert_audit(
        &with_raw("            keep(raw.assume_trusted())"),
        &[("assume_trusted", "raw.assume_trusted()")],
    );
}

// ---- `Untrusted<...>` (`E0320`) ----------------------------------------------

#[test]
fn untrusted_bytes_can_be_declared() {
    assert_ok(
        "struct Packet { body: Untrusted<Bytes> }\n\
         fn f(p: Packet) -> Untrusted<Bytes> {\n    let b: Untrusted<Bytes> = p.body\n    b\n}",
    );
}

#[test]
fn untrusted_of_an_integer_is_still_an_error() {
    assert_error(
        "fn f(n: Untrusted<Int>) {\n}",
        "E0320",
        "Untrusted<Int>",
        "marks text and bytes only",
    );
}

#[test]
fn untrusted_of_a_list_of_bytes_is_still_an_error() {
    assert_error(
        "fn f(n: Untrusted<List<U8>>) {\n}",
        "E0320",
        "Untrusted<List<U8>>",
        "marks text and bytes only",
    );
}

// ---- Sinks ---------------------------------------------------------------------

#[test]
fn bytes_where_a_sink_literal_is_required_are_a_plain_type_mismatch() {
    // Only text gets `E0341`'s advice to write a literal: bytes are no
    // kind of text, so they are the wrong type, trusted or not.
    for bytes in ["raw", "Bytes.new()"] {
        assert_error(
            &format!(
                "use std.sql\n\
                 fn f(db: sql.Db, raw: Untrusted<Bytes>) -> Result<List<sql.Row>, sql.SqlError> {{\n    \
                 db.query({bytes}, [])\n}}"
            ),
            "E0220",
            bytes,
            "expected `SqlLiteral`, found `",
        );
    }
}

// ---- Laundering ---------------------------------------------------------------

#[test]
fn a_generic_function_does_not_launder_untrusted_bytes() {
    assert_error(
        &format!(
            "fn same<T>(x: T) -> T {{\n    x\n}}\n{}",
            with_raw("            keep(same(raw))")
        ),
        "E0300",
        "raw",
        LOST,
    );
}

#[test]
fn a_generic_function_returning_fixed_bytes_does_not_launder() {
    assert_error(
        &format!(
            "fn launder<T: Hash>(x: T) -> Bytes {{\n    Bytes.zeroed(x.hash() % 2)\n}}\n{}",
            with_raw("            keep(launder(raw))")
        ),
        "E0300",
        "launder(raw)",
        LOST,
    );
}

#[test]
fn a_closure_does_not_launder_untrusted_bytes() {
    assert_error(
        &with_raw("            let give = || raw\n            keep(give())"),
        "E0300",
        "give()",
        LOST,
    );
}

#[test]
fn untrusted_bytes_cannot_fill_a_trusted_field() {
    assert_error(
        &format!(
            "struct Blob {{ data: Bytes }}\n{}",
            with_raw("            let _ = Blob(data: raw)")
        ),
        "E0300",
        "raw",
        LOST,
    );
}

#[test]
fn a_list_of_trusted_and_untrusted_bytes_holds_untrusted_bytes() {
    assert_error(
        &with_raw("            let all = [Bytes.new(), raw]\n            keep(all.at(0))"),
        "E0300",
        "all.at(0)",
        LOST,
    );
}

#[test]
fn if_branches_of_trusted_then_untrusted_bytes_are_untrusted() {
    assert_error(
        &with_raw("            let b = if raw.len() > 1 { Bytes.new() } else { raw }\n            keep(b)"),
        "E0300",
        "b",
        LOST,
    );
}

#[test]
fn a_trait_method_on_trusted_bytes_rejects_untrusted_bytes() {
    assert_error(
        &format!(
            "trait Pack {{\n    fn pack(self) -> Bytes\n}}\nimpl Pack for Bytes {{\n    fn pack(self) -> Bytes {{\n        self\n    }}\n}}\n{}",
            with_raw("            keep(raw.pack())")
        ),
        "E0300",
        "raw",
        LOST,
    );
}

// ---- Oska Core -----------------------------------------------------------------

/// The Lean model of Oska Core needs no change for trust on bytes: the
/// core IR export refuses each new byte method of a capability, and
/// `.assume_trusted()` on bytes, as a built-in outside Oska Core, so no
/// program it accepts can reach one. Each probe's call is the first
/// construct the export meets (it looks at a callee before its arguments,
/// the receiver among them, so a file capability made in the body is not
/// refused first), and the refusal must name the method, so a byte method
/// let into Oska Core fails here.
#[test]
fn no_oska_core_program_can_reach_a_byte_capability_method() {
    const PROBES: &[(&str, &str, &str)] = &[
        (
            "d: DirRead",
            "d.read_bytes(Path.from(\"a\"))",
            "the built-in `DirRead.read_bytes`",
        ),
        (
            "d: DirWrite",
            "d.write_bytes(Path.from(\"a\"), Bytes.new())",
            "the built-in `DirWrite.write_bytes`",
        ),
        (
            "d: DirRead",
            "d.restrict_to_file(Path.from(\"a\")).read_bytes()",
            "the built-in `FileRead.read_bytes`",
        ),
        (
            "d: DirWrite",
            "d.restrict_to_file(Path.from(\"a\")).write_bytes(Bytes.new())",
            "the built-in `FileWrite.write_bytes`",
        ),
        (
            "d: DirRead",
            "d.read_bytes(Path.from(\"a\")).unwrap_or(Bytes.new()).assume_trusted()",
            "the built-in `Bytes.assume_trusted`",
        ),
    ];
    for (param, call, construct) in PROBES {
        let src = format!("fn probe({param}) {{\n    let _ = {call}\n}}\n\nfn main() {{\n}}\n");
        let report = oska_check::check_module(&src);
        let program = report
            .program
            .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics));
        let refusal = oska_check::core_ir::export(&program)
            .expect_err("a program with a byte method is outside Oska Core");
        assert_eq!(refusal.construct, *construct, "{call}: {refusal}");
    }
}
