//! Modules of a package use each other (spec #115, #118): `use pkg.a.b`
//! binds `b`, a brace group imports names, items are private unless
//! `pub(pkg)` or `pub`, and the checked program spans every module.

mod common;

use common::{
    assert_modules_errors, assert_modules_fix, assert_modules_fix_in, assert_modules_ok,
    check_modules,
};

#[test]
fn a_module_import_binds_its_last_segment_as_a_qualifier() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util.text\n\nfn main() {}\n\nfn f() -> String {\n    text.shout(\"hi\")\n}\n",
        ),
        (
            "util.text",
            "pub(pkg) fn shout(s: String) -> String {\n    \"{s}!\"\n}\n",
        ),
    ]);
}

#[test]
fn a_clean_package_of_several_modules_carries_one_checked_program() {
    let report = check_modules(&[
        (
            "main",
            "use pkg.util\n\nfn main() {}\n\nfn f() -> Int {\n    util.one()\n}\n",
        ),
        ("util", "pub(pkg) fn one() -> Int {\n    1\n}\n"),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let program = report.program.expect("a checked program");
    let names: Vec<&str> = program.functions.keys().map(String::as_str).collect();
    assert_eq!(names, ["f", "main", "util.one"]);
}

#[test]
fn a_brace_group_imports_names_unqualified() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.geo.{Point, origin}\n\nfn main() {}\n\n\
             fn f() -> Point {\n    origin()\n}\n",
        ),
        (
            "geo",
            "pub(pkg) struct Point {\n    pub x: Int\n}\n\n\
             pub(pkg) fn origin() -> Point {\n    Point(x: 0)\n}\n",
        ),
    ]);
}

#[test]
fn structs_enums_traits_functions_and_constants_work_across_modules() {
    let shapes = "pub(pkg) enum Shape {\n    Circle(Int),\n    Square(Int),\n}\n\n\
                  pub(pkg) struct Size {\n    pub n: Int\n}\n\n\
                  pub(pkg) trait Area {\n    fn area(self) -> Int\n}\n\n\
                  pub(pkg) const UNIT: Int = 1\n\n\
                  pub fn unit_circle() -> Shape {\n    Shape.Circle(1)\n}\n";
    let main = "use pkg.geo.shapes\nuse pkg.geo.shapes.{Area, UNIT}\n\nfn main() {}\n\n\
                impl Area for shapes.Shape {\n    fn area(self) -> Int {\n        \
                match self {\n            shapes.Shape.Circle(r) => 3 * r * r\n            \
                shapes.Shape.Square(s) => s * s\n        }\n    }\n}\n\n\
                fn total<T: Area>(xs: List<T>) -> Int {\n    var sum = 0\n    \
                for x in xs {\n        sum = sum + x.area()\n    }\n    sum\n}\n\n\
                fn f() -> Int {\n    let size = shapes.Size(n: 2)\n    \
                total([shapes.unit_circle(), shapes.Shape.Square(size.n)])\n}\n";
    assert_modules_ok(&[("main", main), ("geo.shapes", shapes)]);
}

#[test]
fn items_are_private_by_default() {
    assert_modules_errors(
        &[
            (
                "main",
                "use pkg.util\nuse pkg.util.{Token}\n\nfn main() {}\n\n\
                 fn f() -> Int {\n    util.helper()\n}\n",
            ),
            (
                "util",
                "fn helper() -> Int {\n    1\n}\n\nstruct Token {\n    n: Int\n}\n",
            ),
        ],
        &[
            ("src/main.osk", "E0206", "Token"),
            ("src/main.osk", "E0206", "helper"),
        ],
    );
}

