//! Trust on text (#50): text from outside the program is untrusted, and
//! untrusted text never flows into a trusted `String` slot (`E0300`).

mod common;

use common::{assert_errors, assert_ok};

#[test]
fn untrusted_text_cannot_be_passed_to_a_string_parameter() {
    assert_errors(
        "fn greet(name: String) -> String {\n    name\n}\n\
         fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    Some(greet(arg))\n}",
        &[("E0300", "arg")],
    );
}

#[test]
fn untrusted_text_cannot_fill_a_string_field() {
    assert_errors(
        "struct User { name: String }\n\
         fn f(args: Args) -> Option<User> {\n    let arg = args.get(1)?\n    Some(User(name: arg))\n}",
        &[("E0300", "arg")],
    );
}

#[test]
fn untrusted_text_cannot_be_returned_as_a_string() {
    assert_errors(
        "fn f(env: Env) -> String {\n    match env.get(\"HOME\") {\n        Some(home) => home\n        None => \"/\"\n    }\n}",
        &[("E0300", "home")],
    );
}

#[test]
fn untrusted_text_cannot_be_returned_inside_a_trusted_container() {
    assert_errors(
        "fn f(args: Args) -> Option<String> {\n    args.get(1)\n}",
        &[("E0300", "args.get(1)")],
    );
}

#[test]
fn file_contents_are_untrusted() {
    assert_errors(
        "fn f(fs: FsRead, path: Path) -> Result<String, IoError> {\n    let text = fs.read_to_string(path)?\n    Ok(text)\n}",
        &[("E0300", "text")],
    );
}

#[test]
fn a_line_from_stdin_is_untrusted() {
    assert_errors(
        "fn f(input: Stdin) -> Result<Option<String>, IoError> {\n    input.read_line()\n}",
        &[("E0300", "input.read_line()")],
    );
}

#[test]
fn trusted_text_is_accepted_where_untrusted_text_is_expected() {
    assert_ok(
        "fn show(raw: Untrusted<String>) -> Untrusted<String> {\n    raw\n}\n\
         fn f() -> Untrusted<String> {\n    show(\"literal\")\n}",
    );
}

#[test]
fn untrusted_text_is_accepted_by_an_untrusted_parameter() {
    assert_ok(
        "fn count(raw: Untrusted<String>) -> Int {\n    1\n}\n\
         fn f(args: Args) -> Option<Int> {\n    let arg = args.get(1)?\n    Some(count(arg))\n}",
    );
}

#[test]
fn println_and_path_from_accept_untrusted_text() {
    assert_ok(
        "fn f(io: Stdout, fs: FsRead, args: Args) -> Result<(), IoError> {\n    \
         let arg = args.get(1).ok_or(IoError(message: \"usage\"))?\n    \
         io.println(arg)?\n    \
         let _ = fs.read_to_string(Path.from(arg))?\n    \
         Ok(())\n}",
    );
}
