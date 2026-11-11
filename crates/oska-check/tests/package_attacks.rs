//! Attack programs (spec #115, #119), in the style of spec #105's review:
//! each uses a module boundary to try to hide authority, trust, a secret,
//! or a closure's capabilities, or to reach what another module keeps
//! private. Every one is rejected.

mod common;

use common::{assert_modules_errors, check_modules};

/// The codes `modules` reports, in order.
fn codes(modules: &[(&str, &str)]) -> Vec<&'static str> {
    check_modules(modules)
        .diagnostics
        .iter()
        .map(|d| d.code)
        .collect()
}

// ---- Reaching another module's private items without importing them ----------------

#[test]
fn writing_a_key_by_hand_reaches_nothing() {
    // `pkg.util.secret` is the key linking gives `secret`; a module that
    // writes it is writing field accesses on an unknown `pkg`.
    let main = "fn main() {}\n\nfn f() -> Int {\n    pkg.util.secret()\n}\n";
    let util = "fn secret() -> Int {\n    42\n}\n";
    let found = codes(&[("main", main), ("util", util)]);
    assert!(!found.is_empty(), "a hand-written key reached `secret`");
    assert!(!found.contains(&"E0206"), "{found:?}");
}

#[test]
fn a_local_named_pkg_does_not_reach_a_modules_items() {
    let main = "fn main() {}\n\nfn f(pkg: Int) -> Int {\n    pkg.util.secret()\n}\n";
    let util = "fn secret() -> Int {\n    42\n}\n";
    assert!(!codes(&[("main", main), ("util", util)]).is_empty());
}

#[test]
fn a_module_named_pkg_cannot_shadow_the_package_root() {
    let main = "use pkg.pkg\n\nfn main() {}\n\nfn f() -> Int {\n    pkg.util.secret()\n}\n";
    let shadow = "pub(pkg) fn util() -> Int {\n    1\n}\n";
    let util = "fn secret() -> Int {\n    42\n}\n";
    assert!(!codes(&[("main", main), ("pkg", shadow), ("util", util)]).is_empty());
}

#[test]
fn a_module_cannot_name_the_entry_modules_private_items() {
    let main = "fn main() {}\n\nfn helper(out: Stdout) {}\n";
    let util = "pub(pkg) fn f(out: Stdout) {\n    helper(out)\n}\n";
    assert_modules_errors(
        &[("main", main), ("util", util)],
        &[("src/util.osk", "E0221", "helper")],
    );
}

#[test]
fn an_impl_in_another_module_cannot_read_a_private_field() {
    let auth = "pub(pkg) struct Token {\n    secret: Int\n}\n";
    let leak = "use pkg.auth.{Token}\n\n\
                impl Token {\n    pub(pkg) fn leak(self) -> Int {\n        self.secret\n    }\n}\n";
    assert_modules_errors(
        &[("main", "fn main() {}\n"), ("auth", auth), ("leak", leak)],
        &[("src/leak.osk", "E0206", "secret")],
    );
}

#[test]
fn a_trait_impl_in_another_module_cannot_read_a_private_field() {
    let auth = "pub(pkg) struct Token {\n    secret: String\n}\n";
    let leak = "use pkg.auth.{Token}\n\n\
                impl Display for Token {\n    fn to_string(self) -> String {\n        self.secret\n    }\n}\n";
    assert_modules_errors(
        &[("main", "fn main() {}\n"), ("auth", auth), ("leak", leak)],
        &[("src/leak.osk", "E0206", "secret")],
    );
}

#[test]
fn a_private_method_cannot_be_taken_as_a_function_value() {
    let count = "pub(pkg) struct C {\n    pub n: Int\n}\n\n\
                 impl C {\n    fn bump(self) -> Int {\n        self.n\n    }\n}\n";
    let main = "use pkg.count.{C}\n\nfn main() {}\n\nfn f() -> fn(C) -> Int {\n    C.bump\n}\n";
    assert_modules_errors(
        &[("main", main), ("count", count)],
        &[("src/main.osk", "E0206", "bump")],
    );
}

#[test]
fn a_private_trait_cannot_be_used_as_a_bound_or_called_qualified() {
    let vault = "trait Unlock {\n    fn unlock(self) -> Int\n}\n";
    let main = "use pkg.vault\n\nfn main() {}\n\n\
                fn f<T: vault.Unlock>(x: T) -> Int {\n    vault.Unlock.unlock(x)\n}\n";
    let found = codes(&[("main", main), ("vault", vault)]);
    assert!(found.contains(&"E0206"), "{found:?}");
}

