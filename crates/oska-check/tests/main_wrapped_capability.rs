//! `E0217` names a directory or file capability declared bare by `main`,
//! such as `main(d: DirRead)`. One inside another type, such as
//! `Option<DirRead>`, is not matched by it, and gets the general `E0211`
//! for a parameter `main` cannot take instead (review item F11 of #93,
//! accepted as is). This pins that behaviour: either way `main` is
//! refused.

mod common;

use common::assert_errors;

#[test]
fn a_bare_directory_capability_on_main_is_e0217() {
    assert_errors("fn main(d: DirRead) {\n}", &[("E0217", "DirRead")]);
}

#[test]
fn a_directory_capability_in_an_option_on_main_is_e0211() {
    assert_errors(
        "fn main(d: Option<DirRead>) {\n}",
        &[("E0211", "Option<DirRead>")],
    );
}

#[test]
fn a_file_capability_in_a_list_on_main_is_e0211() {
    assert_errors(
        "fn main(files: List<FileRead>) {\n}",
        &[("E0211", "List<FileRead>")],
    );
}
