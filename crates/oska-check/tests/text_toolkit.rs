//! The text toolkit (#72): `starts_with`, `ends_with`, `replace`, `join`,
//! `chars`, `slice`, and `repeat` on `String`, and `FsRead.list_dir`.
//! Every text-returning method follows the trust of its inputs.

mod common;

use common::{assert_errors, assert_ok};

/// A function taking untrusted `arg` (the first program argument), then
/// `body`, which may use `arg`, a trusted `ok: String`, and `greet`, whose
/// trusted parameter is a structural position.
fn with_arg(ret: &str, body: &str) -> String {
    format!(
        "fn greet(name: String) -> String {{\n    name\n}}\n\
         fn f(args: Args, ok: String) -> Option<{ret}> {{\n    let arg = args.get(1)?\n{body}\n}}"
    )
}

// ---- Signatures on trusted text ------------------------------------------

#[test]
fn the_text_methods_check_on_trusted_text() {
    assert_ok(
        "fn f(s: String) -> String {\n    \
         let a: Bool = s.starts_with(\"ab\")\n    \
         let b: Bool = s.ends_with(\"yz\")\n    \
         let c: String = s.replace(\"a\", \"b\")\n    \
         let d: String = \",\".join(s.split(\",\"))\n    \
         let e: List<String> = s.chars()\n    \
         let g: String = s.slice(1, 3)\n    \
         let h: String = s.repeat(3)\n    \
         h\n}",
    );
}

#[test]
fn chars_can_be_iterated() {
    assert_ok(
        "fn f(s: String) -> Int {\n    var n = 0\n    for c in s.chars() {\n        if c == \"a\" {\n            n = n + 1\n        }\n    }\n    n\n}",
    );
}

#[test]
fn text_methods_reject_the_wrong_argument_types() {
    assert_errors(
        "fn f(s: String) -> String {\n    s.slice(\"a\", 2)\n}",
        &[("E0220", "\"a\"")],
    );
    assert_errors(
        "fn f(s: String) -> String {\n    s.repeat(\"x\")\n}",
        &[("E0220", "\"x\"")],
    );
    assert_errors(
        "fn f(s: String) -> String {\n    s.join(\"x\")\n}",
        &[("E0220", "\"x\"")],
    );
}

// ---- Trust propagates: through the receiver --------------------------------

#[test]
fn replacing_in_untrusted_text_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(arg.replace(\"a\", \"b\")))"),
        &[("E0300", "arg.replace(\"a\", \"b\")")],
    );
}

#[test]
fn slicing_untrusted_text_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(arg.slice(0, 2)))"),
        &[("E0300", "arg.slice(0, 2)")],
    );
}

#[test]
fn repeating_untrusted_text_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(arg.repeat(2)))"),
        &[("E0300", "arg.repeat(2)")],
    );
}

#[test]
fn the_chars_of_untrusted_text_are_untrusted() {
    assert_errors(
        &with_arg("List<String>", "    Some(arg.chars())"),
        &[("E0300", "arg.chars()")],
    );
}

#[test]
fn joining_with_an_untrusted_separator_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(arg.join(ok.split(\",\"))))"),
        &[("E0300", "arg.join(ok.split(\",\"))")],
    );
}

// ---- Trust propagates: through the arguments -------------------------------

#[test]
fn replacing_with_untrusted_text_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(ok.replace(\"a\", arg)))"),
        &[("E0300", "ok.replace(\"a\", arg)")],
    );
    assert_errors(
        &with_arg("String", "    Some(greet(ok.replace(arg, \"b\")))"),
        &[("E0300", "ok.replace(arg, \"b\")")],
    );
}

#[test]
fn joining_untrusted_parts_is_untrusted() {
    assert_errors(
        &with_arg("String", "    Some(greet(\",\".join(arg.split(\"/\"))))"),
        &[("E0300", "\",\".join(arg.split(\"/\"))")],
    );
}

#[test]
fn joining_a_list_that_holds_untrusted_text_is_untrusted() {
    assert_errors(
        "fn greet(name: String) -> String {\n    name\n}\n\
         fn f(parts: List<Untrusted<String>>) -> String {\n    greet(\",\".join(parts))\n}",
        &[("E0300", "\",\".join(parts)")],
    );
}

// ---- Trusted inputs stay trusted -------------------------------------------

#[test]
fn text_methods_on_trusted_text_return_trusted_text() {
    assert_ok(
        "fn greet(name: String) -> String {\n    name\n}\n\
         fn f(s: String) -> String {\n    \
         let a = greet(s.replace(\"a\", \"b\"))\n    \
         let b = greet(s.slice(0, 1))\n    \
         let c = greet(s.repeat(2))\n    \
         greet(\",\".join(s.chars()))\n}",
    );
}

#[test]
fn the_predicates_of_untrusted_text_are_trusted_bools() {
    assert_ok(
        "fn f(args: Args) -> Option<Bool> {\n    let arg = args.get(1)?\n    \
         let a: Bool = arg.starts_with(\"x\")\n    \
         let b: Bool = arg.ends_with(\"y\")\n    \
         Some(a && b)\n}",
    );
}

#[test]
fn an_untrusted_argument_does_not_taint_a_predicate() {
    assert_ok(
        "fn f(args: Args, s: String) -> Option<Bool> {\n    let arg = args.get(1)?\n    \
         let a: Bool = s.starts_with(arg)\n    Some(a || s.ends_with(arg))\n}",
    );
}

// ---- FsRead.list_dir ---------------------------------------------------

#[test]
fn list_dir_returns_untrusted_names() {
    assert_ok(
        "fn names(fs: FsRead) -> Result<List<Untrusted<String>>, IoError> {\n    fs.list_dir(Path.new(\".\"))\n}",
    );
}

#[test]
fn the_names_list_dir_returns_are_untrusted() {
    assert_errors(
        "fn greet(name: String) -> String {\n    name\n}\n\
         fn first(fs: FsRead) -> Result<String, IoError> {\n    \
         let names = fs.list_dir(Path.new(\".\"))?\n    \
         Ok(greet(names.at(0)))\n}",
        &[("E0300", "names.at(0)")],
    );
}

#[test]
fn a_write_only_filesystem_cannot_list() {
    assert_errors(
        "fn f(fs: FsWrite) -> Result<List<Untrusted<String>>, IoError> {\n    fs.list_dir(Path.new(\".\"))\n}",
        &[("E0228", "list_dir")],
    );
}
