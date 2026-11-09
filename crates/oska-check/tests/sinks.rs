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

// ---- SQL ----------------------------------------------------------------------------

/// A function with a database, untrusted text `id`, and trusted text
/// `text`, whose body is `body`.
fn with_db(body: &str) -> String {
    format!(
        "use std.sql\n\
         fn f(db: sql.Db, id: Untrusted<String>, text: String) -> Result<List<sql.Row>, sql.SqlError> {{\n    \
         {body}\n}}"
    )
}

#[test]
fn untrusted_values_are_accepted_as_sql_parameters() {
    assert_ok(&with_db(
        "db.query(sql\"SELECT * FROM users WHERE id = ?\", [id, \"admin\"])",
    ));
}

#[test]
fn untrusted_text_is_rejected_as_sql_query_text() {
    assert_errors(&with_db("db.query(id, [])"), &[("E0341", "id")]);
}

#[test]
fn trusted_text_is_rejected_as_sql_query_text() {
    assert_errors(&with_db("db.query(text, [])"), &[("E0341", "text")]);
}

#[test]
fn a_string_literal_is_rejected_as_sql_query_text() {
    assert_errors(
        &with_db("db.query(\"SELECT 1\", [])"),
        &[("E0341", "\"SELECT 1\"")],
    );
}

#[test]
fn an_interpolated_string_is_rejected_as_sql_query_text() {
    assert_errors(
        &with_db("db.query(\"SELECT * FROM users WHERE id = {id}\", [])"),
        &[("E0341", "\"SELECT * FROM users WHERE id = {id}\"")],
    );
}

#[test]
fn a_query_text_annotation_rejects_a_string() {
    assert_errors(
        "use std.sql\nfn f() {\n    let q: sql.SqlLiteral = \"SELECT 1\"\n}",
        &[("E0341", "\"SELECT 1\"")],
    );
}

#[test]
fn query_text_cannot_be_constructed_outside_a_literal() {
    assert_errors(
        "use std.sql\nfn f(text: String) -> sql.SqlLiteral {\n    sql.SqlLiteral(text: text)\n}",
        &[("E0271", "SqlLiteral")],
    );
}

#[test]
fn the_database_url_rejects_untrusted_text() {
    assert_errors(
        "use std.sql\n\
         fn f(net: NetConnect, url: Untrusted<String>) -> Result<sql.Db, sql.SqlError> {\n    \
         sql.connect(net, url)\n}",
        &[("E0300", "url")],
    );
}

// ---- Processes ----------------------------------------------------------------------

#[test]
fn untrusted_text_is_accepted_as_process_arguments() {
    assert_ok(
        "use std.proc\n\
         fn f(spawn: Spawn, branch: Untrusted<String>) -> Result<proc.Output, proc.ProcError> {\n    \
         proc.run(spawn, \"git\", [\"log\", branch])\n}",
    );
}

#[test]
fn untrusted_text_is_rejected_as_the_program_name() {
    assert_errors(
        "use std.proc\n\
         fn f(spawn: Spawn, program: Untrusted<String>) -> Result<proc.Output, proc.ProcError> {\n    \
         proc.run(spawn, program, [])\n}",
        &[("E0300", "program")],
    );
}

#[test]
fn a_process_output_is_untrusted() {
    assert_errors(
        "use std.proc\nfn f(out: proc.Output) -> String {\n    out.stdout()\n}",
        &[("E0300", "out.stdout()")],
    );
}

// ---- HTML ---------------------------------------------------------------------------

#[test]
fn untrusted_text_is_accepted_as_escaped_html_text() {
    assert_ok(
        "use std.html\nfn f(name: Untrusted<String>) -> html.Html {\n    \
         html.Html.raw(html.TrustedHtml.new(\"<h1>\")).concat(html.text(name))\n}",
    );
}

#[test]
fn untrusted_text_is_rejected_as_trusted_html() {
    assert_errors(
        "use std.html\nfn f(name: Untrusted<String>) -> html.TrustedHtml {\n    \
         html.TrustedHtml.new(name)\n}",
        &[("E0300", "name")],
    );
}

#[test]
fn html_cannot_be_constructed_from_a_string() {
    assert_errors(
        "use std.html\nfn f(markup: String) -> html.Html {\n    html.Html(markup: markup)\n}",
        &[("E0271", "Html")],
    );
}

// ---- Paths --------------------------------------------------------------------------

#[test]
fn untrusted_text_is_accepted_as_a_path_under_a_restricted_capability() {
    assert_ok(
        "fn f(fs: Fs, name: Untrusted<String>) -> Result<Untrusted<String>, IoError> {\n    \
         fs.restrict_to(\"/srv/files\").read_only().read_to_string(Path.from(name))\n}",
    );
}

#[test]
fn untrusted_text_is_rejected_as_the_restricted_directory() {
    assert_errors(
        "fn f(fs: Fs, dir: Untrusted<String>) -> Fs {\n    fs.restrict_to(dir)\n}",
        &[("E0300", "dir")],
    );
}

#[test]
fn untrusted_text_is_rejected_as_the_restricted_directory_of_a_read_capability() {
    assert_errors(
        "fn f(fs: FsRead, dir: Untrusted<String>) -> FsRead {\n    fs.restrict_to(dir)\n}",
        &[("E0300", "dir")],
    );
}

// ---- Network ------------------------------------------------------------------------

#[test]
fn untrusted_text_is_accepted_as_a_request_body() {
    assert_ok(
        "fn f(net: NetConnect, body: Untrusted<String>) -> Result<(), IoError> {\n    \
         let conn = net.connect(\"api.example.com\", 443)?\n    conn.send(body)\n}",
    );
}

#[test]
fn untrusted_text_is_rejected_as_the_host() {
    assert_errors(
        "fn f(net: NetConnect, host: Untrusted<String>) -> Result<Connection, IoError> {\n    \
         net.connect(host, 443)\n}",
        &[("E0300", "host")],
    );
}

#[test]
fn a_response_is_untrusted() {
    assert_errors(
        "fn f(conn: Connection) -> Result<String, IoError> {\n    conn.receive()\n}",
        &[("E0300", "conn.receive()")],
    );
}
