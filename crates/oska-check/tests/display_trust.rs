//! Turning a value that holds untrusted text into text (review fix F1 of
//! #93): `IoError`, `SqlError` and `ProcError` keep their `message` as
//! untrusted text, so their text form is untrusted too, by every route.
//! Before the fix, `IoError(message: arg).to_string()` was trusted text,
//! and untrusted command-line text could pick a directory capability's
//! root with no audit site.

mod common;

use common::{assert_errors, assert_ok};

/// `main` with an untrusted argument `a`, then `body`.
fn with_arg(uses: &str, body: &str) -> String {
    format!(
        "{uses}fn main(fs: Fs, args: Args) {{\n    \
         let a = args.get(1).unwrap_or(\"x\")\n{body}\n}}"
    )
}

#[test]
fn an_io_errors_text_cannot_pick_a_root() {
    // The reviewer's exploit: it read /etc/hostname from the command line.
    let src = with_arg(
        "",
        "    let e = IoError(message: a)\n    let d = fs.read_only().restrict_to(e.to_string())",
    );
    assert_errors(&src, &[("E0300", "e.to_string()")]);
}

#[test]
fn an_io_error_called_qualified_by_its_type_is_untrusted_text() {
    let src = with_arg(
        "",
        "    let e = IoError(message: a)\n    let d = fs.restrict_to(IoError.to_string(e))",
    );
    assert_errors(&src, &[("E0300", "IoError.to_string(e)")]);
}

#[test]
fn an_io_error_called_qualified_by_the_trait_is_untrusted_text() {
    let src = with_arg(
        "",
        "    let e = IoError(message: a)\n    let d = fs.restrict_to(Display.to_string(e))",
    );
    assert_errors(&src, &[("E0300", "Display.to_string(e)")]);
}

#[test]
fn an_io_error_interpolated_is_untrusted_text() {
    let src = with_arg(
        "",
        "    let e = IoError(message: a)\n    let d = fs.restrict_to(\"{e}\")",
    );
    assert_errors(&src, &[("E0300", "\"{e}\"")]);
}

#[test]
fn a_sql_errors_text_is_untrusted() {
    let src = with_arg(
        "use std.sql\n",
        "    let e = sql.SqlError(message: a)\n    let d = fs.restrict_to(e.to_string())",
    );
    assert_errors(&src, &[("E0300", "e.to_string()")]);
}

#[test]
fn a_proc_errors_text_is_untrusted() {
    let src = with_arg(
        "use std.proc\n",
        "    let e = proc.ProcError(message: a)\n    let d = fs.restrict_to(e.to_string())",
    );
    assert_errors(&src, &[("E0300", "e.to_string()")]);
}

#[test]
fn a_user_struct_holding_an_io_error_is_untrusted_text_when_interpolated() {
    let src = format!(
        "struct Failure {{ cause: IoError }}\n\
         impl Display for Failure {{\n    \
         fn to_string(self) -> String {{ \"failed\" }}\n}}\n{}",
        with_arg(
            "",
            "    let f = Failure(cause: IoError(message: a))\n    let d = fs.restrict_to(\"{f}\")"
        )
    );
    assert_errors(&src, &[("E0300", "\"{f}\"")]);
}

#[test]
fn a_user_display_cannot_return_an_io_errors_text_as_trusted() {
    assert_errors(
        "struct Failure { cause: IoError }\n\
         impl Display for Failure {\n    \
         fn to_string(self) -> String { self.cause.to_string() }\n}",
        &[("E0300", "self.cause.to_string()")],
    );
}

#[test]
fn an_io_errors_text_is_still_printable() {
    assert_ok(
        "fn main(io: Stdout, args: Args) -> Result<(), IoError> {\n    \
         let a = args.get(1).unwrap_or(\"x\")\n    \
         let e = IoError(message: a)\n    \
         io.println(e.to_string())?\n    \
         io.println(\"failed: {e}\")\n}",
    );
}

#[test]
fn an_io_errors_text_can_be_vouched_for_at_an_audit_site() {
    assert_ok(&with_arg(
        "",
        "    let e = IoError(message: a)\n    \
         let d = fs.restrict_to(e.to_string().assume_trusted())",
    ));
}

#[test]
fn an_int_still_displays_as_trusted_text() {
    assert_ok("fn f(n: Int) -> String {\n    let s = \"{n}\"\n    n.to_string()\n}");
}
