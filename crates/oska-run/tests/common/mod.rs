//! Helpers shared by the `oska-run` integration tests.
//!
//! Every test runs a small Oska program through [`oska_run::run`] on a
//! [`FakeHost`] and asserts on what a user sees: what the program printed,
//! its exit code, its panic, or the diagnostics that refused it.

#![allow(dead_code)]

use oska_run::{
    codes, run, test, FakeHost, Host, RunOutcome, TestOptions, TestReport, UNSUPPORTED_EXIT_CODE,
};

/// The program path every helper passes as `args[0]`.
pub const PROGRAM_PATH: &str = "prog.osk";

/// Run `src` on a fresh [`FakeHost`], with `args` after the program path.
pub fn run_fake(src: &str, args: &[&str]) -> RunOutcome {
    run_on(FakeHost::new(), src, args)
}

/// Run `src` on `host`, with `args` after the program path.
pub fn run_on(mut host: FakeHost, src: &str, args: &[&str]) -> RunOutcome {
    let args: Vec<String> = std::iter::once(PROGRAM_PATH)
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    run(src, &mut host, &args)
}

/// Whether `code` is one the runtime refuses a checked program with,
/// rather than one of the checker's.
fn is_runtime_refusal(code: &str) -> bool {
    codes::ALL.contains(&code) && code.starts_with('E')
}

/// Assert that `src` runs to completion with exit code 0, and that it
/// printed exactly `expected_stdout`.
pub fn assert_runs(src: &str, args: &[&str], expected_stdout: &str) {
    let outcome = run_fake(src, args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src:?} should run cleanly: {outcome:#?}"
    );
    assert_eq!(outcome.stdout, expected_stdout, "stdout of {src:?}");
}

/// Assert that `src` is refused before it starts, with a diagnostic
/// `code`, and that it printed nothing. A checker code exits with 1; a
/// runtime refusal (`E0370`, `E0380`, ...) of a program that checks
/// exits with 2.
pub fn assert_refused(src: &str, code: &str) {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.iter().any(|d| d.code == code),
        "{src:?} should be refused with {code}: {outcome:#?}"
    );
    let exit_code = if is_runtime_refusal(code) {
        UNSUPPORTED_EXIT_CODE
    } else {
        1
    };
    assert_eq!(
        outcome.exit_code, exit_code,
        "the exit code refusing {code}"
    );
    assert_eq!(outcome.stdout, "", "a refused program prints nothing");
    assert!(outcome.panic.is_none(), "a refused program never panics");
}

/// Assert that `src` stops with a panic `code` whose span covers exactly
/// `spanned_text`, and that it exits with code 101.
pub fn assert_panics(src: &str, code: &str, spanned_text: &str) {
    let outcome = run_fake(src, &[]);
    let Some(panic) = &outcome.panic else {
        panic!("{src:?} should panic with {code}: {outcome:#?}");
    };
    assert_eq!(panic.code, code, "the panic code of {src:?}: {panic:#?}");
    assert_eq!(
        &src[panic.span.start..panic.span.end],
        spanned_text,
        "the text the panic {code} spans"
    );
    assert_eq!(outcome.exit_code, 101, "a panic exits with code 101");
}

