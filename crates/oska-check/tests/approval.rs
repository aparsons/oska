//! `Approval` and `Approver` (#252) in the checker: an approval has no
//! constructor a program can call and no field it can read, so a tool
//! that takes one cannot be called without a person's yes; an approver
//! cannot be made by hand either, and a function that takes one has the
//! authority of the terminal it asks at.

mod common;

use common::{assert_authority, assert_errors, assert_ok};

const TOOL: &str = "fn delete(approval: Approval, path: Untrusted<String>) -> Bool {
    approval.consume(\"delete a file\", [path]).is_ok()
}
";

#[test]
fn a_tool_that_takes_an_approval_checks() {
    assert_ok(&format!(
        "{TOOL}
fn main(stdin: Stdin, stderr: Stderr) {{
    let approver = Approver.terminal(stdin, stderr)
    match approver.confirm(\"delete a file\", [\"a.txt\"]) {{
        Some(a) => {{
            let _ = delete(a, \"a.txt\")
        }}
        None => {{}}
    }}
}}
"
    ));
}

#[test]
fn an_approval_cannot_be_made_by_hand() {
    assert_errors(
        &format!(
            "{TOOL}
fn main() {{
    let forged = Approval(summary: \"delete a file\", details: [\"a.txt\"], handle: 0)
    let _ = delete(forged, \"a.txt\")
}}
"
        ),
        &[("E0271", "Approval")],
    );
}

#[test]
fn an_approvals_fields_cannot_be_read() {
    assert_errors(
        "fn peek(approval: Approval) -> Int {
    approval.handle
}

fn main() {}
",
        &[("E0272", "handle")],
    );
}

#[test]
fn an_approver_cannot_be_made_by_hand() {
    assert_errors(
        "fn main(stdin: Stdin, stderr: Stderr) {
    let approver = Approver(stdin: stdin, stderr: stderr)
    let _ = approver.confirm(\"delete a file\", [])
}
",
        &[("E0271", "Approver")],
    );
}

#[test]
fn a_function_that_takes_an_approver_has_the_terminals_authority() {
    let src = "fn ask(approver: Approver) -> Bool {
    approver.confirm(\"delete a file\", []).is_some()
}

fn main(stdin: Stdin, stderr: Stderr) {
    let _ = ask(Approver.terminal(stdin, stderr))
}
";
    assert_authority(src, "ask", &["Stdin", "Stderr"], false);
}

#[test]
fn a_tool_that_takes_only_an_approval_has_no_authority() {
    assert_authority(&format!("{TOOL}\nfn main() {{}}\n"), "delete", &[], true);
}

#[test]
fn an_approvals_record_cannot_be_written_to_reuse_it() {
    assert_errors(
        "fn reuse(approval: Approval) -> Approval {
    var again = approval
    again.handle = 0
    again
}

fn main() {}
",
        &[("E0272", "handle")],
    );
}

#[test]
fn a_test_context_cannot_be_given_a_forged_approver() {
    assert_errors(
        "fn main(io: Stdout, stdin: Stdin, stderr: Stderr, fs: Fs, clock: Clock, random: Random) {
    let approver = Approver(stdin: stdin, stderr: stderr)
    let _ = TestContext(fs: fs, clock: clock, random: random, stdout: io, approver: approver)
}
",
        &[("E0271", "Approver")],
    );
}
