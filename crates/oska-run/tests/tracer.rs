//! The tracer: a capability's method called with a string literal prints,
//! and a program with checker diagnostics never starts.

mod common;

use common::{assert_refused, assert_runs};

#[test]
fn hello_prints_its_greeting() {
    assert_runs(
        include_str!("../../../examples/hello.osk"),
        &[],
        "Hello from Oska!\n",
    );
}

#[test]
fn a_program_with_a_checker_error_is_refused_and_prints_nothing() {
    assert_refused(
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")?\n    io.printline(\"there\")\n}\n",
        "E0228",
    );
}
