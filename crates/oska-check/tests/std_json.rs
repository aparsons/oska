// SPDX-License-Identifier: Apache-2.0
//! `std.json` (#248): a `std` module written in Oska, checked with each
//! program that imports it. A parsed string is untrusted, so it is refused
//! in every structural position and accepted in data positions; the
//! module's names follow the rules of the other `std` modules; and its
//! code is the toolchain's, so the authority report leaves it out.

mod common;

use common::{assert_errors, assert_ok, check, check_modules};

/// A program that parses its first argument and binds `name`, a string
/// field of it, before `body`.
fn with_field(body: &str) -> String {
    format!(
        "use std.json\n\n\
         fn main(io: Stdout, args: Args) -> Result<(), IoError> {{\n    \
         let raw = args.get(1).unwrap_or(\"null\")\n    \
         let value = match json.parse(raw) {{\n        \
         Ok(value) => value\n        \
         Err(_) => json.Value.Null\n    }}\n    \
         let name = value.text(\"name\").unwrap_or(\"\")\n\
         {body}    Ok(())\n}}\n"
    )
}

#[test]
fn a_parsed_string_is_refused_where_trusted_text_is_required() {
    let src = format!(
        "{}\nfn greet(name: String) -> String {{\n    name\n}}\n",
        with_field("    let _ = greet(name)\n")
    );
    assert_errors(&src, &[("E0300", "name")]);
}

#[test]
fn a_parsed_string_is_refused_as_a_sink_literal() {
    let src = "use std.json\nuse std.sql\n\n\
               fn lookup(db: sql.Db, raw: Untrusted<String>) {\n    \
               let value = json.parse(raw).unwrap_or(json.Value.Null)\n    \
               let query = value.text(\"query\").unwrap_or(\"\")\n    \
               let _ = db.query(query, [])\n}\n";
    let report = check(src);
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0341"], "{:#?}", report.diagnostics);
}

/// A function that parses `raw` and binds `name`, a string field of it,
/// before `body`, with `params` after `raw`.
fn field_in(params: &str, body: &str) -> String {
    format!(
        "fn use_it(raw: Untrusted<String>{params}) {{\n    \
         let value = json.parse(raw).unwrap_or(json.Value.Null)\n    \
         let name = value.text(\"name\").unwrap_or(\"\")\n\
         {body}}}\n"
    )
}

#[test]
fn a_parsed_string_is_refused_in_every_structural_position() {
    let cases = [
        (
            "use std.proc",
            ", spawn: Spawn",
            "    let _ = proc.run(spawn, name, [])\n",
        ),
        (
            "use std.html",
            "",
            "    let _ = html.TrustedHtml.new(name)\n",
        ),
        (
            "use std.sql",
            ", net: NetConnect",
            "    let _ = sql.connect(net, name)\n",
        ),
        (
            "",
            ", net: NetConnect",
            "    let _ = net.connect(name, 443)\n",
        ),
        ("", ", fs: FsRead", "    let _ = fs.restrict_to(name)\n"),
        (
            "",
            ", fs: FsRead",
            "    let _ = fs.restrict_to_file(name)\n",
        ),
        ("", ", fs: FsWrite", "    let _ = fs.restrict_to(name)\n"),
        (
            "",
            ", fs: FsWrite",
            "    let _ = fs.restrict_to_file(name)\n",
        ),
    ];
    for (import, params, body) in cases {
        let src = format!("use std.json\n{import}\n\n{}", field_in(params, body));
        assert_errors(&src, &[("E0300", "name")]);
    }
}

#[test]
fn a_parsed_string_is_refused_as_a_path_root() {
    let src = with_field("    let _ = Path.new(name)\n");
    assert_errors(&src, &[("E0300", "name")]);
}

#[test]
fn a_parsed_string_is_accepted_in_data_positions() {
    assert_ok(
        &with_field(
            "    io.println(name)?\n    \
         let _ = Path.from(name)\n    \
         let _ = html.text(name)\n    \
         let _ = json.encode(json.Value.Text(name))\n",
        )
        .replace("use std.json\n", "use std.json\nuse std.html\n"),
    );
}

#[test]
fn numbers_and_booleans_from_a_parsed_value_are_trusted() {
    assert_ok(&with_field(
        "    let n = value.int(\"count\").unwrap_or(0)\n    \
         let flag = value.bool(\"on\").unwrap_or(false)\n    \
         let label: String = \"{n}\"\n    let _ = flag && true\n    \
         io.println(label)?\n",
    ));
}

#[test]
fn encoding_gives_untrusted_text() {
    let src = format!(
        "{}\nfn keep(text: String) -> String {{\n    text\n}}\n",
        with_field("    let _ = keep(json.encode(json.Value.Null))\n")
    );
    assert_errors(&src, &[("E0300", "json.encode(json.Value.Null)")]);
}

#[test]
fn an_unqualified_name_needs_the_module_qualifier() {
    let src = "use std.json\n\nfn f() -> Value {\n    json.Value.Null\n}\n";
    assert_errors(src, &[("E0340", "Value")]);
}

#[test]
fn a_brace_import_names_only_exports() {
    assert_ok("use std.json.{Value, parse}\n\nfn f() -> Value {\n    Value.Null\n}\n");
    let report = check("use std.json.{Parser}\n\nfn f() {}\n");
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
}

#[test]
fn the_module_s_private_items_and_fields_stay_private() {
    let report = check(
        "use std.json\n\nfn f() -> Int {\n    json.compare(\"\".to_bytes(), \"\".to_bytes())\n}\n",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    let report = check("use std.json\n\nfn f(n: json.Number) -> String {\n    n.text\n}\n");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("private to `std.json`")),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn without_the_use_its_names_are_unknown() {
    let report = check("fn f() {\n    let _ = json.parse(\"1\")\n}\n");
    assert!(!report.diagnostics.is_empty());
}

#[test]
fn a_message_names_the_module_s_types_as_code_writes_them() {
    let report = check("use std.json\n\nfn f() -> Int {\n    json.Value.Null\n}\n");
    let messages: Vec<&str> = report
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("json.Value") && !m.contains("pkg.")),
        "{messages:#?}"
    );
}