#[test]
fn private_types_traits_enums_and_constants_cannot_be_imported() {
    assert_modules_errors(
        &[
            (
                "main",
                "use pkg.util.{Kind, Show, LIMIT}\n\nfn main() {}\n\nfn f(t: util2.Token) {}\n",
            ),
            (
                "util",
                "enum Kind {\n    A,\n}\n\ntrait Show {}\n\nconst LIMIT: Int = 3\n",
            ),
        ],
        &[
            ("src/main.osk", "E0206", "Kind"),
            ("src/main.osk", "E0206", "Show"),
            ("src/main.osk", "E0206", "LIMIT"),
            ("src/main.osk", "E0200", "util2.Token"),
        ],
    );
}

#[test]
fn using_a_private_item_has_a_fix_that_marks_its_declaration_pub_pkg() {
    let modules = [
        (
            "main",
            "use pkg.util\n\nfn main() {}\n\nfn f() -> Int {\n    util.helper()\n}\n",
        ),
        (
            "util",
            "// A helper.\nfn helper() -> Int {\n    1\n}\n\n#[integration]\ntest \"t\" {}\n",
        ),
    ];
    assert_modules_fix_in(
        &modules,
        "E0206",
        "src/util.osk",
        "// A helper.\npub(pkg) fn helper() -> Int {\n    1\n}\n\n#[integration]\ntest \"t\" {}\n",
    );
}

#[test]
fn the_pub_pkg_fix_goes_after_an_item_s_attributes_and_before_cap() {
    assert_modules_fix_in(
        &[
            ("main", "use pkg.log.{Logger}\n\nfn main() {}\n"),
            ("log", "cap struct Logger {\n    out: Stdout\n}\n"),
        ],
        "E0206",
        "src/log.osk",
        "pub(pkg) cap struct Logger {\n    out: Stdout\n}\n",
    );
}

#[test]
fn pub_and_pub_pkg_items_are_both_visible_to_other_modules() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util\n\nfn main() {}\n\nfn f() -> Int {\n    util.a() + util.b()\n}\n",
        ),
        (
            "util",
            "pub fn a() -> Int {\n    1\n}\n\npub(pkg) fn b() -> Int {\n    2\n}\n",
        ),
    ]);
}

#[test]
fn an_item_is_visible_in_its_own_module_without_any_marker() {
    assert_modules_ok(&[
        ("main", "use pkg.util\n\nfn main() {}\n\nfn f() -> Int {\n    util.a()\n}\n"),
        (
            "util",
            "pub(pkg) fn a() -> Int {\n    helper()\n}\n\nfn helper() -> Int {\n    1\n}\n",
        ),
    ]);
}

#[test]
fn an_unknown_module_path_suggests_the_closest_module() {
    let modules = [
        ("main", "use pkg.util.txt\n\nfn main() {}\n"),
        ("util.text", "pub(pkg) fn f() {}\n"),
    ];
    assert_modules_errors(&modules, &[("src/main.osk", "E0204", "pkg.util.txt")]);
    assert_modules_fix(
        &modules,
        "src/main.osk",
        "E0204",
        "use pkg.util.text\n\nfn main() {}\n",
    );
}

#[test]
fn an_unknown_name_in_a_brace_group_suggests_the_closest_item() {
    let modules = [
        ("main", "use pkg.util.{shuot}\n\nfn main() {}\n"),
        (
            "util",
            "pub(pkg) fn shout() {}\n\npub(pkg) fn whisper() {}\n",
        ),
    ];
    assert_modules_errors(&modules, &[("src/main.osk", "E0205", "shuot")]);
    assert_modules_fix(
        &modules,
        "src/main.osk",
        "E0205",
        "use pkg.util.{shout}\n\nfn main() {}\n",
    );
}

#[test]
fn an_unknown_qualified_name_is_reported_once() {
    assert_modules_errors(
        &[
            (
                "main",
                "use pkg.util\n\nfn main() {}\n\nfn f() -> util.Thing {\n    util.make()\n}\n",
            ),
            ("util", "pub(pkg) fn mk() -> Int {\n    1\n}\n"),
        ],
        &[
            ("src/main.osk", "E0205", "Thing"),
            ("src/main.osk", "E0205", "make"),
        ],
    );
}

