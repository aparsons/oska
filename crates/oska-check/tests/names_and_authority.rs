//! The checker tracer (#34): type names resolve against the prelude, and
//! every function gets an authority report.

mod common;

use common::{assert_authority, assert_errors, assert_fix, assert_ok, assert_report_json};

#[test]
fn a_capability_parameter_is_the_functions_authority() {
    assert_authority(
        "fn count(fs: FsRead, path: Path) -> Int { 0 }",
        "count",
        &["FsRead"],
        false,
    );
}

#[test]
fn a_function_without_capability_parameters_is_pure() {
    assert_authority("fn add(a: Int, b: Int) -> Int { a + b }", "add", &[], true);
}

#[test]
fn plain_data_containers_stay_pure() {
    assert_authority(
        "fn total(xs: List<Int>, label: Option<String>) -> Int { 0 }",
        "total",
        &[],
        true,
    );
}

#[test]
fn a_methods_receiver_counts_towards_its_authority() {
    assert_authority(
        "cap struct Logger { out: Stdout }\n\
         impl Logger {\n    fn log(self, line: String) { }\n}",
        "Logger.log",
        &["Stdout"],
        false,
    );
}

#[test]
fn a_method_on_plain_data_is_pure() {
    assert_authority(
        "struct Point { x: Int, y: Int }\n\
         impl Point {\n    fn sum(self) -> Int { 0 }\n}",
        "Point.sum",
        &[],
        true,
    );
}

#[test]
fn a_list_of_capabilities_carries_them() {
    assert_authority(
        "fn broadcast(outs: List<Stdout>) { }",
        "broadcast",
        &["Stdout"],
        false,
    );
}

#[test]
fn a_user_generic_container_is_expanded_through_its_argument() {
    assert_authority(
        "struct Box<T> { value: T }\nfn f(b: Box<Option<Clock>>) { }",
        "f",
        &["Clock"],
        false,
    );
}

#[test]
fn a_cap_struct_is_expanded_to_the_capabilities_in_its_fields() {
    assert_authority(
        "cap struct Ctx { out: Stdout, log: Logger, net: NetConnect }\n\
         cap struct Logger { err: Stderr, out: Stdout }\n\
         fn run(ctx: Ctx, n: Int) { }",
        "run",
        &["Stdout", "Stderr", "NetConnect"],
        false,
    );
}

#[test]
fn a_cap_enum_is_expanded_through_every_variant() {
    assert_authority(
        "cap enum Sink {\n    Console(Stdout),\n    File(FsWrite),\n    Nowhere,\n}\n\
         fn emit(sink: Sink) { }",
        "emit",
        &["Stdout", "FsWrite"],
        false,
    );
}

#[test]
fn a_recursive_cap_type_is_expanded_once() {
    assert_authority(
        "cap struct Chain { out: Stdout, next: Option<Chain> }\nfn f(c: Chain) { }",
        "f",
        &["Stdout"],
        false,
    );
}

#[test]
fn attenuated_capabilities_keep_their_names() {
    assert_authority(
        "fn serve(fs: FsRead, net: NetListen, out: FsWrite) { }",
        "serve",
        &["FsRead", "NetListen", "FsWrite"],
        false,
    );
}

#[test]
fn authority_is_a_set_across_parameters() {
    assert_authority(
        "fn f(a: Stdout, b: List<Stdout>, c: Env) { }",
        "f",
        &["Stdout", "Env"],
        false,
    );
}

#[test]
fn a_cap_fn_parameter_makes_a_function_impure() {
    assert_authority(
        "fn each(xs: List<Int>, f: cap fn(Int)) { }",
        "each",
        &[],
        false,
    );
}

#[test]
fn a_plain_fn_parameter_stays_pure() {
    assert_authority(
        "fn each(xs: List<Int>, f: fn(Int) -> Int) { }",
        "each",
        &[],
        true,
    );
}

#[test]
fn a_generic_parameter_is_pure_until_instantiated() {
    assert_authority("fn id<T>(x: T) -> T { x }", "id", &[], true);
}

