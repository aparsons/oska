// SPDX-License-Identifier: Apache-2.0
//! `Hash` for the core key types and its derivation (#600, ADR 0028).
//!
//! The golden values are the fixed 64-bit FNV-1a of `oska_check::hashing`,
//! worked out here independently of the code: an integer or `Bool` hashes
//! as the FNV-1a of its 8 little-endian bytes, text as the FNV-1a of its
//! UTF-8 bytes, and anything else as the state of its variant's index
//! (a list's length) mixed with the hash of each field or item. Every
//! helper that runs a program also runs it natively on Linux and compares
//! (`common/mod.rs`), so each golden value holds on both backends.

mod common;

use common::{assert_refused, assert_runs, run_fake};

/// A program of `items` and a `main` that prints each line of `body`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// A `println` of each `Int` expression, then `Ok(())`. An expression
/// that starts with `?` is a `Bool`, shown as `true` or `false`.
fn printing(exprs: &[&str]) -> String {
    let mut body = String::new();
    for (at, e) in exprs.iter().enumerate() {
        match e.strip_prefix('?') {
            Some(cond) => body.push_str(&format!(
                "    let v{at} = if {cond} {{ \"true\" }} else {{ \"false\" }}\n    io.println(v{at})?\n"
            )),
            None => body.push_str(&format!(
                "    let v{at} = {e}\n    io.println(v{at}.to_string())?\n"
            )),
        }
    }
    body.push_str("    Ok(())");
    body
}

#[test]
fn integers_and_bool_have_golden_hashes() {
    let src = program(
        "",
        &format!(
            "    let zero = 0\n    let one = 1\n    let minus = -1\n    let n = 42\n    \
             let byte: U8 = 255\n    let small: I8 = -1\n    let big: U64 = 18446744073709551615\n    \
             let yes = true\n    let no = false\n{}",
            printing(&[
                "zero.hash()",
                "one.hash()",
                "minus.hash()",
                "n.hash()",
                "byte.hash()",
                "small.hash()",
                "big.hash()",
                "yes.hash()",
                "no.hash()",
            ])
        ),
    );
    assert_runs(
        &src,
        &[],
        "-6284781860667377211\n-8517097267634966620\n-8289690350564177859\n\
         -55488592825689361\n-8064062821143829734\n-8289690350564177859\n\
         -8289690350564177859\n-8517097267634966620\n-6284781860667377211\n",
    );
}

#[test]
fn text_and_path_hash_their_bytes() {
    let src = program(
        "",
        &format!(
            "    let empty = \"\"\n    let a = \"a\"\n    let accent = \"h\u{e9}\"\n    let path = Path.new(\"a\")\n{}",
            printing(&[
                "empty.hash()",
                "a.hash()",
                "accent.hash()",
                "path.hash()",
                "a.to_bytes().hash()",
            ])
        ),
    );
    assert_runs(
        &src,
        &[],
        "-3750763034362895579\n-5808556873153909620\n3544597997190567535\n\
         -5808556873153909620\n-5808556873153909620\n",
    );
}

#[test]
fn option_and_list_hash_their_parts() {
    let src = program(
        "",
        &format!(
            "    let some: Option<Int> = Some(1)\n    let none: Option<Int> = None\n    \
             let empty: List<Int> = []\n    let items = [1, 2, 3]\n{}",
            printing(&["some.hash()", "none.hash()", "empty.hash()", "items.hash()",])
        ),
    );
    assert_runs(
        &src,
        &[],
        "3114138370207592541\n-8517097267634966620\n-6284781860667377211\n\
         -3460496380521443797\n",
    );
}

const SHAPES: &str = "struct Point { x: Int, y: Int }

enum Shape {
    Circle(Int),
    Square { w: Int, h: Int },
    Empty,
}

enum Color {
    Red,
    Green,
}

fn key<T: Hash>(value: T) -> Int {
    value.hash()
}";

#[test]
fn a_struct_or_enum_of_hash_parts_derives_hash() {
    let src = program(
        SHAPES,
        &format!(
            "    let p = Point(x: 1, y: 2)\n    let circle = Shape.Circle(5)\n    \
             let square = Shape.Square(w: 1, h: 2)\n{}",
            printing(&[
                "p.hash()",
                "key(p)",
                "circle.hash()",
                "key(square)",
                "key(Shape.Empty)",
                "key(Color.Green)",
            ])
        ),
    );
    assert_runs(
        &src,
        &[],
        "7123698491999978714\n7123698491999978714\n-1660146765292881040\n\
         7850579566139451943\n-1820151046732198393\n-8517097267634966620\n",
    );
}