#[test]
fn an_import_cycle_is_reported_once_naming_every_module_in_it() {
    let report = check_modules(&[
        ("main", "use pkg.a\n\nfn main() {}\n"),
        ("a", "use pkg.b\n\npub(pkg) fn f() {}\n"),
        ("b", "use pkg.c\n\npub(pkg) fn g() {}\n"),
        ("c", "use pkg.a\n\npub(pkg) fn h() {}\n"),
    ]);
    let cycles: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0207")
        .collect();
    assert_eq!(cycles.len(), 1, "{:#?}", report.diagnostics);
    for module in ["`pkg.a`", "`pkg.b`", "`pkg.c`"] {
        assert!(cycles[0].message.contains(module), "{}", cycles[0].message);
    }
    assert!(!cycles[0].message.contains("`pkg.main`"));
}

#[test]
fn a_module_that_imports_itself_is_a_cycle() {
    assert_modules_errors(
        &[
            ("main", "fn main() {}\n"),
            ("a", "use pkg.a\n\npub(pkg) fn f() {}\n"),
        ],
        &[("src/a.osk", "E0207", "pkg.a")],
    );
}

#[test]
fn two_imports_cannot_bind_the_same_qualifier() {
    assert_modules_errors(
        &[
            ("main", "use pkg.a.util\nuse pkg.b.util\n\nfn main() {}\n"),
            ("a.util", "pub(pkg) fn f() {}\n"),
            ("b.util", "pub(pkg) fn g() {}\n"),
        ],
        &[("src/main.osk", "E0208", "util")],
    );
}

#[test]
fn a_qualifier_cannot_collide_with_an_item_of_the_module() {
    assert_modules_errors(
        &[
            ("main", "use pkg.util\n\nfn main() {}\n\nfn util() {}\n"),
            ("util", "pub(pkg) fn f() {}\n"),
        ],
        &[("src/main.osk", "E0208", "util")],
    );
}

#[test]
fn a_pkg_qualifier_cannot_collide_with_a_std_one() {
    assert_modules_errors(
        &[
            ("main", "use std.sql\nuse pkg.db.sql\n\nfn main() {}\n"),
            ("db.sql", "pub(pkg) fn f() {}\n"),
        ],
        &[("src/main.osk", "E0208", "sql")],
    );
}

#[test]
fn a_local_shadows_a_qualifier_and_an_item() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util\n\nfn main() {}\n\nfn f(util: Int) -> Int {\n    util\n}\n\n\
             fn g() -> Int {\n    let helper = 2\n    helper\n}\n",
        ),
        (
            "util",
            "pub(pkg) fn f() {}\n\nfn helper() -> Int {\n    1\n}\n",
        ),
    ]);
}

#[test]
fn the_entry_module_s_items_are_not_in_scope_in_other_modules() {
    assert_modules_errors(
        &[
            (
                "main",
                "use pkg.util\n\nfn main() {}\n\nfn helper() -> Int {\n    1\n}\n",
            ),
            ("util", "pub(pkg) fn f() -> Int {\n    helper()\n}\n"),
        ],
        &[("src/util.osk", "E0221", "helper")],
    );
}

#[test]
fn a_module_sees_another_module_s_items_only_through_an_import() {
    assert_modules_errors(
        &[
            ("main", "fn main() {}\n\nfn f() -> Int {\n    util.one()\n}\n"),
            ("util", "pub(pkg) fn one() -> Int {\n    1\n}\n"),
        ],
        &[("src/main.osk", "E0221", "util")],
    );
}

#[test]
fn a_std_import_is_per_module() {
    assert_modules_errors(
        &[
            (
                "main",
                "use std.sql\nuse pkg.db\n\nfn main() {}\n\nfn q() -> sql.SqlLiteral {\n    sql\"SELECT 1\"\n}\n",
            ),
            (
                "db",
                "pub(pkg) fn q() -> sql.SqlLiteral {\n    sql\"SELECT 1\"\n}\n",
            ),
        ],
        &[
            ("src/db.osk", "E0221", "sql"),
            ("src/db.osk", "E0310", "sql"),
        ],
    );
}

