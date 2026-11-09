//! Trust on text (#50): text from outside the program is untrusted, and
//! untrusted text never flows into a trusted `String` slot (`E0300`).

mod common;

use common::{assert_errors, assert_ok};

#[test]
fn untrusted_text_cannot_be_passed_to_a_string_parameter() {
    assert_errors(
        "fn greet(name: String) -> String {\n    name\n}\n\
         fn f(args: Args) -> Option<String> {\n    let arg = args.get(1)?\n    Some(greet(arg))\n}",
        &[("E0300", "arg")],
    );
}