/// Assert that `src` runs to the end with exit code `code`, without a
/// panic, and that it wrote exactly `expected_stderr` to stderr.
pub fn assert_exits(src: &str, code: i32, expected_stderr: &str) {
    let outcome = run_fake(src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "{src:?} should run without a panic: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, code, "the exit code of {src:?}");
    assert_eq!(outcome.stderr, expected_stderr, "stderr of {src:?}");
}

/// Run the `test` blocks of `src`, the plain ones on fresh [`FakeHost`]s.
pub fn test_fake(src: &str) -> TestReport {
    test_with(FakeHost::new(), src, TestOptions::default())
}

/// Run the `test` blocks of `src` with `options`; `host` backs the
/// `#[integration]` tests when they run.
pub fn test_with(mut host: FakeHost, src: &str, options: TestOptions) -> TestReport {
    test(src, &mut host, options)
}

// ---- Filesystem capability programs --------------------------------------------------

/// Oska helpers a filesystem test program may call, to print what an
/// operation returned: its text (or `done`, or the names joined with
/// `,`), or `REFUSED: ` and its error's message.
pub const SHOWN: &str = r#"
fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// The source of a program whose `main` runs `body`, each line one
/// `io.println`, followed by [`SHOWN`]. `main` is given `io: Stdout`,
/// `fs: Fs`, `read: FsRead`, `write: FsWrite`, and `args: Args`.
pub fn fs_program(body: &str) -> String {
    format!(
        "fn main(io: Stdout, fs: Fs, read: FsRead, write: FsWrite, args: Args) \
         -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{SHOWN}"
    )
}

/// What the [`fs_program`] running `body` prints on `host`, with `args`
/// after the program path. The host is left as the program left it.
pub fn printed_with(host: &mut dyn Host, args: &[&str], body: &str) -> String {
    let src = fs_program(body);
    let args: Vec<String> = std::iter::once(PROGRAM_PATH)
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    let outcome = run(&src, host, &args);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// What the [`fs_program`] running `body` prints on `host`, with no
/// arguments.
pub fn printed(mut host: FakeHost, body: &str) -> String {
    printed_with(&mut host, &[], body)
}

// ---- Real directories ------------------------------------------------------------------

/// `path` as the inside of an Oska string literal holds it: with each `\`
/// of a Windows path escaped.
pub fn literal(path: impl AsRef<std::ffi::OsStr>) -> String {
    std::path::Path::new(path.as_ref())
        .display()
        .to_string()
        .replace('\\', "\\\\")
}

/// A fresh directory under the system temp dir, holding `root/` and
/// `report/`, removed on drop. [`TempTree::saw`] runs a program on
/// [`oska_run::RealHost`] with capabilities on `root`, and reads what it
/// saw back from `report`, since a real host prints to the terminal.
pub struct TempTree {
    base: std::path::PathBuf,
}

impl TempTree {
    pub fn new(test: &str) -> TempTree {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "oska-{test}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        for dir in ["root", "report"] {
            std::fs::create_dir_all(base.join(dir)).expect("create a directory");
        }
        let base = std::fs::canonicalize(&base).expect("canonicalize the tree");
        TempTree { base }
    }

    /// The absolute path of `at` beneath the tree.
    pub fn path(&self, at: &str) -> std::path::PathBuf {
        self.base.join(at)
    }

    /// Write `text` to the file `at`, creating its directory.
    pub fn write(&self, at: &str, text: &str) {
        let path = self.path(at);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create a directory");
        }
        std::fs::write(path, text).expect("write a file");
    }

    /// The text of the file at `at`.
    pub fn file(&self, at: &str) -> String {
        std::fs::read_to_string(self.path(at)).expect("read a file")
    }

    /// What `body` saw on a real host, each `say(...)` one line. `main` is
    /// given `read: DirRead` and `write: DirWrite` on `root`, and `fs: Fs`
    /// for more, with `{root}` in `body` the root's absolute path. The
    /// helpers in [`SHOWN`] are in scope.
    pub fn saw(&self, body: &str) -> String {
        let literal = |at: &str| literal(self.path(at));
        let root = literal("root");
        let src = format!(
            "fn main(fs: Fs) -> Result<(), IoError> {{\n\
             \x20   let read = fs.read_only().restrict_to(\"{root}\")\n\
             \x20   let write = fs.write_only().restrict_to(\"{root}\")\n\
             \x20   var seen: Untrusted<String> = \"\"\n\
             {body}\n\
             \x20   fs.write_only().restrict_to(\"{report}\").write(Path.new(\"seen.txt\"), seen)\n\
             }}\n\
             \n\
             fn said(seen: Untrusted<String>, line: Untrusted<String>) -> Untrusted<String> {{\n\
             \x20   \"{{seen}}\\n{{line}}\"\n\
             }}\n{SHOWN}",
            report = literal("report"),
            body = body
                .replace("say(", "seen = said(seen, ")
                .replace("{root}", &root),
        );
        let args = [PROGRAM_PATH.to_string()];
        let outcome = run(&src, &mut oska_run::RealHost::new(), &args);
        assert!(
            outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
            "{src} should run: {outcome:#?}"
        );
        self.file("report/seen.txt")
            .trim_start_matches('\n')
            .to_string()
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}
