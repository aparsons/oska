//! The fix of `E0102` (ticket #374, spec #367): an expression in an
//! interpolation hole is hoisted into `let <fresh name> = <expression>` on
//! its own line before the statement, and the name goes in the hole. The
//! fix is `machine` when the expression calls only pure functions, so
//! `oskac fix` applies it, and `maybe` when it may reach a capability or
//! when hoisting would change when it runs: a loop condition, or the right
//! side of `&&` or `||`.
//!
//! Each case writes its program into a fresh scratch directory and runs
//! the real binary there: `oskac fix` for what is applied, and `oskac
//! check --format=json` for the applicability a diagnostic's fix gives.

mod common;

use common::portable::Scratch;
use common::Json;
use std::path::Path;
use std::process::{Command, Output};

fn scratch(name: &str, program: &str) -> Scratch {
    let dir = Scratch::new("hoist_fix", name);
    std::fs::write(dir.join("app.osk"), program).expect("should write the program");
    dir
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

fn program_in(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("app.osk")).expect("should read the program")
}

/// The `E0102` diagnostic of `oskac check app.osk --format=json`.
fn e0102(dir: &Path) -> Json {
    let output = oskac(dir, &["check", "app.osk", "--format=json"]);
    let report = Json::parse(&text(&output.stdout));
    let Some(Json::Arr(diagnostics)) = report.get("diagnostics") else {
        panic!("no diagnostics in {report:?}");
    };
    let found: Vec<&Json> = diagnostics
        .iter()
        .filter(|d| d.get("code") == Some(&Json::Str("E0102".into())))
        .collect();
    assert_eq!(found.len(), 1, "one E0102 in {diagnostics:?}");
    found[0].clone()
}

/// The applicability of the `E0102` fix, or `None` without a fix.
fn applicability(dir: &Path) -> Option<String> {
    match e0102(dir).get("fix")?.get("applicability") {
        Some(Json::Str(name)) => Some(name.clone()),
        other => panic!("a fix without an applicability: {other:?}"),
    }
}

/// `oskac fix` on `program`: whether it succeeded, and the program after.
fn fixed(name: &str, program: &str) -> (bool, String) {
    let dir = scratch(name, program);
    let output = oskac(&dir, &["fix", "app.osk"]);
    let after = program_in(&dir);
    if output.status.success() {
        let check = oskac(&dir, &["check", "app.osk"]);
        assert!(
            check.status.success(),
            "the fixed program should check:\n{after}\n{}",
            text(&check.stdout)
        );
    }
    (output.status.success(), after)
}