#[test]
fn authority_is_reported_even_with_errors() {
    assert_authority("fn f(out: Stdout, x: Widget) { }", "f", &["Stdout"], false);
}

// ---- Name resolution -------------------------------------------------------

#[test]
fn prelude_names_resolve_without_imports() {
    assert_ok(
        "fn f(a: Stdin, b: Stdout, c: Stderr, d: Fs, e: FsRead, f: FsWrite,\n\
         \x20     g: Net, h: NetConnect, i: NetListen, j: Env, k: Clock,\n\
         \x20     l: Random, m: Sched, n: Spawn, o: Args, p: Path) { }\n\
         fn g(a: Int, b: I8, c: I16, d: I32, e: I64, f: U8, g: U16, h: U32,\n\
         \x20     i: U64, j: Bool, k: String, l: ()) -> Result<Option<List<Int>>, IoError> { }",
    );
}

#[test]
fn an_empty_module_checks_cleanly() {
    assert_ok("");
}

#[test]
fn user_types_and_generics_resolve() {
    assert_ok(
        "struct Pair<A, B> { a: A, b: B }\n\
         enum Shape { Circle(Int), Empty }\n\
         fn first<T: Eq + Display>(p: Pair<T, Shape>) -> T { p.a }",
    );
}

#[test]
fn self_resolves_in_impls_and_traits() {
    assert_ok(
        "trait Named {\n    fn same(self, other: Self) -> Bool\n}\n\
         struct User { id: Int }\n\
         impl Named for User {\n    fn same(self, other: Self) -> Bool { true }\n}\n\
         impl From<Int> for User {\n    fn from(id: Int) -> Self { User(id: id) }\n}",
    );
}

#[test]
fn an_unknown_type_name_is_an_error() {
    assert_errors("fn f(x: Widget) -> Int { 0 }", &[("E0200", "Widget")]);
}

#[test]
fn every_unknown_name_is_reported() {
    assert_errors(
        "struct S { a: Foo, b: List<Bar> }\n\
         enum E { V(Baz) }\n\
         fn f<T: Qux>(x: fn(Quux) -> Corge) { }\n\
         const C: Grault = 1",
        &[
            ("E0200", "Foo"),
            ("E0200", "Bar"),
            ("E0200", "Baz"),
            ("E0200", "Qux"),
            ("E0200", "Quux"),
            ("E0200", "Corge"),
            ("E0200", "Grault"),
        ],
    );
}

#[test]
fn names_in_impls_and_traits_are_resolved() {
    assert_errors(
        "trait T {\n    fn m(self) -> Missing\n}\n\
         impl Unknown for Nowhere {\n    fn m(self, x: Gone) { }\n}",
        &[
            ("E0200", "Missing"),
            ("E0200", "Unknown"),
            ("E0200", "Nowhere"),
            ("E0200", "Gone"),
        ],
    );
}

#[test]
fn self_outside_an_impl_is_unknown() {
    assert_errors("fn f(x: Self) { }", &[("E0200", "Self")]);
}

#[test]
fn a_generic_parameter_is_scoped_to_its_declaration() {
    assert_errors("fn f<T>(x: T) { }\nfn g(x: T) { }", &[("E0200", "T")]);
}

#[test]
fn a_qualified_type_path_is_unknown() {
    // `use` of `std.*` modules comes later; until then no path resolves.
    assert_errors("fn f(q: sql.Query) { }", &[("E0200", "sql.Query")]);
}

#[test]
fn an_unknown_name_suggests_the_closest_one() {
    let report = common::check("fn f(path: Pth) { }");
    assert_eq!(
        report.diagnostics[0].message,
        "unknown type `Pth`; did you mean `Path`?"
    );
}

#[test]
fn the_fix_replaces_a_misspelled_type_with_the_closest_name() {
    assert_fix(
        "fn greet(out: Stdotu, name: Strng) { }",
        "E0200",
        "fn greet(out: Stdout, name: Strng) { }",
    );
    assert_fix(
        "struct Config { path: Path, retries: Itn }",
        "E0200",
        "struct Config { path: Path, retries: Int }",
    );
}