#[test]
fn the_authority_report_leaves_the_module_s_code_out() {
    let report = check(&with_field("    io.println(name)?\n"));
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["main"]);
}

#[test]
fn a_package_s_modules_each_import_it() {
    let report = check_modules(&[
        (
            "main",
            "use std.json\nuse pkg.wire\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    \
             io.println(json.encode(wire.ping()))\n}\n",
        ),
        (
            "wire",
            "use std.json\n\npub fn ping() -> json.Value {\n    \
             json.Value.Object([json.Member(key: \"ping\", value: json.Value.Bool(true))])\n}\n",
        ),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["pkg.main.main", "pkg.wire.ping"]);
}

#[test]
fn a_package_cannot_reach_it_through_a_pkg_path() {
    let report = check_modules(&[
        ("main", "use pkg.std.json\n\nfn main() {}\n"),
        ("util", "pub fn f() {}\n"),
    ]);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "E0204" || d.message.contains("unknown module `pkg.std.json`")),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn a_single_file_keeps_refusing_pkg_paths() {
    let report = check("use std.json\nuse pkg.util\n\nfn main() {}\n");
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    let alone = check("use pkg.util\n\nfn main() {}\n");
    let alone: Vec<&str> = alone.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, alone);
}

/// `std.json`'s own source, checked as a program's module.
const JSON_SOURCE: &str = include_str!("../src/std/json.osk");

#[test]
fn the_module_holds_one_audited_trust_assertion_and_no_authority() {
    // The authority and audit reports leave a `std` module written in
    // Oska out, so this checks it as code of its own: it must reach no
    // capability, and vouch for text in one place only.
    let report = check(JSON_SOURCE);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let reaching: Vec<&str> = report
        .functions
        .iter()
        .filter(|f| !f.pure || !f.authority.is_empty())
        .map(|f| f.name.as_str())
        .collect();
    assert!(
        reaching.is_empty(),
        "functions that reach authority: {reaching:?}"
    );
    let sites: Vec<&str> = report
        .audit
        .iter()
        .map(|site| &JSON_SOURCE[site.span.start..site.span.end])
        .collect();
    assert_eq!(sites, ["checked.assume_trusted()"]);
}
