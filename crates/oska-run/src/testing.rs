//! `oskac test`: running a file's `test` blocks.
//!
//! [`test`] checks the file, refuses it as [`crate::run`] would, and then
//! runs each `test` block in source order, each in a fresh interpreter
//! with `t`, a `TestContext`, in slot 0:
//!
//! - A plain test runs on a fresh [`FakeHost`], so `t.fs` is an empty
//!   in-memory filesystem, `t.clock` is fixed at the Unix epoch, and
//!   `t.random` starts from [`crate::FAKE_SEED`]. Nothing one test does is
//!   seen by another, and the same file gives the same report every time.
//! - An `#[integration]` test runs only when [`TestOptions::integration`]
//!   is set, on the host [`test`] was given: the real OS under `oskac
//!   test --integration`. Its stdout is still recorded, by [`Recording`],
//!   so `t.printed()` works and the report stays the only thing printed.
//!
//! An assertion that fails panics with its own `P`-code (`P0010` to
//! `P0013`). A panic, from an assertion or a bug, stops only its own test,
//! which is reported failed with the panic as its failure.

use std::io;
use std::path::{Path, PathBuf};

use oska_check::program::{Program, Test};
use oska_syntax::{json_escape, Diagnostic, Span};

use crate::host::{FakeHost, Host, Output};
use crate::interp::Interpreter;
use crate::intrinsics::{debug, HigherOrder, Intrinsic};
use crate::panics::{codes, Panic};
use crate::value::{Capability, Value};
use crate::{support, unimplemented_module, UNSUPPORTED_EXIT_CODE};

/// What `oskac test` was asked to run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestOptions {
    /// Also run the `#[integration]` tests, on the given host.
    pub integration: bool,
}

/// How one test ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    /// An assertion failed, or the test panicked.
    Failed,
    /// An `#[integration]` test, not run without
    /// [`TestOptions::integration`].
    Skipped,
}

impl TestStatus {
    /// The status as the report prints it: `passed`, `failed`, `skipped`.
    pub fn as_str(self) -> &'static str {
        match self {
            TestStatus::Passed => "passed",
            TestStatus::Failed => "failed",
            TestStatus::Skipped => "skipped",
        }
    }
}

/// The result of one test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    /// The name as written.
    pub name: String,
    pub status: TestStatus,
    /// Why a failed test failed: the panic that stopped it, shaped like a
    /// diagnostic, with a `P`-code and the span where it happened.
    pub failure: Option<Diagnostic>,
}

/// The result of running a file's tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestReport {
    /// Every test, in source order. Empty when the file was refused.
    pub tests: Vec<TestResult>,
    /// The diagnostics that refused the file before any test ran: the
    /// checker's, or the runtime's (`E0371`, `E0380`).
    pub diagnostics: Vec<Diagnostic>,
}

impl TestReport {
    /// How many tests ended with `status`.
    pub fn count(&self, status: TestStatus) -> usize {
        self.tests.iter().filter(|t| t.status == status).count()
    }

    /// The exit code of `oskac test`: 0 when no test failed, 1 when one
    /// did or the checker refused the file, and 2 when the runtime
    /// refused it (`docs/runtime.md`).
    pub fn exit_code(&self) -> i32 {
        if self
            .diagnostics
            .iter()
            .any(|d| codes::ALL.contains(&d.code))
        {
            UNSUPPORTED_EXIT_CODE
        } else if !self.diagnostics.is_empty() || self.count(TestStatus::Failed) > 0 {
            1
        } else {
            0
        }
    }

    /// The report as one JSON object:
    /// `{"tests":[{"name":...,"status":...,"failure":...}],
    /// "passed":n,"failed":n,"skipped":n,"diagnostics":[...]}`. A test's
    /// `failure` is a diagnostic object, or `null` when it did not fail.
    pub fn to_json(&self) -> String {
        let tests: Vec<String> = self
            .tests
            .iter()
            .map(|t| {
                format!(
                    "{{\"name\":\"{}\",\"status\":\"{}\",\"failure\":{}}}",
                    json_escape(&t.name),
                    t.status.as_str(),
                    t.failure
                        .as_ref()
                        .map_or_else(|| "null".to_string(), Diagnostic::to_json)
                )
            })
            .collect();
        let diagnostics: Vec<String> = self.diagnostics.iter().map(Diagnostic::to_json).collect();
        format!(
            "{{\"tests\":[{}],\"passed\":{},\"failed\":{},\"skipped\":{},\"diagnostics\":[{}]}}",
            tests.join(","),
            self.count(TestStatus::Passed),
            self.count(TestStatus::Failed),
            self.count(TestStatus::Skipped),
            diagnostics.join(",")
        )
    }
}

/// Check `src` and run its `test` blocks: the plain ones each on a fresh
/// [`FakeHost`], and the `#[integration]` ones, only when `options` asks
/// for them, on `host`.
pub fn test(src: &str, host: &mut dyn Host, options: TestOptions) -> TestReport {
    let report = oska_check::check_module(src);
    let Some(program) = report.program else {
        return TestReport {
            tests: Vec::new(),
            diagnostics: report.diagnostics,
        };
    };
    if let Some(refusal) =
        unimplemented_module(&program).or_else(|| support::unsupported_with_tests(&program))
    {
        return TestReport {
            tests: Vec::new(),
            diagnostics: vec![refusal],
        };
    }
    let tests = program
        .tests
        .iter()
        .map(|test| {
            let outcome = match (test.integration, options.integration) {
                (false, _) => Some(run_one(&program, test, &mut FakeHost::new())),
                (true, true) => Some(run_one(&program, test, &mut Recording::new(host))),
                (true, false) => None,
            };
            let (status, failure) = match outcome {
                None => (TestStatus::Skipped, None),
                Some(Ok(())) => (TestStatus::Passed, None),
                Some(Err(panic)) => (TestStatus::Failed, Some(panic.into_diagnostic())),
            };
            TestResult {
                name: test.name.clone(),
                status,
                failure,
            }
        })
        .collect();
    TestReport {
        tests,
        diagnostics: Vec::new(),
    }
}

