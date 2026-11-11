//! Splitting a program into modules never weakens a guarantee (spec #115,
//! #119). Each test splits the violating code of a single-file test across
//! modules and expects the same code, in the file where the single-file
//! version reports it.

mod common;

use common::{assert_errors, assert_modules_errors, assert_modules_ok};

// ---- Trust tracking and sinks ----------------------------------------------------

#[test]
fn untrusted_text_passed_to_a_trusted_parameter_of_another_module_is_rejected() {
    let helper = "pub(pkg) fn count(text: String) -> Int {\n    text.len()\n}\n";
    let main = "use pkg.words\n\nfn main() {}\n\n\
                fn f(u: Untrusted<String>) -> Int {\n    words.count(u)\n}\n";
    assert_errors(
        &format!("{helper}\nfn f(u: Untrusted<String>) -> Int {{\n    count(u)\n}}\n"),
        &[("E0300", "u")],
    );
    assert_modules_errors(
        &[("main", main), ("words", helper)],
        &[("src/main.osk", "E0300", "u")],
    );
}

#[test]
fn untrusted_text_returned_from_another_module_stays_untrusted() {
    let helper = "pub(pkg) fn read(u: Untrusted<String>) -> Untrusted<String> {\n    u\n}\n";
    let main = "use pkg.input\n\nfn main() {}\n\n\
                fn f(u: Untrusted<String>) -> String {\n    input.read(u)\n}\n";
    assert_modules_errors(
        &[("main", main), ("input", helper)],
        &[("src/main.osk", "E0300", "input.read(u)")],
    );
}

#[test]
fn untrusted_text_cannot_reach_a_sink_through_another_modules_function() {
    let helper = "use std.sql\n\n\
                  pub(pkg) fn run(q: sql.SqlLiteral) -> Int {\n    1\n}\n";
    let main = "use pkg.db\n\nfn main() {}\n\n\
                fn f(u: Untrusted<String>) -> Int {\n    db.run(u)\n}\n";
    let report = common::check_modules(&[("main", main), ("db", helper)]);
    assert!(
        !report.diagnostics.is_empty(),
        "untrusted text reached a sink: {:#?}",
        report.diagnostics
    );
}

// ---- Secrets ---------------------------------------------------------------------

#[test]
fn a_secret_from_another_module_cannot_be_interpolated() {
    let helper = "pub(pkg) fn key(env: Env) -> Secret<String> {\n    env.secret(\"API_KEY\")\n}\n";
    let main = "use pkg.config\n\nfn main() {}\n\n\
                fn header(env: Env) -> String {\n    let key = config.key(env)\n    \"Bearer {key}\"\n}\n";
    assert_modules_errors(
        &[("main", main), ("config", helper)],
        &[("src/main.osk", "E0350", "{key}")],
    );
}

#[test]
fn a_secret_field_of_another_modules_struct_cannot_be_interpolated() {
    let helper = "pub(pkg) struct Config {\n    pub token: Secret<String>\n}\n";
    let main = "use pkg.config\n\nfn main() {}\n\n\
                fn show(c: config.Config) -> String {\n    \"token: {c.token}\"\n}\n";
    assert_modules_errors(
        &[("main", main), ("config", helper)],
        &[("src/main.osk", "E0350", "{c.token}")],
    );
}