#[test]
fn a_sink_literal_s_fix_adds_the_use_to_its_own_module() {
    assert_modules_fix_in(
        &[
            ("main", "use std.sql\nuse pkg.db\n\nfn main() {}\n"),
            ("db", "pub(pkg) fn q() {\n    let s = sql\"SELECT 1\"\n}\n"),
        ],
        "E0310",
        "src/db.osk",
        "use std.sql\npub(pkg) fn q() {\n    let s = sql\"SELECT 1\"\n}\n",
    );
}

#[test]
fn a_std_brace_import_is_per_module() {
    assert_modules_errors(
        &[
            ("main", "use std.html.{text}\nuse pkg.page\n\nfn main() {}\n"),
            (
                "page",
                "use std.html\n\npub(pkg) fn f() -> html.Html {\n    text(\"hi\")\n}\n",
            ),
        ],
        &[("src/page.osk", "E0340", "text")],
    );
}

#[test]
fn a_module_cannot_declare_a_prelude_name() {
    assert_modules_errors(
        &[
            ("main", "use pkg.util\n\nfn main() {}\n"),
            ("util", "pub(pkg) struct Option {\n    n: Int\n}\n"),
        ],
        &[("src/util.osk", "E0202", "Option")],
    );
}

#[test]
fn a_generic_parameter_shadows_an_imported_type() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util.{T}\n\nfn main() {}\n\nfn id<T>(x: T) -> T {\n    x\n}\n",
        ),
        ("util", "pub(pkg) struct T {\n    n: Int\n}\n"),
    ]);
}

#[test]
fn two_modules_may_each_have_a_private_item_of_the_same_name() {
    let report = check_modules(&[
        (
            "main",
            "use pkg.util\n\nfn main() {}\n\nfn helper() -> String {\n    \"main\"\n}\n\n\
             fn f() -> Int {\n    util.count()\n}\n",
        ),
        (
            "util",
            "fn helper() -> Int {\n    1\n}\n\npub(pkg) fn count() -> Int {\n    helper()\n}\n",
        ),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn use_pkg_in_a_single_file_says_pkg_paths_need_a_package() {
    let report = oska_check::check_file(
        "prog.osk",
        "use pkg.util\n\nfn main() {}\n",
        oska_check::CheckOptions::default(),
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0203"]);
    assert!(
        report.diagnostics[0].message.contains("need a directory with an `oska.toml`"),
        "{}",
        report.diagnostics[0].message
    );
}

#[test]
fn a_package_of_one_module_may_not_import_a_module_it_lacks() {
    let package = oska_check::Package {
        manifest: Some("[package]\nname = \"p\"\nversion = \"0.1.0\"\n".to_string()),
        modules: vec![oska_check::ModuleSource::in_package(
            "main",
            "use pkg.util\n\nfn main() {}\n",
        )],
    };
    let report = oska_check::check_package(&package, oska_check::CheckOptions::default());
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0204"]);
}

#[test]
fn the_json_report_names_the_file_a_fix_edits_when_it_is_another() {
    let json = check_modules(&[
        ("main", "use pkg.util.{helper}\n\nfn main() {}\n"),
        ("util", "fn helper() {}\n"),
    ])
    .to_json();
    assert!(
        json.contains(
            r#""fix":{"message":"mark `helper` `pub(pkg)`","path":"src/util.osk","edits":[{"span":{"start":0,"end":0},"replacement":"pub(pkg) "}]}"#
        ),
        "{json}"
    );
}

#[test]
fn an_unknown_type_in_a_helper_module_is_reported_in_its_file() {
    assert_modules_errors(
        &[
            ("main", "use pkg.util\n\nfn main() {}\n"),
            ("util", "pub(pkg) fn one() -> Strng {\n    \"1\"\n}\n"),
        ],
        &[("src/util.osk", "E0200", "Strng")],
    );
}
