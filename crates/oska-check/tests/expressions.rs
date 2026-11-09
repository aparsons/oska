//! Expression checking (#36): function bodies are type-checked with local,
//! bidirectional inference.

mod common;

use common::{assert_errors, assert_ok};

// ---- Bindings and inference ------------------------------------------------

#[test]
fn a_let_type_is_inferred_from_its_initializer() {
    assert_ok("fn f() -> Int {\n    let x = 5\n    x\n}");
    assert_errors(
        "fn f() -> String {\n    let x = 5\n    x\n}",
        &[("E0220", "x")],
    );
}
