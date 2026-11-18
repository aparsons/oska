//! Module-level constants (#83, spec #184), run on both backends through
//! the differential harness: a constant used in functions, methods,
//! interpolations, other constants, and a test gives the same value
//! under `oskac run --interpret` and a native executable.

#![cfg(any(
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
))]

mod differential;

use differential::{assert_same, Case};

#[test]
fn constants_give_the_same_value_on_both_backends() {
    let src = r#"const LIMIT: Int = 5
const NAME: String = "oska"
const NEXT: Int = LIMIT + 1
const FLAG: Bool = LIMIT < NEXT

struct Meter { n: Int }

impl Meter {
    fn over(self) -> Bool {
        self.n > LIMIT
    }
}

fn describe() -> String {
    "{NAME} {LIMIT} {NEXT}"
}

fn main(io: Stdout) {
    let _ = io.println(NAME)
    let _ = io.println(describe())
    if FLAG {
        let _ = io.println("flag")
    }
    let m = Meter(n: 9)
    if m.over() {
        let _ = io.println("over")
    }
    let n = NEXT * 2
    let _ = io.println("{n}")
}
"#;
    let outcome = assert_same(&Case::file("module_consts", src));
    assert_eq!(
        outcome.stdout, "oska\noska 5 6\nflag\nover\n12\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn each_module_of_a_package_sees_its_own_constant() {
    let manifest = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
    let main = r#"use pkg.util

const LABEL: String = "main"

fn main(io: Stdout) {
    let _ = io.println(LABEL)
    let _ = io.println(util.label())
}
"#;
    let util = "const LABEL: String = \"util\"\n\npub(pkg) fn label() -> String {\n    LABEL\n}\n";
    let outcome = assert_same(&Case::package(
        "package_consts",
        &[
            ("oska.toml", manifest),
            ("src/main.osk", main),
            ("src/util.osk", util),
        ],
    ));
    assert_eq!(outcome.stdout, "main\nutil\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}
