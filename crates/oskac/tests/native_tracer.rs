//! The native tracer (ticket #128): the first programs `oskac build`
//! compiles, run on both backends through the differential harness
//! (seam 1). Native executables run only on aarch64 Linux, so these tests
//! exist only there; `build_golden.rs` covers `oskac build` everywhere.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

#[test]
fn hello_world_prints_the_same_on_both_backends() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/hello.osk"
    ))
    .expect("the hello example should exist");
    let outcome = assert_same(&Case::file("hello", &src));
    assert_eq!(outcome.stdout, "Hello from Oska!\n");
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn a_main_without_a_result_exits_0_after_printing_to_both_streams() {
    let src = "fn main(out: Stdout, err: Stderr) {\n    \
               let _ = out.println(\"to stdout\")\n    \
               let _ = err.println(\"to stderr, ünïcode and all\")\n    \
               let _ = out.println(\"\")\n}\n";
    let outcome = assert_same(&Case::file("both_streams", src));
    assert_eq!(outcome.stdout, "to stdout\n\n");
    assert_eq!(outcome.stderr, "to stderr, ünïcode and all\n");
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn an_err_from_main_prints_its_display_text_to_stderr_and_exits_1() {
    let src = "enum AppError {\n    Broken,\n    Lost,\n}\n\n\
               impl Display for AppError {\n    \
               fn to_string(self) -> String {\n        \"it broke\"\n    }\n}\n\n\
               fn main(io: Stdout) -> Result<(), AppError> {\n    \
               let _ = io.println(\"before\")\n    \
               Err(AppError.Lost)\n}\n";
    let outcome = assert_same(&Case::file("err_from_main", src));
    assert_eq!(outcome.stdout, "before\n");
    assert_eq!(outcome.stderr, "it broke\n");
    assert_eq!(outcome.exit_code, Some(1));
}

#[test]
fn a_long_line_is_written_in_full() {
    let line = "0123456789abcdef".repeat(8192);
    let src =
        format!("fn main(io: Stdout) -> Result<(), IoError> {{\n    io.println(\"{line}\")\n}}\n");
    let outcome = assert_same(&Case::file("long_line", &src));
    assert_eq!(outcome.stdout, format!("{line}\n"));
}

#[test]
fn a_package_builds_and_runs_the_same() {
    let outcome = assert_same(&Case::package(
        "package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/main.osk",
                "use pkg.greet\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    \
                 greet.hello(io)\n}\n",
            ),
            (
                "src/greet.osk",
                "pub fn hello(io: Stdout) -> Result<(), IoError> {\n    \
                 io.println(\"hello from a module\")\n}\n",
            ),
        ],
    ));
    assert_eq!(outcome.stdout, "hello from a module\n");
}