#[test]
fn derived_hash_reaches_through_generics_options_and_lists() {
    let src = program(
        &format!(
            "{SHAPES}\n\nstruct Pair<A, B> {{ first: A, second: B }}\n\n\
             enum Tree {{\n    Leaf,\n    Node(List<Tree>),\n}}"
        ),
        &printing(&[
            "?key(Pair(first: 1, second: 2)) == key(Pair(first: 1, second: 2))",
            "?key(Pair(first: 1, second: 2)) == key(Pair(first: 2, second: 1))",
            "?key(Some(Point(x: 1, y: 2))) == key(Some(Point(x: 1, y: 2)))",
            "?key([Point(x: 1, y: 2)]) == key([Point(x: 1, y: 3)])",
            "?key(Tree.Node([Tree.Leaf, Tree.Node([])])) == key(Tree.Node([Tree.Leaf, Tree.Node([])]))",
            "?key(Tree.Node([Tree.Leaf])) == key(Tree.Node([Tree.Node([])]))",
        ]),
    );
    assert_runs(&src, &[], "true\nfalse\ntrue\nfalse\ntrue\nfalse\n");
}

#[test]
fn a_hash_impl_the_program_writes_is_used_instead() {
    let src = program(
        "struct Tag { id: Int, note: String }

impl Hash for Tag {
    fn hash(self) -> Int { self.id }
}

struct Owner { tag: Tag }

fn key<T: Hash>(value: T) -> Int {
    value.hash()
}",
        &printing(&[
            "Tag(id: 7, note: \"x\").hash()",
            "key(Tag(id: 7, note: \"y\"))",
            "?key(Owner(tag: Tag(id: 7, note: \"z\"))) == key(Owner(tag: Tag(id: 7, note: \"w\")))",
        ]),
    );
    assert_runs(&src, &[], "7\n7\ntrue\n");
}

#[test]
fn the_hash_of_untrusted_text_is_the_hash_of_the_same_trusted_text() {
    // `Hash.hash` follows trust: untrusted text is a receiver, and so is a
    // type argument of a generic `Hash` function.
    let src = program(
        "fn key<T: Hash>(value: T) -> Int {
    value.hash()
}

fn count(n: Int) -> Int {
    n
}",
        &format!(
            "    let line: Untrusted<String> = \"a\"\n    let outside = count(line.hash())\n    \
             let generic = key(line)\n    let trusted = \"a\"\n{}",
            printing(&["outside", "generic", "?outside == trusted.hash()"])
        ),
    );
    assert_runs(
        &src,
        &[],
        "-5808556873153909620\n-5808556873153909620\ntrue\n",
    );
}

#[test]
fn a_hash_of_untrusted_text_cannot_become_trusted_text() {
    // The hash `Int` carries no flag, but text built from it with an
    // untrusted part stays untrusted: the receiver's trust joins in.
    let src = program(
        "fn trusted(text: String) -> String {
    text
}",
        "    let line: Untrusted<String> = \"a\"\n    \
         let h = line.hash()\n    let shown = \"{line}:{h}\"\n    \
         io.println(trusted(shown))?\n    Ok(())",
    );
    assert_refused(&src, "E0300");
}

#[test]
fn a_type_that_holds_a_secret_is_refused_as_hashable() {
    let items = "struct Holder { token: Secret<String> }

struct Wrapped { inner: Holder }

fn key<T: Hash>(value: T) -> Int {
    value.hash()
}";
    for (value, label) in [
        ("key(Holder(token: Secret.new(\"k\")))", "a struct"),
        (
            "key(Wrapped(inner: Holder(token: Secret.new(\"k\"))))",
            "a nested struct",
        ),
        ("key(Some(Holder(token: Secret.new(\"k\"))))", "an option"),
        ("key([Secret.new(\"k\")])", "a list of secrets"),
    ] {
        let src = program(items, &format!("    let h = {value}\n    Ok(())"));
        let outcome = run_fake(&src, &[]);
        assert!(
            outcome.diagnostics.iter().any(|d| d.code == "E0354"),
            "{label} that holds a secret should be refused with E0354: {outcome:#?}"
        );
    }
}

#[test]
fn a_type_with_an_unhashable_part_is_refused_as_hashable() {
    // A `Result`, a capability, a function: none is `Hash`, and neither is
    // a type that has one (`Float` too, once it exists, has no impl).
    let items = "struct Outcome { value: Result<Int, String> }

struct Handle { out: Stdout }

fn key<T: Hash>(value: T) -> Int {
    value.hash()
}

fn wrapped() -> Result<Int, Int> {
    Ok(1)
}";
    for value in [
        "key(Outcome(value: Ok(1)))",
        "key(Some(Outcome(value: Ok(1))))",
        "key(wrapped())",
    ] {
        let src = program(items, &format!("    let h = {value}\n    Ok(())"));
        assert_refused(&src, "E0265");
    }
    let src = program(items, "    let h = key(Handle(out: io))\n    Ok(())");
    assert_refused(&src, "E0265");
}

#[test]
fn a_secret_itself_is_not_hash() {
    let src = program(
        "",
        "    let secret: Secret<String> = Secret.new(\"k\")\n    let h = secret.hash()\n    Ok(())",
    );
    assert_refused(&src, "E0354");
}
