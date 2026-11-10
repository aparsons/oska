//! `oska_run::test`: running a file's `test` blocks, each against fresh
//! deterministic stand-ins, and reporting each one (ticket #66).

mod common;

use common::{test_fake, test_with};
use oska_run::{codes, FakeHost, Host, TestOptions, TestStatus};

#[test]
fn reports_each_test_as_passed_or_failed_in_source_order() {
    let report = test_fake(
        "test \"holds\" {\n    t.assert(1 + 1 == 2)\n}\n\
         test \"breaks\" {\n    t.assert(1 + 1 == 3)\n}\n",
    );
    let statuses: Vec<(&str, TestStatus)> = report
        .tests
        .iter()
        .map(|r| (r.name.as_str(), r.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("holds", TestStatus::Passed),
            ("breaks", TestStatus::Failed)
        ]
    );
    assert_eq!(report.exit_code(), 1, "a failed test fails the run");
}

#[test]
fn a_file_whose_tests_all_pass_exits_with_zero() {
    let report = test_with(
        FakeHost::new(),
        "test \"holds\" {\n    t.assert(true)\n}\n",
        TestOptions::default(),
    );
    assert_eq!(report.tests[0].status, TestStatus::Passed);
    assert!(report.tests[0].failure.is_none());
    assert_eq!(report.exit_code(), 0);
}

/// Assert that the only test in `src` fails with `code`, spanning exactly
/// `spanned_text`, and return its failure message.
fn assert_fails(src: &str, code: &str, spanned_text: &str) -> String {
    let report = test_fake(src);
    assert_eq!(report.tests.len(), 1, "{report:#?}");
    let result = &report.tests[0];
    assert_eq!(result.status, TestStatus::Failed, "{report:#?}");
    let failure = result
        .failure
        .as_ref()
        .expect("a failed test has a failure");
    assert_eq!(failure.code, code, "{failure:#?}");
    assert_eq!(&src[failure.span.start..failure.span.end], spanned_text);
    failure.message.clone()
}

#[test]
fn a_false_assert_fails_the_test_with_p0010_at_the_call() {
    assert_fails(
        "test \"t\" {\n    t.assert(1 > 2)\n}\n",
        codes::ASSERT_FAILED,
        "t.assert(1 > 2)",
    );
}

#[test]
fn assert_eq_shows_both_values() {
    let message = assert_fails(
        "test \"t\" {\n    t.assert_eq(1 + 1, 3)\n}\n",
        "P0011",
        "t.assert_eq(1 + 1, 3)",
    );
    assert!(message.contains("actual 2, expected 3"), "{message}");
}

#[test]
fn assert_eq_compares_through_the_programs_own_eq() {
    let src = "struct P { x: Int }\n\
               impl Eq for P {\n    fn eq(self, other: P) -> Bool { true }\n}\n\
               test \"t\" {\n    t.assert_eq(P(x: 1), P(x: 2))\n}\n";
    assert_eq!(test_fake(src).tests[0].status, TestStatus::Passed);
}

#[test]
fn assert_ne_fails_on_equal_values_with_p0012() {
    let message = assert_fails(
        "test \"t\" {\n    t.assert_ne(\"a\", \"a\")\n}\n",
        "P0012",
        "t.assert_ne(\"a\", \"a\")",
    );
    assert!(message.contains("\"a\""), "{message}");
}

#[test]
fn fail_fails_the_test_with_its_message_and_p0013() {
    let message = assert_fails(
        "test \"t\" {\n    t.fail(\"not yet\")\n}\n",
        "P0013",
        "t.fail(\"not yet\")",
    );
    assert_eq!(message, "not yet");
}

#[test]
fn a_failed_assertion_stops_the_rest_of_its_test() {
    let report = test_fake("test \"t\" {\n    t.fail(\"first\")\n    t.fail(\"second\")\n}\n");
    let failure = report.tests[0].failure.as_ref().expect("failed");
    assert_eq!(failure.message, "first");
}

