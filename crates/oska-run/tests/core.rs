//! Core execution (#61): integers, bindings, control flow, calls, and
//! panics.

mod common;

use common::assert_panics;

/// A `main` that runs `body` with `io: Stdout` in scope.
fn main_with(body: &str) -> String {
    format!("fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

#[test]
fn reaching_todo_panics_with_p0003_at_the_call() {
    assert_panics(
        &main_with("    let n: Int = todo()\n    io.println(\"unreachable\")"),
        "P0003",
        "todo()",
    );
}