#[test]
fn the_fix_can_suggest_a_user_type_or_generic() {
    assert_fix(
        "struct Account { id: Int }\nfn load(a: Acount) { }",
        "E0200",
        "struct Account { id: Int }\nfn load(a: Account) { }",
    );
    assert_fix(
        "fn keep<Item>(x: Iten) { }",
        "E0200",
        "fn keep<Item>(x: Item) { }",
    );
}

#[test]
fn a_name_far_from_every_declaration_has_no_fix() {
    let report = common::check("fn f(x: Zzyzx) { }");
    assert_eq!(report.diagnostics[0].message, "unknown type `Zzyzx`");
    assert_eq!(report.diagnostics[0].fix, None);
}

#[test]
fn an_unknown_trait_suggests_a_trait() {
    assert_fix(
        "fn f<T: Dispaly>(x: T) { }",
        "E0200",
        "fn f<T: Display>(x: T) { }",
    );
}

#[test]
fn a_trait_is_not_a_type() {
    assert_errors("fn show(x: Display) { }", &[("E0201", "Display")]);
}

#[test]
fn a_type_is_not_a_trait() {
    assert_errors(
        "fn f<T: Int>(x: T) { }\nstruct P { }\nimpl String for P { }",
        &[("E0201", "Int"), ("E0201", "String")],
    );
}

#[test]
fn a_type_declared_twice_is_an_error() {
    assert_errors(
        "struct Point { x: Int }\nenum Point { A }",
        &[("E0202", "Point")],
    );
}

#[test]
fn a_prelude_name_cannot_be_redeclared() {
    // A user `Stdout` would otherwise shadow the built-in capability.
    assert_errors(
        "struct Stdout { fd: Int }\ntrait Display { }\nenum Bool { Yes, No }",
        &[("E0202", "Stdout"), ("E0202", "Display"), ("E0202", "Bool")],
    );
}

#[test]
fn a_parse_error_is_reported_with_no_functions() {
    let report = common::check("fn f( { }");
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.diagnostics[0].code.starts_with("E01"));
    assert!(report.functions.is_empty());
}

// ---- The JSON report: {"diagnostics": [...], "functions": [...]} -------------

#[test]
fn a_clean_module_reports_only_authority() {
    assert_report_json(
        "cap struct Logger { out: Stdout }\n\
         impl Logger {\n    fn log(self, line: String) { }\n}\n\
         fn run(log: Logger, fs: FsRead, n: Int) { }\n\
         fn add(a: Int, b: Int) -> Int { a + b }",
        r#"{
            "diagnostics": [],
            "functions": [
                {"name": "Logger.log", "authority": ["Stdout"],
                 "via": [{"param": "self", "capabilities": ["Stdout"]}],
                 "pure": false},
                {"name": "run", "authority": ["Stdout", "FsRead"],
                 "via": [{"param": "log", "capabilities": ["Stdout"]},
                         {"param": "fs", "capabilities": ["FsRead"]}],
                 "pure": false},
                {"name": "add", "authority": [], "via": [], "pure": true}
            ]
        }"#,
    );
}

#[test]
fn diagnostics_use_the_shared_envelope_with_their_fix() {
    assert_report_json(
        "fn f(x: Strng) { }",
        r#"{
            "diagnostics": [
                {"code": "E0200",
                 "message": "unknown type `Strng`; did you mean `String`?",
                 "span": {"start": 8, "end": 13},
                 "fix": {"message": "replace with `String`",
                         "edits": [{"span": {"start": 8, "end": 13},
                                    "replacement": "String"}]}}
            ],
            "functions": [
                {"name": "f", "authority": [], "via": [], "pure": true}
            ]
        }"#,
    );
}

#[test]
fn a_cap_fn_parameter_is_listed_in_via_without_capabilities() {
    assert_report_json(
        "fn each(f: cap fn(Int)) { }",
        r#"{
            "diagnostics": [],
            "functions": [
                {"name": "each", "authority": [],
                 "via": [{"param": "f", "capabilities": []}],
                 "pure": false}
            ]
        }"#,
    );
}