/// Assert that every test in `src` passes.
fn assert_passes(src: &str) {
    let report = test_fake(src);
    assert!(
        !report.tests.is_empty() && report.tests.iter().all(|t| t.status == TestStatus::Passed),
        "{src}\n{report:#?}"
    );
}

#[test]
fn printed_returns_one_entry_per_println_to_t_stdout() {
    assert_passes(
        "test \"t\" {\n    let _ = t.stdout.println(\"one\")\n    let _ = t.stdout.println(\"two\")\n    \
         t.assert_eq(t.printed(), [\"one\", \"two\"])\n}\n",
    );
}

#[test]
fn printed_is_empty_before_anything_is_printed() {
    assert_passes("test \"t\" {\n    t.assert_eq(t.printed().len(), 0)\n}\n");
}

#[test]
fn t_clock_is_fixed_at_the_unix_epoch() {
    assert_passes("test \"t\" {\n    t.assert_eq(t.clock.now_millis(), 0)\n}\n");
}

#[test]
fn t_random_is_seeded_so_every_test_sees_the_same_sequence() {
    let src = "test \"a\" {\n    let n = t.random.next_int()\n    t.fail(\"{n}\")\n}\n\
               test \"b\" {\n    let n = t.random.next_int()\n    t.fail(\"{n}\")\n}\n";
    let report = test_fake(src);
    let first = &report.tests[0].failure.as_ref().expect("failed").message;
    let second = &report.tests[1].failure.as_ref().expect("failed").message;
    assert_eq!(first, second, "each test starts from the same seed");
    assert!(first.parse::<i64>().is_ok(), "{first}");
}

#[test]
fn t_fs_is_an_in_memory_filesystem_a_test_can_write_and_read_back() {
    assert_passes(
        "test \"t\" {\n    let path = Path.new(\"notes.txt\")\n    \
         let _ = t.fs.write_only().write(path, \"hi\")\n    \
         let text = t.fs.read_only().read_to_string(path).unwrap_or(\"missing\")\n    \
         t.assert_eq(text, \"hi\")\n}\n",
    );
}

#[test]
fn each_test_gets_a_fresh_filesystem_and_stdout() {
    let src =
        "test \"writes\" {\n    let _ = t.fs.write_only().write(Path.new(\"f.txt\"), \"x\")\n    \
               let _ = t.stdout.println(\"from writes\")\n}\n\
               test \"sees nothing\" {\n    \
               t.assert(!t.fs.read_only().read_to_string(Path.new(\"f.txt\")).is_ok())\n    \
               t.assert_eq(t.printed().len(), 0)\n}\n";
    assert_passes(src);
}

#[test]
fn a_panic_fails_only_its_own_test() {
    let src = "fn div(a: Int, b: Int) -> Int { a / b }\n\
               test \"divides by zero\" {\n    let _ = div(1, 0)\n}\n\
               test \"still runs\" {\n    t.assert_eq(div(4, 2), 2)\n}\n";
    let report = test_fake(src);
    let statuses: Vec<TestStatus> = report.tests.iter().map(|t| t.status).collect();
    assert_eq!(statuses, [TestStatus::Failed, TestStatus::Passed]);
    let failure = report.tests[0].failure.as_ref().expect("failed");
    assert_eq!(failure.code, codes::DIVISION_BY_ZERO);
    assert_eq!(&src[failure.span.start..failure.span.end], "a / b");
}

const INTEGRATION: &str = "#[integration]\ntest \"reads the real config\" {\n    \
    let text = t.fs.read_only().read_to_string(Path.new(\"config.txt\")).unwrap_or(\"none\")\n    \
    let _ = t.stdout.println(text)\n    \
    t.assert_eq(t.printed(), [\"real\"])\n}\n\
    test \"plain\" {\n    t.assert(true)\n}\n";

