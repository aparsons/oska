//! Divergences the full differential corpus (#140) found between the
//! native backend and the interpreter, each fixed and kept fixed here.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

/// A package whose `main` calls `call` on the module `util`, which holds
/// `util`: the outcome on both backends, which must be a panic whose
/// report names `expected`.
fn assert_package_panic(name: &str, util: &str, call: &str, expected: &str) {
    let main = format!(
        "use pkg.util\n\nfn main(out: Stdout) -> Result<(), IoError> {{\n    {call}\n    out.println(\"done\")\n}}\n"
    );
    let outcome = assert_same(&Case::package(
        name,
        &[
            ("oska.toml", MANIFEST),
            ("src/main.osk", &main),
            ("src/util.osk", util),
        ],
    ));
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
    assert!(outcome.stderr.contains(expected), "{outcome:#?}");
    assert!(!outcome.stderr.contains("pkg."), "{outcome:#?}");
}

#[test]
fn a_broken_precondition_in_a_package_names_the_function_as_the_caller_writes_it() {
    assert_package_panic(
        "package-requires",
        "pub(pkg) fn half(n: Int) -> Int\n    requires n > 0\n{\n    n / 2\n}\n",
        "let n = util.half(0)\n    let _ = n",
        "precondition violated: `util.half` requires `n > 0`",
    );
}

#[test]
fn a_broken_postcondition_in_a_package_names_the_function_as_its_module_writes_it() {
    assert_package_panic(
        "package-ensures",
        "pub(pkg) fn half(n: Int) -> Int\n    ensures |r| r > 0\n{\n    n / 2\n}\n",
        "let n = util.half(1)\n    let _ = n",
        "postcondition violated: `half` ensures `r > 0`",
    );
}

#[test]
fn a_broken_precondition_through_a_function_value_in_a_package_names_the_function() {
    assert_package_panic(
        "package-requires-value",
        "pub(pkg) fn half(n: Int) -> Int\n    requires n > 0\n{\n    n / 2\n}\n\n\
         pub(pkg) fn apply(f: fn(Int) -> Int, n: Int) -> Int {\n    f(n)\n}\n",
        "let n = util.apply(util.half, 0)\n    let _ = n",
        "requires `n > 0`",
    );
}

#[test]
fn a_broken_precondition_through_a_function_value_called_in_another_module_names_it_there() {
    assert_package_panic(
        "package-requires-value-main",
        "pub(pkg) fn half(n: Int) -> Int\n    requires n > 0\n{\n    n / 2\n}\n",
        "let f = util.half\n    let n = f(0)\n    let _ = n",
        "precondition violated: `util.half` requires `n > 0`",
    );
}
