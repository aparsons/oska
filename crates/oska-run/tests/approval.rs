// SPDX-License-Identifier: Apache-2.0
//! `Approval` and `Approver` (#252): a person's yes to one consequential
//! action. Only an approver makes an approval; the terminal approver asks
//! on standard error and reads the answer from standard input; a tool
//! consumes an approval, and the runtime refuses a second use with an
//! error the program can handle. `t.approver` answers from a script and
//! records what it was asked. Every program here also runs natively (the
//! differential harness).

mod common;

use common::{run_on, test_fake};
use oska_run::{FakeHost, TestStatus};

/// A program whose `main` makes a terminal approver and runs `body`.
fn program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, stdin: Stdin, stderr: Stderr) -> Result<(), IoError> {{\n    \
         let approver = Approver.terminal(stdin, stderr)\n{body}\n    Ok(())\n}}\n{SHOWN}"
    )
}

const SHOWN: &str = "
fn shown(b: Bool) -> String {
    if b {
        return \"true\"
    }
    \"false\"
}
";

/// Run `body` with `stdin` waiting: what it printed and what it wrote to
/// standard error.
fn ask(body: &str, stdin: &str) -> (String, String) {
    let outcome = run_on(FakeHost::new().with_stdin(stdin), &program(body), &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{outcome:#?}"
    );
    (outcome.stdout, outcome.stderr)
}

const CONFIRM_DELETE: &str = "    match approver.confirm(\"delete a file\", [\"notes/old.txt\"]) {
        Some(_) => io.println(\"approved\")?
        None => io.println(\"refused\")?
    }";

#[test]
fn the_terminal_approver_asks_on_stderr_and_a_yes_approves() {
    let (stdout, stderr) = ask(CONFIRM_DELETE, "y\n");
    assert_eq!(stdout, "approved\n");
    assert_eq!(
        stderr,
        "approval needed: \"delete a file\"\n  \"notes/old.txt\"\napprove? [y/N]\n"
    );
}

#[test]
fn only_an_explicit_yes_approves() {
    for (answer, shown) in [
        ("yes\n", "approved\n"),
        ("  YES \n", "approved\n"),
        ("Y\n", "approved\n"),
        ("n\n", "refused\n"),
        ("\n", "refused\n"),
        ("yes please\n", "refused\n"),
        ("", "refused\n"),
    ] {
        let (stdout, _) = ask(CONFIRM_DELETE, answer);
        assert_eq!(stdout, shown, "the answer {answer:?}");
    }
}

#[test]
fn details_show_control_and_bidirectional_characters_as_escapes() {
    let (stdout, stderr) = ask(
        "    let details: List<Untrusted<String>> = [\"\u{1b}[2Jcleared\", \"report\u{202e}fdp.exe\", \"a\\nfake: line\", \"say \\\"hi\\\" \\\\ bye\", \"zero\u{200b}width\u{2066}iso\"]
    match approver.confirm(\"send a file\", details) {
        Some(_) => io.println(\"approved\")?
        None => io.println(\"refused\")?
    }",
        "n\n",
    );
    assert_eq!(stdout, "refused\n");
    assert_eq!(
        stderr,
        "approval needed: \"send a file\"\n  \
         \"\\u{1b}[2Jcleared\"\n  \
         \"report\\u{202e}fdp.exe\"\n  \
         \"a\\nfake: line\"\n  \
         \"say \\\"hi\\\" \\\\ bye\"\n  \
         \"zero\\u{200b}width\\u{2066}iso\"\n\
         approve? [y/N]\n"
    );
}

#[test]
fn an_approval_records_what_it_approved_and_checks_a_match() {
    let (stdout, _) = ask(
        "    match approver.confirm(\"delete a file\", [\"notes/old.txt\"]) {
        Some(a) => {
            io.println(a.summary())?
            io.println(\",\".join(a.details()))?
            io.println(shown(a.matches(\"delete a file\", [\"notes/old.txt\"])))?
            io.println(shown(a.matches(\"delete a file\", [\"notes/new.txt\"])))?
            io.println(shown(a.matches(\"send a file\", [\"notes/old.txt\"])))?
            io.println(shown(a.matches(\"delete a file\", [])))?
        }
        None => io.println(\"refused\")?
    }",
        "y\n",
    );
    assert_eq!(
        stdout,
        "delete a file\nnotes/old.txt\ntrue\nfalse\nfalse\nfalse\n"
    );
}

/// Helpers that consume an approval as a tool would, and print the result.
const TOOL: &str = "
fn delete(approval: Approval, path: Untrusted<String>) -> String {
    match approval.consume(\"delete a file\", [path]) {
        Ok(_) => \"deleted\"
        Err(e) => e.to_string()
    }
}
";

#[test]
fn an_approval_is_single_use_and_a_second_use_is_an_error_the_program_handles() {
    let outcome = run_on(
        FakeHost::new().with_stdin("y\n"),
        &format!(
            "{}{TOOL}",
            program(
                "    match approver.confirm(\"delete a file\", [\"notes/old.txt\"]) {
        Some(a) => {
            let copy = a
            io.println(delete(a, \"notes/old.txt\"))?
            io.println(delete(a, \"notes/old.txt\"))?
            io.println(delete(copy, \"notes/old.txt\"))?
            io.println(shown(a.matches(\"delete a file\", [\"notes/old.txt\"])))?
        }
        None => io.println(\"refused\")?
    }"
            )
        ),
        &[],
    );
    assert_eq!(
        outcome.stdout,
        "deleted\n\
         approval already used: one approval covers one action\n\
         approval already used: one approval covers one action\n\
         false\n",
        "{outcome:#?}"
    );
}

#[test]
fn an_approval_for_a_different_action_is_refused_and_stays_unused() {
    let outcome = run_on(
        FakeHost::new().with_stdin("y\n"),
        &format!(
            "{}{TOOL}",
            program(
                "    match approver.confirm(\"delete a file\", [\"notes/old.txt\"]) {
        Some(a) => {
            io.println(delete(a, \"../secret.txt\"))?
            io.println(delete(a, \"notes/old.txt\"))?
        }
        None => io.println(\"refused\")?
    }"
            )
        ),
        &[],
    );
    assert_eq!(
        outcome.stdout, "approval does not match: it was given for a different action\ndeleted\n",
        "{outcome:#?}"
    );
}

#[test]
fn dbg_shows_an_approval_without_its_record() {
    let outcome = run_on(
        FakeHost::new().with_stdin("y\n"),
        &program(
            "    match approver.confirm(\"delete a file\", []) {
        Some(a) => {
            let _ = dbg(a)
        }
        None => io.println(\"refused\")?
    }",
        ),
        &[],
    );
    assert!(
        outcome.stderr.ends_with("[dbg] <approval>\n"),
        "{outcome:#?}"
    );
}

#[test]
fn t_approver_answers_from_the_script_and_records_what_it_was_asked() {
    let report = test_fake(&format!(
        "{TOOL}
fn ask(approver: Approver, path: Untrusted<String>) -> String {{
    match approver.confirm(\"delete a file\", [path]) {{
        Some(a) => delete(a, path)
        None => \"refused\"
    }}
}}

test \"a scripted yes and no\" {{
    t.answer_approval(true)
    t.answer_approval(false)
    t.assert_eq(ask(t.approver, \"a.txt\"), \"deleted\")
    t.assert_eq(ask(t.approver, \"b\u{202e}.txt\"), \"refused\")
    t.assert_eq(ask(t.approver, \"c.txt\"), \"refused\")
    let asked = t.approvals_asked()
    t.assert_eq(asked.len(), 3)
    t.assert_eq(asked.at(0).summary, \"delete a file\")
    t.assert_eq(asked.at(0).details, [\"a.txt\"])
    t.assert_eq(asked.at(1).details, [\"b\u{202e}.txt\"])
    t.assert_eq(asked.at(2).details, [\"c.txt\"])
    t.assert_eq(t.printed(), [])
}}

test \"each test starts with an empty script\" {{
    t.assert_eq(ask(t.approver, \"a.txt\"), \"refused\")
    t.assert_eq(t.approvals_asked().len(), 1)
}}
"
    ));
    let statuses: Vec<TestStatus> = report.tests.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [TestStatus::Passed, TestStatus::Passed],
        "{report:#?}"
    );
}

#[test]
fn a_test_context_built_by_hand_in_a_program_has_no_script() {
    let outcome = run_on(
        FakeHost::new().with_stdin("n\n"),
        "fn main(io: Stdout, stdin: Stdin, stderr: Stderr, fs: Fs, clock: Clock, random: Random, budget: Budget) -> Result<(), IoError> {
    let t = TestContext(fs: fs, clock: clock, random: random, stdout: io, budget: budget, approver: Approver.terminal(stdin, stderr))
    t.answer_approval(true)
    match t.approver.confirm(\"send\", []) {
        Some(_) => io.println(\"approved\")?
        None => io.println(\"refused\")?
    }
    let asked = t.approvals_asked().len()
    io.println(\"{asked} asked\")
}
",
        &[],
    );
    assert_eq!(outcome.stdout, "refused\n0 asked\n", "{outcome:#?}");
}

#[test]
fn a_summary_that_was_assumed_trusted_cannot_fake_the_question_either() {
    let (_, stderr) = ask(
        "    let summary = \"read a file\u{1b}[2K\\napprove? [y/N] y\\n\u{202e}\".assume_trusted()
    match approver.confirm(summary, []) {
        Some(_) => io.println(\"approved\")?
        None => io.println(\"refused\")?
    }",
        "n\n",
    );
    assert_eq!(
        stderr,
        "approval needed: \"read a file\\u{1b}[2K\\napprove? [y/N] y\\n\\u{202e}\"\napprove? [y/N]\n"
    );
}