#[test]
fn integration_tests_are_skipped_unless_asked_for() {
    let report = test_with(
        FakeHost::new().with_file("config.txt", "real"),
        INTEGRATION,
        TestOptions::default(),
    );
    let statuses: Vec<TestStatus> = report.tests.iter().map(|t| t.status).collect();
    assert_eq!(statuses, [TestStatus::Skipped, TestStatus::Passed]);
    assert_eq!(
        report.exit_code(),
        0,
        "a skipped test does not fail the run"
    );
}

#[test]
fn integration_tests_run_on_the_given_host_when_asked_for() {
    let mut host = FakeHost::new().with_file("config.txt", "real");
    let report = oska_run::test(INTEGRATION, &mut host, TestOptions { integration: true });
    let statuses: Vec<TestStatus> = report.tests.iter().map(|t| t.status).collect();
    assert_eq!(
        statuses,
        [TestStatus::Passed, TestStatus::Passed],
        "{report:#?}"
    );
    assert_eq!(
        host.take_output().stdout,
        "",
        "an integration test's stdout is recorded, not written to the host"
    );
}

#[test]
fn plain_tests_never_see_the_given_host() {
    let src = "test \"t\" {\n    \
               t.assert(!t.fs.read_only().read_to_string(Path.new(\"config.txt\")).is_ok())\n}\n";
    let report = test_with(
        FakeHost::new().with_file("config.txt", "real"),
        src,
        TestOptions { integration: true },
    );
    assert_eq!(report.tests[0].status, TestStatus::Passed, "{report:#?}");
}

#[test]
fn running_the_same_file_twice_gives_identical_reports() {
    let src = "fn div(a: Int, b: Int) -> Int { a / b }\n\
               test \"random\" {\n    let n = t.random.next_int()\n    t.fail(\"{n}\")\n}\n\
               test \"clock\" {\n    t.assert_eq(t.clock.now_millis(), 0)\n}\n\
               test \"panics\" {\n    let _ = div(1, 0)\n}\n\
               test \"prints\" {\n    let _ = t.stdout.println(\"x\")\n    \
               t.assert_eq(t.printed(), [\"y\"])\n}\n";
    let first = test_fake(src);
    let second = test_fake(src);
    assert_eq!(first, second);
    assert_eq!(first.to_json(), second.to_json());
}

#[test]
fn a_file_the_checker_rejects_runs_no_test_and_exits_with_one() {
    let report = test_fake("test \"t\" {\n    t.assert(1)\n}\n");
    assert!(report.tests.is_empty());
    assert!(!report.diagnostics.is_empty());
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn a_test_using_something_the_interpreter_cannot_run_refuses_the_file() {
    let report = test_fake(
        "trait Tag {\n    fn tag(self) -> Int\n}\n\n\
         impl Tag for Int {\n    fn tag(self) -> Int { 1 }\n}\n\n\
         impl Tag for U8 {\n    fn tag(self) -> Int { 2 }\n}\n\n\
         fn show<T: Tag>(x: T) -> Int { x.tag() }\n\n\
         test \"t\" {\n    let b: U8 = 3\n    t.assert_eq(show(b), 2)\n}\n",
    );
    assert!(report.tests.is_empty());
    assert_eq!(report.diagnostics[0].code, codes::UNSUPPORTED);
    assert_eq!(report.exit_code(), 2);
}

#[test]
fn the_json_report_lists_each_test_with_its_status_and_failure() {
    let src = "test \"ok\" {\n    t.assert(true)\n}\ntest \"bad\" {\n    t.fail(\"no\")\n}\n";
    assert_eq!(
        test_fake(src).to_json(),
        "{\"tests\":[{\"name\":\"ok\",\"status\":\"passed\",\"failure\":null},\
         {\"name\":\"bad\",\"status\":\"failed\",\"failure\":\
         {\"code\":\"P0013\",\"message\":\"no\",\"span\":{\"start\":50,\"end\":62}}}],\
         \"passed\":1,\"failed\":1,\"skipped\":0,\"diagnostics\":[]}"
    );
}