#[test]
fn exposing_a_secret_in_another_module_is_an_audit_site_of_that_file() {
    let helper = "pub(pkg) fn token(env: Env) -> String {\n    env.secret(\"K\").expose()\n}\n";
    let main = "use pkg.config\n\nfn main(env: Env) {\n    let _ = config.token(env)\n}\n";
    let report = common::check_modules(&[("main", main), ("config", helper)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let sites: Vec<(&str, &str)> = report
        .audit
        .iter()
        .map(|s| (s.kind.as_str(), common::path_of(s.location.as_ref())))
        .collect();
    assert_eq!(sites, [("expose", "src/config.osk")]);
}

// ---- Capabilities and authority ---------------------------------------------------

#[test]
fn a_capability_passed_to_another_module_keeps_its_authority_in_the_report() {
    let helper = "pub(pkg) fn log(out: Stdout) {\n    let _ = out.println(\"x\")\n}\n";
    let main = "use pkg.logging\n\nfn main(out: Stdout) {\n    logging.log(out)\n}\n";
    let report = common::check_modules(&[("main", main), ("logging", helper)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let log = report
        .functions
        .iter()
        .find(|f| f.name == "logging.log")
        .expect("logging.log in the report");
    assert_eq!(log.authority, ["Stdout"]);
    assert!(!log.pure);
}

#[test]
fn a_capability_type_of_another_module_must_be_declared_cap() {
    let helper = "pub(pkg) struct Logger {\n    pub out: Stdout\n}\n";
    let main = "fn main() {}\n";
    assert_errors(helper, &[("E0210", "Logger")]);
    assert_modules_errors(
        &[("main", main), ("logging", helper)],
        &[("src/logging.osk", "E0210", "Logger")],
    );
}

#[test]
fn a_cap_struct_of_another_module_carries_its_authority() {
    let helper = "pub(pkg) cap struct Logger {\n    pub out: Stdout\n}\n";
    let main = "use pkg.logging\n\nfn main() {}\n\n\
                fn f(l: logging.Logger) {\n    let _ = l.out.println(\"x\")\n}\n";
    let report = common::check_modules(&[("main", main), ("logging", helper)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let f = report
        .functions
        .iter()
        .find(|f| f.name == "main.f")
        .expect("main.f in the report");
    assert_eq!(f.authority, ["Stdout"]);
}

#[test]
fn a_plain_fn_closure_from_another_module_cannot_capture_a_capability() {
    let helper = "pub(pkg) fn run(f: fn() -> Int) -> Int {\n    f()\n}\n";
    let main = "use pkg.runner\n\nfn main() {}\n\n\
                fn f(out: Stdout) -> Int {\n    runner.run(|| {\n        let _ = out\n        2\n    })\n}\n";
    assert_modules_errors(
        &[("main", main), ("runner", helper)],
        &[(
            "src/main.osk",
            "E0270",
            "|| {\n        let _ = out\n        2\n    }",
        )],
    );
}

#[test]
fn a_capability_cannot_be_forged_through_another_modules_name() {
    let main = "fn main() {}\n";
    let helper = "pub(pkg) fn forge() -> Stdout {\n    Stdout()\n}\n";
    assert_errors(
        "fn forge() -> Stdout {\n    Stdout()\n}\n",
        &[("E0271", "Stdout")],
    );
    assert_modules_errors(
        &[("main", main), ("forge", helper)],
        &[("src/forge.osk", "E0271", "Stdout")],
    );
}

#[test]
fn an_attenuation_in_another_module_names_its_file_and_function() {
    let helper = "pub(pkg) fn docs(read: FsRead) -> DirRead {\n    read.restrict_to(\"docs\")\n}\n";
    let main = "use pkg.files\n\nfn main(read: FsRead) {\n    let _ = files.docs(read)\n}\n";
    let report = common::check_modules(&[("main", main), ("files", helper)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let entries: Vec<(&str, &str, &str)> = report
        .attenuations
        .iter()
        .map(|a| {
            (
                a.function.as_str(),
                a.result.as_str(),
                common::path_of(a.location.as_ref()),
            )
        })
        .collect();
    assert_eq!(entries, [("files.docs", "DirRead", "src/files.osk")]);
    let docs = report
        .functions
        .iter()
        .find(|f| f.name == "files.docs")
        .expect("files.docs in the report");
    assert_eq!(docs.authority, ["FsRead"]);
}

// ---- Contracts and their purity rules -----------------------------------------------

#[test]
fn an_impure_call_into_another_module_in_a_contract_is_rejected() {
    let helper =
        "pub(pkg) fn log(out: Stdout) -> Bool {\n    let _ = out.println(\"x\")\n    true\n}\n";
    let main = "use pkg.logging\n\nfn main() {}\n\n\
                fn f(out: Stdout) -> Int\n    requires logging.log(out)\n{\n    1\n}\n";
    assert_modules_errors(
        &[("main", main), ("logging", helper)],
        &[("src/main.osk", "E0280", "logging.log(out)")],
    );
}

#[test]
fn a_question_mark_in_a_contract_calling_another_module_is_rejected() {
    let helper = "pub(pkg) fn maybe(n: Int) -> Option<Int> {\n    Some(n)\n}\n";
    let main = "use pkg.m\n\nfn main() {}\n\n\
                fn f(n: Int) -> Option<Int>\n    requires m.maybe(n)? > 0\n{\n    Some(n)\n}\n";
    assert_modules_errors(
        &[("main", main), ("m", helper)],
        &[("src/main.osk", "E0282", "m.maybe(n)?")],
    );
}

#[test]
fn a_trait_contract_binds_an_impl_in_another_module() {
    let shapes =
        "pub(pkg) trait Shape {\n    fn area(self) -> Int\n        requires self.sides() > 0\n    \
                  fn sides(self) -> Int\n}\n";
    let main = "use pkg.shapes.{Shape}\n\nfn main() {}\n\nstruct Square { side: Int }\n\n\
                impl Shape for Square {\n    fn sides(self) -> Int { 4 }\n    \
                fn area(self) -> Int\n        requires self.side > 0\n    { self.side * self.side }\n}\n";
    assert_modules_errors(
        &[("main", main), ("shapes", shapes)],
        &[("src/main.osk", "E0218", "requires self.side > 0")],
    );
}

#[test]
fn a_generic_contract_of_another_module_rejects_a_capability() {
    let speak = "pub(pkg) trait Speak {\n    fn ok(self) -> Bool\n}\n\n\
                 impl Speak for Stdout {\n    fn ok(self) -> Bool {\n        true\n    }\n}\n\n\
                 pub(pkg) fn check<T: Speak>(x: T) -> Int\n    requires x.ok()\n{\n    1\n}\n";
    let main = "use pkg.speak\n\nfn main(out: Stdout) {\n    let n = speak.check(out)\n}\n";
    assert_modules_errors(
        &[("main", main), ("speak", speak)],
        &[("src/main.osk", "E0274", "speak.check")],
    );
}

#[test]
fn a_trait_whose_contract_uses_self_rejects_a_capability_bearing_impl_in_another_module() {
    let speak = "pub(pkg) trait Speak {\n    fn ok(self) -> Bool\n    \
                 fn go(self) -> Int\n        requires self.ok()\n}\n";
    let main = "use pkg.speak.{Speak}\n\nfn main() {}\n\n\
                impl Speak for Stdout {\n    fn ok(self) -> Bool {\n        true\n    }\n    \
                fn go(self) -> Int {\n        1\n    }\n}\n";
    let report = common::check_modules(&[("main", main), ("speak", speak)]);
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0274"], "{:#?}", report.diagnostics);
}

// ---- The cap fn join -------------------------------------------------------------------

#[test]
fn a_cap_fn_from_another_module_joins_a_list_of_plain_fns() {
    let helper = "pub(pkg) fn pure() -> Int {\n    1\n}\n\n\
                  pub(pkg) fn all(fs: List<fn() -> Int>) -> Int {\n    fs.len()\n}\n";
    let main = "use pkg.fns\n\nfn main() {}\n\n\
                fn f(out: Stdout) -> Int {\n    let fs = [fns.pure, || {\n        let _ = out\n        2\n    }]\n    \
                fns.all(fs)\n}\n";
    assert_modules_errors(
        &[("main", main), ("fns", helper)],
        &[("src/main.osk", "E0270", "fs")],
    );
}

#[test]
fn a_cap_fn_returned_from_another_module_cannot_become_a_plain_fn() {
    let helper = "pub(pkg) fn logger(out: Stdout) -> cap fn() {\n    || {\n        let _ = out.println(\"x\")\n    }\n}\n";
    let main = "use pkg.logging\n\nfn main() {}\n\n\
                fn f(out: Stdout) -> fn() {\n    logging.logger(out)\n}\n";
    let report = common::check_modules(&[("main", main), ("logging", helper)]);
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert!(
        !codes.is_empty(),
        "a cap fn became a plain fn across modules"
    );
}

// ---- Coherence and the orphan rule, per package -------------------------------------

#[test]
fn an_impl_may_live_in_any_module_of_the_package() {
    let types = "pub(pkg) struct Point {\n    pub x: Int\n}\n";
    let shows = "use pkg.types.{Point}\n\n\
                 impl Display for Point {\n    fn to_string(self) -> String {\n        \"p\"\n    }\n}\n";
    let main = "use pkg.types.{Point}\n\nfn main() {}\n\n\
                fn f(p: Point) -> String {\n    p.to_string()\n}\n";
    assert_modules_ok(&[("main", main), ("types", types), ("shows", shows)]);
}

#[test]
fn an_inherent_impl_may_live_in_another_module_of_the_package() {
    let types = "pub(pkg) struct Point {\n    pub x: Int\n}\n";
    let methods = "use pkg.types.{Point}\n\n\
                   impl Point {\n    pub(pkg) fn double(self) -> Int {\n        self.x * 2\n    }\n}\n";
    let main = "use pkg.types.{Point}\n\nfn main() {}\n\n\
                fn f(p: Point) -> Int {\n    p.double()\n}\n";
    assert_modules_ok(&[("main", main), ("types", types), ("methods", methods)]);
}

#[test]
fn two_impls_of_one_trait_for_one_type_in_two_modules_conflict() {
    let types = "pub(pkg) struct Foo {\n    pub x: Int\n}\n\n\
                 impl Display for Foo {\n    fn to_string(self) -> String {\n        \"a\"\n    }\n}\n";
    let main = "use pkg.types.{Foo}\n\nfn main() {}\n\n\
                impl Display for Foo {\n    fn to_string(self) -> String {\n        \"b\"\n    }\n}\n";
    assert_modules_errors(
        &[("main", main), ("types", types)],
        // The entry module comes first, so the other module's impl is the
        // second.
        &[("src/types.osk", "E0290", "Display")],
    );
}

#[test]
fn two_inherent_methods_of_one_name_in_two_modules_conflict() {
    let types = "pub(pkg) struct Foo {\n    pub x: Int\n}\n\n\
                 impl Foo {\n    pub(pkg) fn a(self) -> Int {\n        1\n    }\n}\n";
    let main = "use pkg.types.{Foo}\n\nfn main() {}\n\n\
                impl Foo {\n    fn a(self) -> Int {\n        2\n    }\n}\n";
    let report = common::check_modules(&[("main", main), ("types", types)]);
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0202"], "{:#?}", report.diagnostics);
}

#[test]
fn no_module_can_add_inherent_methods_to_a_prelude_type() {
    let helper = "impl FsRead {\n    fn size(self) -> Int {\n        1\n    }\n}\n";
    assert_modules_errors(
        &[("main", "fn main() {}\n"), ("ext", helper)],
        &[("src/ext.osk", "E0291", "FsRead")],
    );
}

// ---- Recursion ---------------------------------------------------------------------

#[test]
fn types_that_contain_each_other_across_modules_have_no_base_case() {
    let ping = "use pkg.pong.{Pong}\n\npub(pkg) struct Ping {\n    pub pong: Pong\n}\n";
    let pong = "pub(pkg) struct Pong {\n    pub n: Int\n}\n";
    // Modules cannot import each other, so a cycle goes through the
    // entry module's plain names... which no other module can see; the
    // cycle is within one direction only. One module's type containing
    // itself through another's generic still has no base case.
    assert_modules_ok(&[("main", "fn main() {}\n"), ("ping", ping), ("pong", pong)]);
    let wrap = "pub(pkg) struct Wrap<T> {\n    pub inner: T\n}\n";
    let main = "use pkg.wrap.{Wrap}\n\nfn main() {}\n\nstruct Loop {\n    me: Wrap<Loop>\n}\n";
    assert_modules_errors(
        &[("main", main), ("wrap", wrap)],
        &[("src/main.osk", "E0215", "Loop")],
    );
}

// ---- Trait conformance -------------------------------------------------------------

#[test]
fn an_impl_in_another_module_missing_a_trait_method_is_rejected() {
    let greet = "pub(pkg) trait Greet {\n    fn name(self) -> String\n    fn age(self) -> Int\n}\n";
    let main = "use pkg.greet.{Greet}\n\nfn main() {}\n\nstruct Cat { name: String }\n\n\
                impl Greet for Cat {\n    fn name(self) -> String { self.name }\n}\n";
    assert_modules_errors(
        &[("main", main), ("greet", greet)],
        &[("src/main.osk", "E0261", "Greet")],
    );
}

// ---- The authority report names functions by module ------------------------------

/// The report's functions of the package of `modules`, as their human
/// lines.
fn report_lines(modules: &[(&str, &str)]) -> Vec<String> {
    let report = common::check_modules(modules);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    report.functions.iter().map(|f| f.to_human()).collect()
}

#[test]
fn functions_of_one_name_in_two_modules_are_told_apart_by_module_path() {
    let lines = report_lines(&[
        (
            "main",
            "use pkg.a.log\n\nfn main(out: Stdout) {\n    log.write(out)\n}\n\nfn write() {}\n",
        ),
        (
            "a.log",
            "pub(pkg) fn write(out: Stdout) {\n    let _ = out.println(\"x\")\n}\n",
        ),
    ]);
    assert_eq!(
        lines,
        [
            "fn main.main: Stdout (out)",
            "fn main.write: pure",
            "fn a.log.write: Stdout (out)",
        ]
    );
}

#[test]
fn a_method_is_named_by_the_module_of_its_impl_and_its_type() {
    let lines = report_lines(&[
        ("main", "use pkg.geo.{Point}\n\nfn main() {}\n\nimpl Display for Point {\n    fn to_string(self) -> String {\n        \"p\"\n    }\n}\n"),
        ("geo", "pub(pkg) struct Point {\n    pub x: Int\n}\n\nimpl Point {\n    pub(pkg) fn x(self) -> Int {\n        self.x\n    }\n}\n\ntrait Area {\n    fn area(self) -> Int\n}\n\nimpl Area for Int {\n    fn area(self) -> Int {\n        self\n    }\n}\n"),
    ]);
    assert_eq!(
        lines,
        [
            "fn main.main: pure",
            "fn main.geo.Point.to_string: pure",
            "fn geo.Point.x: pure",
            "fn geo.Int.area: pure",
        ]
    );
}

#[test]
fn a_pub_function_is_marked_pub() {
    let report = common::check_modules(&[
        ("main", "fn main() {}\n"),
        (
            "api",
            "pub fn open() {}\n\npub(pkg) fn shared() {}\n\nfn own() {}\n",
        ),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let lines: Vec<String> = report.functions.iter().map(|f| f.to_human()).collect();
    assert_eq!(
        lines,
        [
            "fn main.main: pure",
            "pub fn api.open: pure",
            "fn api.shared: pure",
            "fn api.own: pure",
        ]
    );
    let json: Vec<String> = report.functions.iter().map(|f| f.to_json()).collect();
    assert_eq!(
        json[1],
        r#"{"name":"api.open","pub":true,"authority":[],"via":[],"pure":true}"#
    );
    assert_eq!(
        json[2],
        r#"{"name":"api.shared","authority":[],"via":[],"pure":true}"#
    );
}

#[test]
fn a_single_file_keeps_plain_names() {
    common::assert_authority("fn main(out: Stdout) {}\n", "main", &["Stdout"], false);
}
