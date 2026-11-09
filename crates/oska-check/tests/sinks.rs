//! Sink modules and structural positions (#54): `use std.x` binds the
//! module, reached through qualified paths such as `sql.Db`; the sink
//! modules `std.sql`, `std.proc`, and `std.html`; and the structural
//! positions that reject untrusted text.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- `use std.x` binds the module ---------------------------------------------------

#[test]
fn a_module_type_is_reached_through_the_module_name() {
    assert_ok("use std.sql\nfn f(q: sql.SqlLiteral) { }");
}

#[test]
fn a_module_type_is_not_in_scope_unqualified_after_a_plain_use() {
    let src = "use std.sql\nfn f(q: SqlLiteral) { }";
    assert_errors(src, &[("E0340", "SqlLiteral")]);
    assert_fix(src, "E0340", "use std.sql\nfn f(q: sql.SqlLiteral) { }");
}

#[test]
fn a_brace_group_imports_names_unqualified() {
    assert_ok("use std.sql.{SqlLiteral}\nfn f(q: SqlLiteral) { }");
}

#[test]
fn a_brace_group_does_not_bind_the_module_name() {
    assert_errors(
        "use std.sql.{SqlLiteral}\nfn f(q: sql.SqlLiteral) { }",
        &[("E0200", "sql.SqlLiteral")],
    );
}

#[test]
fn a_qualified_name_the_module_does_not_export_suggests_the_closest_one() {
    let src = "use std.sql\nfn f(q: sql.SqlLitreal) { }";
    assert_errors(src, &[("E0312", "SqlLitreal")]);
    assert_fix(src, "E0312", "use std.sql\nfn f(q: sql.SqlLiteral) { }");
}

#[test]
fn a_qualified_type_works_in_a_let_annotation() {
    assert_ok("use std.sql\nfn f() {\n    let q: sql.SqlLiteral = sql\"SELECT 1\"\n}");
}

#[test]
fn a_module_function_is_called_through_the_module_name() {
    assert_ok("use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    html.text(name)\n}");
}

#[test]
fn a_module_function_is_not_in_scope_unqualified_after_a_plain_use() {
    let src = "use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    text(name)\n}";
    assert_errors(src, &[("E0340", "text")]);
    assert_fix(
        src,
        "E0340",
        "use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    html.text(name)\n}",
    );
}

#[test]
fn a_brace_group_imports_a_function_unqualified() {
    assert_ok(
        "use std.html.{Html, text}\nfn f(name: Untrusted<String>) -> Html {\n    text(name)\n}",
    );
}

#[test]
fn a_module_type_member_is_reached_through_the_module_name() {
    assert_ok(
        "use std.html\nfn f() -> html.TrustedHtml {\n    html.TrustedHtml.new(\"<b>hi</b>\")\n}",
    );
}

#[test]
fn a_module_type_member_is_not_in_scope_unqualified_after_a_plain_use() {
    assert_errors(
        "use std.html\nfn f() -> html.TrustedHtml {\n    TrustedHtml.new(\"<b>hi</b>\")\n}",
        &[("E0340", "TrustedHtml")],
    );
}

#[test]
fn a_qualified_function_the_module_does_not_export_suggests_the_closest_one() {
    let src = "use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    html.txt(name)\n}";
    assert_errors(src, &[("E0312", "txt")]);
    assert_fix(
        src,
        "E0312",
        "use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    html.text(name)\n}",
    );
}

#[test]
fn a_local_shadows_a_module_name() {
    assert_ok("use std.html\nfn f(html: String) -> Int {\n    html.len()\n}");
}