/// Run one test on `host`, in an interpreter of its own.
fn run_one(program: &Program, test: &Test, host: &mut dyn Host) -> Result<(), Panic> {
    Interpreter::new(program, host).run_test(&test.body, test_context())
}

/// `t`: a `TestContext`, its fields in the prelude's order, `fs`,
/// `clock`, `random`, `stdout`. Which world they reach is the host's
/// business.
fn test_context() -> Value {
    let field = |ty| Value::Capability(Capability::granted(ty).expect("a built-in capability"));
    Value::Struct {
        ty: "TestContext".into(),
        fields: std::rc::Rc::new(vec![
            field("Fs"),
            field("Clock"),
            field("Random"),
            field("Stdout"),
        ]),
    }
}

/// A host that records stdout and passes everything else on to another:
/// the host of an `#[integration]` test.
struct Recording<'h> {
    inner: &'h mut dyn Host,
    printed: Vec<String>,
}

impl<'h> Recording<'h> {
    fn new(inner: &'h mut dyn Host) -> Self {
        Recording {
            inner,
            printed: Vec::new(),
        }
    }
}

impl Host for Recording<'_> {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        self.printed
            .push(text.strip_suffix('\n').unwrap_or(text).to_string());
        Ok(())
    }

    fn printed(&self) -> Vec<String> {
        self.printed.clone()
    }

    fn write_stderr(&mut self, text: &str) -> io::Result<()> {
        self.inner.write_stderr(text)
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        self.inner.read_line()
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        self.inner.env_var(name)
    }

    fn now_millis(&mut self) -> i128 {
        self.inner.now_millis()
    }

    fn next_random(&mut self) -> u64 {
        self.inner.next_random()
    }

    fn canonicalize(&mut self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize(path)
    }

    fn is_symlink(&mut self, path: &Path) -> bool {
        self.inner.is_symlink(path)
    }

    fn read_file(&mut self, path: &Path) -> io::Result<String> {
        self.inner.read_file(path)
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        self.inner.write_file(path, contents)
    }

    fn take_output(&mut self) -> Output {
        self.inner.take_output()
    }
}

// ---- `TestContext` intrinsics ------------------------------------------

/// The `TestContext` methods that need only the host.
pub const INTRINSICS: &[(&str, Intrinsic)] = &[
    ("TestContext.assert", assert),
    ("TestContext.fail", fail),
    ("TestContext.printed", printed),
];

/// The `TestContext` methods that compare through `Eq`, which may be the
/// program's own impl.
pub const HIGHER_ORDER: &[(&str, HigherOrder)] = &[
    ("TestContext.assert_eq", assert_eq),
    ("TestContext.assert_ne", assert_ne),
];

/// `t.assert(cond)`: fails the test when `cond` is false.
fn assert(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    if args[1].as_bool() {
        return Ok(Value::Unit);
    }
    Err(Panic::new(
        codes::ASSERT_FAILED,
        "assertion failed: the condition is false",
        span,
    ))
}

/// `t.fail(message)`: fails the test with `message`.
fn fail(_: &mut dyn Host, args: Vec<Value>, span: Span) -> Result<Value, Panic> {
    Err(Panic::new(codes::TEST_FAILED, args[1].as_str(), span))
}

/// `t.printed()`: every line printed to `t.stdout` so far, one per
/// `println`.
fn printed(host: &mut dyn Host, _: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let lines = host.printed().iter().map(|l| Value::str(l)).collect();
    Ok(Value::List(std::rc::Rc::new(lines)))
}

/// `t.assert_eq(actual, expected)`: fails the test when they differ.
fn assert_eq(
    interp: &mut Interpreter<'_, '_>,
    mut args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let expected = args.pop().expect("`expected`");
    let actual = args.pop().expect("`actual`");
    if equal(interp, &actual, &expected, span)? {
        return Ok(Value::Unit);
    }
    Err(Panic::new(
        codes::ASSERT_EQ_FAILED,
        format!(
            "assertion failed: `actual == expected`: actual {}, expected {}",
            shown(&actual),
            shown(&expected)
        ),
        span,
    ))
}

/// `t.assert_ne(actual, other)`: fails the test when they are equal.
fn assert_ne(
    interp: &mut Interpreter<'_, '_>,
    mut args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let other = args.pop().expect("`other`");
    let actual = args.pop().expect("`actual`");
    if !equal(interp, &actual, &other, span)? {
        return Ok(Value::Unit);
    }
    Err(Panic::new(
        codes::ASSERT_NE_FAILED,
        format!(
            "assertion failed: `actual != other`: both are {}",
            shown(&actual)
        ),
        span,
    ))
}

/// Whether `a` and `b` are equal through `Eq` for their type.
fn equal(
    interp: &mut Interpreter<'_, '_>,
    a: &Value,
    b: &Value,
    span: Span,
) -> Result<bool, Panic> {
    Ok(interp
        .call_trait("Eq", "eq", a.clone(), vec![b.clone()], span)?
        .as_bool())
}

/// `value` as a failure message shows it: as `dbg` does, so a secret is
/// `<redacted>`.
fn shown(value: &Value) -> String {
    let mut out = String::new();
    debug(value, &mut out);
    out
}
