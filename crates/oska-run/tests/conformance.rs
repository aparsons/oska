//! Conformance (#67): the prelude's body-less `String` and trust methods
//! run, and the examples agree with the runtime.

mod common;

use common::{assert_runs, run_on};

fn main_with(body: &str) -> String {
    format!("fn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

#[test]
fn trim_removes_leading_and_trailing_whitespace() {
    assert_runs(
        &main_with("    let n = \"  hi there \\n\".trim()\n    io.println(\"[{n}]\")"),
        &[],
        "[hi there]\n",
    );
}

#[test]
fn split_returns_the_pieces_between_separators() {
    assert_runs(
        &main_with(
            "    let parts = \"a,b,,c\".split(\",\")\n    let n = parts.len()\n    io.println(\"{n}\")?\n    io.println(parts.at(2))?\n    io.println(parts.at(3))",
        ),
        &[],
        "4\n\nc\n",
    );
}

#[test]
fn contains_finds_a_substring() {
    assert_runs(
        &main_with(
            "    if \"hello\".contains(\"ell\") {\n        io.println(\"yes\")?\n    }\n    if \"hello\".contains(\"xyz\") {\n        io.println(\"no\")?\n    }\n    Ok(())",
        ),
        &[],
        "yes\n",
    );
}

#[test]
fn assume_trusted_returns_the_same_text() {
    let src = "fn main(io: Stdout, args: Args) -> Result<(), IoError> {
    match args.get(1) {
        Some(raw) => {
            let text: String = raw.assume_trusted()
            io.println(\"got {text}\")
        }
        None => io.println(\"none\")
    }
}
";
    let outcome = run_on(oska_run::FakeHost::new(), src, &["abc"]);
    assert_eq!(outcome.stdout, "got abc\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, 0);
    assert_runs(src, &[], "none\n");
}