#[test]
fn fix_hoists_a_pure_expression_out_of_the_hole() {
    let (ok, after) = fixed(
        "pure",
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    out.println(\"next is {n + 1}\")\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    let value = n + 1\n    out.println(\"next is {value}\")\n}\n"
    );
}

/// `clock.now_millis()` reaches the `Clock`, so hoisting it could move an
/// effect: the fix is shown, as `maybe`, and `oskac fix` leaves it.
const REACHES_A_CAPABILITY: &str = "fn main(out: Stdout, clock: Clock) -> Result<(), IoError> {\n    out.println(\"now {clock.now_millis()}\")\n}\n";

#[test]
fn an_expression_that_may_reach_a_capability_gets_a_maybe_fix() {
    let dir = scratch("capability", REACHES_A_CAPABILITY);
    assert_eq!(applicability(&dir).as_deref(), Some("maybe"));
}

#[test]
fn fix_leaves_an_expression_that_may_reach_a_capability() {
    let (ok, after) = fixed("capability_fix", REACHES_A_CAPABILITY);
    assert!(!ok);
    assert_eq!(after, REACHES_A_CAPABILITY);
}

#[test]
fn a_call_of_a_pure_function_is_hoisted_by_fix() {
    let (ok, after) = fixed(
        "pure_call",
        "fn double(n: Int) -> Int {\n    n * 2\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    var n = 1\n    n = n + 1\n    out.println(\"twice is {double(n)}\")\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn double(n: Int) -> Int {\n    n * 2\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    var n = 1\n    n = n + 1\n    let value = double(n)\n    out.println(\"twice is {value}\")\n}\n"
    );
}

#[test]
fn a_call_that_passes_a_capability_on_gets_a_maybe_fix() {
    let dir = scratch(
        "passes_on",
        "fn stamp(clock: Clock) -> Int {\n    clock.now_millis()\n}\n\nfn main(out: Stdout, clock: Clock) -> Result<(), IoError> {\n    out.println(\"at {stamp(clock)}\")\n}\n",
    );
    assert_eq!(applicability(&dir).as_deref(), Some("maybe"));
}

#[test]
fn a_hole_in_a_loop_condition_gets_a_maybe_fix() {
    let dir = scratch(
        "loop_condition",
        "fn main() {\n    var i = 0\n    while \"{i + 1}\" != \"3\" {\n        i = i + 1\n    }\n}\n",
    );
    assert_eq!(applicability(&dir).as_deref(), Some("maybe"));
}

#[test]
fn a_hole_on_the_right_of_a_short_circuit_gets_a_maybe_fix() {
    for (name, op) in [("and", "&&"), ("or", "||")] {
        let dir = scratch(
            &format!("short_circuit_{name}"),
            &format!(
                "fn main() {{\n    let n = 1\n    let done = n > 0\n    let ok = done {op} \"{{n + 1}}\" == \"2\"\n}}\n"
            ),
        );
        assert_eq!(applicability(&dir).as_deref(), Some("maybe"), "{op}");
    }
}

#[test]
fn a_hole_on_the_left_of_a_short_circuit_is_hoisted_by_fix() {
    let (ok, after) = fixed(
        "short_circuit_left",
        "fn main() {\n    let n = 1\n    let ok = \"{n + 1}\" == \"2\" && n > 0\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn main() {\n    let n = 1\n    let value = n + 1\n    let ok = \"{value}\" == \"2\" && n > 0\n}\n"
    );
}

#[test]
fn the_hoisted_name_does_not_clash_with_a_name_in_scope() {
    let (ok, after) = fixed(
        "clash",
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let value = 1\n    let value2 = 2\n    out.println(\"sum {value + value2} of {value}\")\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let value = 1\n    let value2 = 2\n    let value3 = value + value2\n    out.println(\"sum {value3} of {value}\")\n}\n"
    );
}

#[test]
fn fix_hoists_every_hole_of_a_statement_in_order() {
    let (ok, after) = fixed(
        "two_holes",
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    out.println(\"{n + 1} and {n * 3}\")\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    let value = n + 1\n    let value2 = n * 3\n    out.println(\"{value} and {value2}\")\n}\n"
    );
}

#[test]
fn a_hole_inside_a_block_is_hoisted_within_that_block() {
    let (ok, after) = fixed(
        "in_block",
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    if n > 0 {\n        out.println(\"{n - 1}\")?\n    }\n    Ok(())\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let n = 1\n    if n > 0 {\n        let value = n - 1\n        out.println(\"{value}\")?\n    }\n    Ok(())\n}\n"
    );
}

#[test]
fn a_hole_in_a_closure_whose_body_is_not_a_block_has_no_fix() {
    let dir = scratch(
        "closure",
        "fn main() {\n    let show = |n: Int| \"{n + 1}\"\n}\n",
    );
    assert_eq!(applicability(&dir), None);
}

#[test]
fn fix_hoists_in_a_module_of_a_package() {
    let dir = Scratch::new("hoist_fix", "package");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("oska.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/main.osk"),
        "use pkg.util.{label}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    out.println(label(2))\n}\n",
    )
    .expect("main");
    std::fs::write(
        dir.join("src/util.osk"),
        "pub(pkg) fn label(n: Int) -> String {\n    \"next {n + 1}\"\n}\n",
    )
    .expect("util");
    let output = oskac(&dir, &["fix"]);
    let after = std::fs::read_to_string(dir.join("src/util.osk")).expect("util");
    assert!(output.status.success(), "{}\n{after}", text(&output.stdout));
    assert_eq!(
        after,
        "pub(pkg) fn label(n: Int) -> String {\n    let value = n + 1\n    \"next {value}\"\n}\n"
    );
}

/// Hoisting moves the hole's expression ahead of everything that runs
/// before it in the statement: there an effect, a `?` or a mutation of
/// what the hole reads could change what the program does, so the fix is
/// `maybe` even though the expression itself is pure.
#[test]
fn a_hole_after_an_effect_or_early_exit_in_its_statement_gets_a_maybe_fix() {
    let dir = scratch(
        "after_try",
        "fn pair(a: (), b: String) -> String {\n    b\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    let n = 6\n    let d = 2\n    let s = pair(out.println(\"a\")?, \"{n / d}\")\n    out.println(s)\n}\n",
    );
    assert_eq!(applicability(&dir).as_deref(), Some("maybe"));
}

#[test]
fn a_hole_after_a_mutation_of_what_it_reads_gets_a_maybe_fix() {
    let dir = scratch(
        "after_mutation",
        "fn pair(a: (), b: String) -> String {\n    b\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    var xs = [1, 2]\n    let s = pair(xs.push(3), \"{xs.len()}\")\n    out.println(s)\n}\n",
    );
    assert_eq!(applicability(&dir).as_deref(), Some("maybe"));
}

/// What runs before the hole in its statement is only names and
/// literals, so nothing can be reordered: the fix stays `machine`.
#[test]
fn a_hole_after_plain_values_in_its_statement_is_hoisted_by_fix() {
    let (ok, after) = fixed(
        "after_values",
        "fn pair(a: Int, b: String) -> String {\n    b\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    let n = 6\n    let s = pair(n, \"{n / 2}\")\n    out.println(s)\n}\n",
    );
    assert!(ok, "{after}");
    assert_eq!(
        after,
        "fn pair(a: Int, b: String) -> String {\n    b\n}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    let n = 6\n    let value = n / 2\n    let s = pair(n, \"{value}\")\n    out.println(s)\n}\n"
    );
}