#[test]
fn a_private_traits_default_method_cannot_be_called_from_another_module() {
    let vault = "trait Unlock {\n    fn unlock(self) -> Int {\n        7\n    }\n}\n\n\
                 pub(pkg) struct V {\n    pub n: Int\n}\n\nimpl Unlock for V {}\n";
    let main = "use pkg.vault.{V}\n\nfn main() {}\n\nfn f(v: V) -> Int {\n    v.unlock()\n}\n";
    assert_modules_errors(
        &[("main", main), ("vault", vault)],
        &[("src/main.osk", "E0206", "unlock")],
    );
}

#[test]
fn a_private_struct_cannot_be_forged_through_a_qualifier() {
    let auth = "pub(pkg) struct Admin {\n    proof: Int\n}\n";
    let main =
        "use pkg.auth\n\nfn main() {}\n\nfn f() -> auth.Admin {\n    auth.Admin(proof: 1)\n}\n";
    assert_modules_errors(
        &[("main", main), ("auth", auth)],
        &[("src/main.osk", "E0206", "auth.Admin")],
    );
}

// ---- Hiding authority -------------------------------------------------------------

#[test]
fn a_capability_wrapped_in_another_modules_struct_shows_in_the_report() {
    let wrap = "pub(pkg) cap struct Handle {\n    pub out: Stdout\n}\n\n\
                pub(pkg) fn wrap(out: Stdout) -> Handle {\n    Handle(out: out)\n}\n";
    let main = "use pkg.wrap\n\nfn main(out: Stdout) {\n    use_it(wrap.wrap(out))\n}\n\n\
                fn use_it(h: wrap.Handle) {\n    let _ = h.out.println(\"x\")\n}\n";
    let report = check_modules(&[("main", main), ("wrap", wrap)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let use_it = report
        .functions
        .iter()
        .find(|f| f.name == "pkg.main.use_it")
        .expect("pkg.main.use_it");
    assert_eq!(use_it.authority, ["Stdout"]);
}

#[test]
fn a_generic_struct_of_another_module_carries_a_capability_it_is_given() {
    let boxes = "pub(pkg) struct Box<T> {\n    pub value: T\n}\n";
    let main = "use pkg.boxes.{Box}\n\nfn main() {}\n\n\
                fn f(b: Box<Stdout>) {\n    let _ = b.value.println(\"x\")\n}\n";
    let report = check_modules(&[("main", main), ("boxes", boxes)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let f = report
        .functions
        .iter()
        .find(|f| f.name == "pkg.main.f")
        .expect("pkg.main.f");
    assert_eq!(f.authority, ["Stdout"]);
}

#[test]
fn a_plain_struct_in_another_module_cannot_hold_a_capability() {
    let hide = "pub(pkg) struct Hidden {\n    pub f: cap fn()\n}\n";
    assert_modules_errors(
        &[("main", "fn main() {}\n"), ("hide", hide)],
        &[("src/hide.osk", "E0210", "Hidden")],
    );
}

#[test]
fn a_closure_built_in_one_module_and_stored_as_a_plain_fn_in_another_is_rejected() {
    let holder = "pub(pkg) struct Holder {\n    pub f: fn()\n}\n";
    let main = "use pkg.holder.{Holder}\n\nfn main(out: Stdout) {\n    \
                let h = Holder(f: || {\n        let _ = out.println(\"x\")\n    })\n}\n";
    let found = codes(&[("main", main), ("holder", holder)]);
    assert_eq!(found, ["E0270"]);
}

#[test]
fn a_closure_returned_from_another_module_as_a_plain_fn_cannot_capture() {
    let make = "pub(pkg) fn make(out: Stdout) -> fn() {\n    || {\n        let _ = out.println(\"x\")\n    }\n}\n";
    let found = codes(&[("main", "fn main() {}\n"), ("make", make)]);
    assert_eq!(found, ["E0270"]);
}

#[test]
fn a_generic_in_another_module_cannot_smuggle_a_capability_into_a_plain_fn() {
    let gen = "pub(pkg) fn later<T>(x: T) -> fn() -> T {\n    || x\n}\n";
    let main = "use pkg.gen\n\nfn main(out: Stdout) {\n    let f = gen.later(out)\n}\n";
    let found = codes(&[("main", main), ("gen", gen)]);
    assert_eq!(found, ["E0274"]);
}

#[test]
fn a_trait_default_method_in_another_module_shows_its_authority() {
    let log = "pub(pkg) trait Log {\n    fn log(self, out: Stdout) {\n        \
               let _ = out.println(\"x\")\n    }\n}\n";
    let main = "use pkg.log.{Log}\n\nfn main() {}\n";
    let report = check_modules(&[("main", main), ("log", log)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let names: Vec<String> = report.functions.iter().map(|f| f.to_human()).collect();
    assert!(
        names.contains(&"fn pkg.log.Log.log: Stdout (out)".to_string()),
        "{names:?}"
    );
}

#[test]
fn an_attenuation_in_a_closure_in_another_module_is_listed_under_its_function() {
    let files = "pub(pkg) fn docs(read: FsRead) -> cap fn() -> DirRead {\n    \
                 || read.restrict_to(\"docs\")\n}\n";
    let report = check_modules(&[("main", "fn main() {}\n"), ("files", files)]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let human = report.authority_to_human("unused");
    assert!(
        human.contains("fn pkg.files.docs: FsRead (read)\n  src/files.osk:"),
        "{human}"
    );
}

// ---- Hiding trust and secrets -------------------------------------------------------

#[test]
fn a_generic_in_another_module_cannot_launder_untrusted_text() {
    let launder = "pub(pkg) fn launder<T: Display>(x: T) -> String {\n    x.to_string()\n}\n";
    let main = "use pkg.launder\n\nfn main() {}\n\n\
                fn trusted(s: String) {}\n\n\
                fn f(u: Untrusted<String>) {\n    trusted(launder.launder(u))\n}\n";
    assert_modules_errors(
        &[("main", main), ("launder", launder)],
        &[
            ("src/main.osk", "E0300", "launder.launder(u)"),
            ("src/launder.osk", "E0300", "x.to_string()"),
        ],
    );
}

#[test]
fn a_struct_of_another_module_that_holds_untrusted_text_stays_untrusted() {
    let req = "pub(pkg) struct Req {\n    pub body: Untrusted<String>\n}\n\n\
               impl Display for Req {\n    fn to_string(self) -> String {\n        \"req\"\n    }\n}\n";
    let main = "use pkg.req.{Req}\n\nfn main() {}\n\nfn trusted(s: String) {}\n\n\
                fn f(r: Req) {\n    trusted(\"{r}\")\n}\n";
    let found = codes(&[("main", main), ("req", req)]);
    assert!(found.contains(&"E0300"), "{found:?}");
}

#[test]
fn a_secret_returned_through_another_modules_generic_cannot_be_displayed() {
    let keep = "pub(pkg) fn keep<T>(x: T) -> T {\n    x\n}\n";
    let main = "use pkg.keep\n\nfn main() {}\n\n\
                fn f(env: Env) -> String {\n    let k = keep.keep(env.secret(\"K\"))\n    \"{k}\"\n}\n";
    assert_modules_errors(
        &[("main", main), ("keep", keep)],
        &[("src/main.osk", "E0350", "{k}")],
    );
}

#[test]
fn a_secret_cannot_be_compared_in_another_module() {
    let cmp = "pub(pkg) fn same(a: Secret<String>, b: Secret<String>) -> Bool {\n    a == b\n}\n";
    let found = codes(&[("main", "fn main() {}\n"), ("cmp", cmp)]);
    assert_eq!(found, ["E0351"]);
}

#[test]
fn a_sink_literal_needs_its_std_import_in_its_own_module() {
    let main = "use std.sql\n\nfn main() {}\n";
    let db = "pub(pkg) fn q() -> Int {\n    let _ = sql\"SELECT 1\"\n    1\n}\n";
    let found = codes(&[("main", main), ("db", db)]);
    assert_eq!(found, ["E0310"]);
}

#[test]
fn a_module_named_like_a_std_sink_does_not_make_its_literals_trusted() {
    let main =
        "use pkg.sql\n\nfn main() {}\n\nfn f(u: Untrusted<String>) -> Int {\n    sql.run(u)\n}\n";
    let sql = "pub(pkg) fn run(q: String) -> Int {\n    1\n}\n";
    assert_modules_errors(
        &[("main", main), ("sql", sql)],
        &[("src/main.osk", "E0300", "u")],
    );
}

// ---- Constants ----------------------------------------------------------------------

#[test]
fn a_private_constant_of_another_module_cannot_be_used() {
    let files = "const ROOT: String = \"/\"\n";
    let main =
        "use pkg.files\n\nfn main(read: FsRead) {\n    let _ = read.restrict_to(files.ROOT)\n}\n";
    let found = codes(&[("main", main), ("files", files)]);
    assert!(found.contains(&"E0206"), "{found:?}");
}

#[test]
fn a_constant_of_another_module_is_treated_as_one_of_the_same_module() {
    // A constant is not yet usable as a value; across modules it is
    // refused with the code a single file reports.
    let single = common::check(
        "const ROOT: String = \"docs\"\nfn main(read: FsRead) {\n    let _ = read.restrict_to(ROOT)\n}\n",
    );
    let single: Vec<&str> = single.diagnostics.iter().map(|d| d.code).collect();
    let files = "pub(pkg) const ROOT: String = \"docs\"\n";
    let main =
        "use pkg.files\n\nfn main(read: FsRead) {\n    let _ = read.restrict_to(files.ROOT)\n}\n";
    assert_eq!(codes(&[("main", main), ("files", files)]), single);
}
