//! The Lean reference (ADR 0018, #425): every runtime test program that is
//! in Oska Core runs on the interpreter and on the Lean reference, which
//! runs its core IR with Oska Core's executable semantics
//! (`formal/lean/OskaFormal/Core/Eval.lean`) on its model of the fake host
//! (`Host.lean`), and the two must agree on what it wrote and how it ends:
//! its stdout, the files and directories it left on a [`FakeHost`] that
//! started empty, its panic's code and its exit code. A difference fails
//! the test, as a bug in one of the two.
//!
//! The runtime test programs are the string literals of this crate's
//! integration tests (`tests/**/*.rs`) that are whole programs: each
//! literal that holds `fn main` and checks. A program built at run time
//! (`format!`) is not among them. The export refuses each program outside
//! Oska Core, and the test prints how many programs it covered and how
//! many the export refused.
//!
//! The Lean reference is `formal/lean`'s `oska_core` executable; the test
//! builds it with `lake build oska_core`, finding `lake` on `PATH` or
//! where `scripts/formal/install.sh` puts it, as the formal step does.
//! Without Lean the test says so and skips, unless `OSKA_LEAN_REFERENCE`
//! is `required`: `scripts/check.sh` sets that in CI, where the formal
//! step installs Lean, so the gate cannot pass by skipping.

mod common;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use common::process::{run_bounded, Finished};
use oska_check::core_ir;
use oska_run::{FakeHost, FakeWorld, RunOutcome};

/// How long `lake build oska_core` may take: a first build compiles and
/// links the Lean reference, well under this.
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);

/// How long the Lean reference may run one program: far longer than any
/// takes. Running past it is a difference, never agreement.
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// The fewest runtime test programs the comparison must cover, as many as
/// it covered when it was written. Raise it as Oska Core grows, so that a
/// regression that makes the export refuse nearly everything fails.
const MIN_COVERED: usize = 36;

/// The program path a run gets as `args[0]`.
const PROGRAM_PATH: &str = "prog.osk";

// ---- The comparison ----------------------------------------------------

/// `text` quoted as the Lean reference quotes it (`quote` in
/// `formal/lean/OskaReference.lean`): `\\`, `"`, a line break, a tab and a
/// carriage return escaped, any other control character as `\u{HEX}`.
fn quoted(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A path of the fake host as the Lean reference shows it: `/` and its
/// components between `/`s, on every OS.
fn shown_path(path: &Path) -> String {
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    format!("/{}", names.join("/"))
}

/// What a run of a program in Oska Core wrote and how it ends, in the
/// form the Lean reference prints it: its stdout, each file and each
/// directory the [`FakeHost`] holds after it (the host started empty), each
/// sorted by path; then `result ()` and `exit 0` when `main` returns, or
/// `panic CODE` and `exit 101`. A program in Oska Core cannot write to
/// stderr, so anything there is shown too, and differs.
fn interpreted(outcome: &RunOutcome, world: &FakeWorld) -> String {
    let mut shown = String::new();
    if !outcome.stdout.is_empty() {
        shown.push_str(&format!("stdout {}\n", quoted(&outcome.stdout)));
    }
    if !outcome.stderr.is_empty() {
        shown.push_str(&format!("stderr {}\n", quoted(&outcome.stderr)));
    }
    let mut files: Vec<(String, &String)> = world
        .files
        .iter()
        .map(|(path, text)| (shown_path(path), text))
        .collect();
    files.sort();
    for (path, text) in files {
        shown.push_str(&format!("file {} {}\n", quoted(&path), quoted(text)));
    }
    let mut dirs: Vec<String> = world.dirs.iter().map(|d| shown_path(d)).collect();
    dirs.sort();
    for dir in dirs {
        shown.push_str(&format!("dir {}\n", quoted(&dir)));
    }
    match &outcome.panic {
        Some(panic) => shown.push_str(&format!("panic {}\n", panic.code)),
        // A `main` in Oska Core returns `()`, the only value an exit with
        // 0 can stand for.
        None if outcome.exit_code == 0 => shown.push_str("result ()\n"),
        None => {}
    }
    shown.push_str(&format!("exit {}\n", outcome.exit_code));
    shown
}

/// What became of one runtime test program.
#[derive(Debug)]
enum Verdict {
    /// The checker refused it, so it never ran.
    NotChecked,
    /// The core IR export refused it: it is outside Oska Core.
    Refused,
    /// The interpreter and the Lean reference agree on it.
    Agreed,
    /// They differ, or the Lean reference refused the core IR.
    Differs(String),
}

/// Run `src` on the interpreter, its result passed through `tamper` (the
/// identity, but for the test that proves a wrong result is caught), and
/// its core IR on the Lean reference `lean`, and compare the two.
fn verdict(
    src: &str,
    lean: &dyn Fn(&str) -> Result<String, String>,
    tamper: &dyn Fn(&mut RunOutcome),
) -> Verdict {
    let report = oska_check::check_module(src);
    let Some(program) = report.program else {
        return Verdict::NotChecked;
    };
    let ir = match core_ir::export(&program) {
        Ok(ir) => ir,
        Err(_) => return Verdict::Refused,
    };
    let mut host = FakeHost::new();
    let mut outcome = oska_run::run_program(&program, &mut host, &[PROGRAM_PATH.to_string()]);
    tamper(&mut outcome);
    let interpreter = interpreted(&outcome, &host.world());
    match lean(&ir) {
        Ok(reference) if reference == interpreter => Verdict::Agreed,
        Ok(reference) => Verdict::Differs(format!(
            "--- interpreter\n{interpreter}--- Lean reference\n{reference}"
        )),
        Err(error) => Verdict::Differs(format!(
            "the Lean reference did not run it: {error}\n--- core IR\n{ir}"
        )),
    }
}

// ---- The Lean reference ------------------------------------------------

/// `formal/lean`, the Lean project of Oska Core.
fn lean_project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../formal/lean")
}

/// `lake` on `PATH`, else where `scripts/formal/install.sh` puts it
/// (`$XDG_CACHE_HOME/oska/formal`, `~/.cache/oska/formal` by default).
fn lake() -> Option<PathBuf> {
    let name = if cfg!(windows) { "lake.exe" } else { "lake" };
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join(name));
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
        .map(|cache| cache.join("oska/formal/lean/bin").join(name));
    on_path.chain(cache).find(|candidate| candidate.is_file())
}

/// The Lean reference's executable, built now, or why there is none: the
/// reason to skip. A build that fails is not a reason to skip.
fn reference() -> &'static Result<PathBuf, String> {
    static REFERENCE: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        let Some(lake) = lake() else {
            return Err("Lean is not installed: no `lake` on PATH or in \
                        ~/.cache/oska/formal (scripts/formal/install.sh installs it)"
                .to_string());
        };
        let project = lean_project();
        // `lake` runs the `lean` beside it.
        let mut path = vec![lake.parent().map(Path::to_path_buf).unwrap_or_default()];
        path.extend(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths),
        );
        let mut build = Command::new(&lake);
        build
            .args(["build", "oska_core"])
            .current_dir(&project)
            .env("PATH", std::env::join_paths(path).expect("PATH joins"));
        let built = run_bounded(build, "", BUILD_TIMEOUT).unwrap_or_else(|_| {
            panic!("`lake build oska_core` ran for more than {BUILD_TIMEOUT:?}")
        });
        assert!(
            built.code == Some(0),
            "`lake build oska_core` failed in {}:\n{}{}",
            project.display(),
            built.stdout,
            built.stderr
        );
        let exe = if cfg!(windows) {
            "oska_core.exe"
        } else {
            "oska_core"
        };
        Ok(project.join(".lake/build/bin").join(exe))
    })
}

/// The Lean reference, or `None` after saying why the test skips. Where
/// the reference is required, a missing one fails the test instead.
fn reference_or_skip(test: &str) -> Option<&'static Path> {
    match reference() {
        Ok(exe) => Some(exe),
        Err(reason) if required() => {
            panic!("{test}: the Lean reference is required here (OSKA_LEAN_REFERENCE=required): {reason}")
        }
        Err(reason) => {
            say(&format!("{test}: skipped: {reason}"));
            None
        }
    }
}

/// Whether a missing Lean fails the test: `OSKA_LEAN_REFERENCE=required`,
/// which `scripts/check.sh` sets in CI.
fn required() -> bool {
    std::env::var("OSKA_LEAN_REFERENCE").is_ok_and(|v| v == "required")
}

/// Run the Lean reference `exe` on `input`, at most [`RUN_TIMEOUT`].
fn run_exe(exe: &Path, input: &str) -> Result<Finished, String> {
    run_bounded(Command::new(exe), input, RUN_TIMEOUT)
        .map_err(|_| format!("the Lean reference ran for more than {RUN_TIMEOUT:?}"))
}

/// Run the core IR `ir` on the Lean reference `exe`: what it printed, or
/// why it ran nothing (its exit code and what it said on stderr, or that
/// it ran out of time).
fn run_reference(exe: &Path, ir: &str) -> Result<String, String> {
    let run = run_exe(exe, ir)?;
    if run.code == Some(0) {
        Ok(run.stdout)
    } else {
        Err(format!(
            "exit {:?} with {}{}",
            run.code, run.stdout, run.stderr
        ))
    }
}

/// Print `line` to the test's stderr even when the harness captures
/// output, so a run always shows it.
fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

// ---- The runtime test programs -----------------------------------------

/// Every `.rs` file under `dir`, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The runtime test programs: each distinct string literal of this
/// crate's integration tests that holds `fn main`, with the file it is
/// in.
fn runtime_test_programs() -> Vec<(String, String)> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    rust_files(&tests, &mut files);
    let mut seen = BTreeSet::new();
    let mut programs = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let name = file
            .strip_prefix(&tests)
            .unwrap_or(&file)
            .display()
            .to_string();
        for literal in string_literals(&text) {
            if literal.contains("fn main") && seen.insert(literal.clone()) {
                programs.push((name.clone(), literal));
            }
        }
    }
    programs
}

/// The values of the string literals in the Rust source `src`: plain,
/// byte and raw strings, with their escapes resolved. Comments and
/// character literals are skipped.
fn string_literals(src: &str) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut literals = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c == '\'' {
            // A character literal, or a lifetime or label, which has no
            // closing quote.
            if next == Some('\\') {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let quote_or_hash = matches!(chars.get(i), Some('"') | Some('#'));
            if (word == "r" || word == "br") && quote_or_hash {
                if let Some((literal, end)) = raw_string(&chars, i) {
                    literals.push(literal);
                    i = end;
                }
            }
            // `b"..."` is read as a plain string next.
        } else if c == '"' {
            let (literal, end) = plain_string(&chars, i + 1);
            literals.push(literal);
            i = end;
        } else {
            i += 1;
        }
    }
    literals
}

/// The raw string whose `#`s or opening quote start at `at`, and the index
/// after it; `None` when `at` does not start one.
fn raw_string(chars: &[char], at: usize) -> Option<(String, usize)> {
    let mut i = at;
    let mut hashes = 0;
    while chars.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    if chars.get(i) != Some(&'"') {
        return None;
    }
    let start = i + 1;
    let mut j = start;
    while j < chars.len() {
        if chars[j] == '"' && (1..=hashes).all(|k| chars.get(j + k) == Some(&'#')) {
            return Some((chars[start..j].iter().collect(), j + 1 + hashes));
        }
        j += 1;
    }
    None
}

/// The plain string whose text starts at `start`, after its opening
/// quote, with its escapes resolved, and the index after its closing
/// quote.
fn plain_string(chars: &[char], start: usize) -> (String, usize) {
    let mut out = String::new();
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '"' => return (out, i + 1),
            '\\' => {
                i += 1;
                match chars.get(i) {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('0') => out.push('\0'),
                    Some('x') => {
                        let hex: String = chars[i + 1..(i + 3).min(chars.len())].iter().collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(c);
                        }
                        i += 2;
                    }
                    Some('u') => {
                        let close = chars[i..].iter().position(|&c| c == '}').map(|p| i + p);
                        if let Some(close) = close {
                            let hex: String = chars[i + 2..close].iter().collect();
                            if let Some(c) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                out.push(c);
                            }
                            i = close;
                        }
                    }
                    // A line continuation: the newline and the
                    // whitespace after it are not part of the string.
                    Some('\n') | Some('\r') => {
                        while chars.get(i + 1).is_some_and(|c| c.is_whitespace()) {
                            i += 1;
                        }
                    }
                    Some(&other) => out.push(other),
                    None => {}
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, i)
}

// ---- Tests ---------------------------------------------------------------

#[test]
fn every_runtime_test_program_in_oska_core_agrees_with_the_lean_reference() {
    let test = "lean_reference";
    let Some(exe) = reference_or_skip(test) else {
        return;
    };
    let lean = |ir: &str| run_reference(exe, ir);
    let (mut covered, mut refused, mut not_checked) = (0, 0, 0);
    let mut differences = Vec::new();
    for (file, src) in runtime_test_programs() {
        match verdict(&src, &lean, &|_| {}) {
            // A literal that holds `fn main` but does not check: a
            // fragment, a `format!` template, or a program the checker
            // refuses on purpose.
            Verdict::NotChecked => not_checked += 1,
            Verdict::Refused => refused += 1,
            Verdict::Agreed => covered += 1,
            Verdict::Differs(difference) => {
                covered += 1;
                differences.push(format!("in {file}:\n{src}\n{difference}"));
            }
        }
    }
    say(&format!(
        "{test}: {covered} runtime test program(s) covered, {refused} refused by the core IR export, \
         {not_checked} literal(s) with `fn main` that do not check"
    ));
    assert!(
        differences.is_empty(),
        "the interpreter and the Lean reference differ on {} program(s):\n\n{}",
        differences.len(),
        differences.join("\n\n")
    );
    assert!(
        covered >= MIN_COVERED,
        "only {covered} runtime test program(s) are in Oska Core, fewer than {MIN_COVERED}: \
         the corpus or the export is broken"
    );
}

/// Runtime test programs that must be in Oska Core and agree with the
/// Lean reference, each found by a line only it holds: the security
/// cases, which a count of covered programs could lose without notice.
const MUST_AGREE: [(&str, &str); 3] = [
    (
        "a_confined_capability_refuses_a_path_outside_its_root",
        "boxed.write(Path.from(\"sub/../../escape.txt\"), \"leaked\")",
    ),
    (
        "a_narrowed_read_capability_reads_only_beneath_its_root",
        "notes.read_to_string(Path.from(\"../secret.txt\"))",
    ),
    (
        "files_written_before_a_panic_are_kept",
        "out.write(Path.from(\"before.txt\"), \"written\")",
    ),
];

/// The one runtime test program that holds `line`.
fn program_holding(line: &str) -> String {
    let found: Vec<String> = runtime_test_programs()
        .into_iter()
        .map(|(_, src)| src)
        .filter(|src| src.contains(line))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one runtime test program should hold {line:?}: {found:#?}"
    );
    found.into_iter().next().unwrap_or_default()
}

#[test]
fn the_confinement_programs_agree_with_the_lean_reference() {
    let test = "lean_reference must-agree";
    let Some(exe) = reference_or_skip(test) else {
        return;
    };
    let lean = |ir: &str| run_reference(exe, ir);
    for (name, line) in MUST_AGREE {
        let src = program_holding(line);
        let verdict = verdict(&src, &lean, &|_| {});
        assert!(
            matches!(verdict, Verdict::Agreed),
            "the program of {name} should be in Oska Core and agree with the Lean reference: \
             {verdict:?}"
        );
    }
}

/// A program in Oska Core that panics: `10 / zero`.
const DIVIDES_BY_ZERO: &str = "fn half(n: Int) -> Int {\n    n / 2\n}\n\nfn main() {\n    let zero = half(1)\n    let n = 10 / zero\n}\n";

/// Make a run end without a panic, with exit code 0.
fn no_panic(outcome: &mut RunOutcome) {
    outcome.panic = None;
    outcome.exit_code = 0;
}

/// Make a run's panic `P0001`.
fn another_panic(outcome: &mut RunOutcome) {
    if let Some(panic) = &mut outcome.panic {
        panic.code = "P0001";
    }
}

/// Make a run exit with 1.
fn another_exit_code(outcome: &mut RunOutcome) {
    outcome.exit_code = 1;
}

/// A change that makes an interpreter's run wrong.
type Tamper = fn(&mut RunOutcome);

/// The ways the tests below make the interpreter's run of
/// [`DIVIDES_BY_ZERO`] wrong.
const WRONG_RESULTS: [(&str, Tamper); 3] = [
    ("no panic", no_panic),
    ("another panic", another_panic),
    ("another exit code", another_exit_code),
];

#[test]
fn a_wrong_interpreter_result_fails_against_the_lean_reference() {
    let Some(exe) = reference_or_skip("lean_reference (a wrong result)") else {
        return;
    };
    let lean = |ir: &str| run_reference(exe, ir);
    assert!(
        matches!(verdict(DIVIDES_BY_ZERO, &lean, &|_| {}), Verdict::Agreed),
        "the untampered run agrees"
    );
    for (what, tamper) in WRONG_RESULTS {
        let verdict = verdict(DIVIDES_BY_ZERO, &lean, &tamper);
        assert!(
            matches!(verdict, Verdict::Differs(_)),
            "{what}: a wrong interpreter result should differ, got {verdict:?}"
        );
    }
}

#[test]
fn a_difference_or_a_refused_core_ir_fails_with_a_stubbed_reference() {
    // The Lean reference's form for DIVIDES_BY_ZERO, without Lean.
    let right = |_: &str| Ok("panic P0004\nexit 101\n".to_string());
    assert!(matches!(
        verdict(DIVIDES_BY_ZERO, &right, &|_| {}),
        Verdict::Agreed
    ));
    for (what, tamper) in WRONG_RESULTS {
        let verdict = verdict(DIVIDES_BY_ZERO, &right, &tamper);
        assert!(
            matches!(verdict, Verdict::Differs(_)),
            "{what}: should differ, got {verdict:?}"
        );
    }
    let refuses = |_: &str| Err("exit status: 2 with error: not core IR version 5".to_string());
    assert!(matches!(
        verdict(DIVIDES_BY_ZERO, &refuses, &|_| {}),
        Verdict::Differs(_)
    ));
}

/// A program in Oska Core that prints and writes a file.
const WRITES: &str = "fn main(io: Stdout, fs: FsWrite) {\n    let _ = fs.restrict_to(\".\").write(Path.from(\"a.txt\"), \"x\")\n    let _ = io.println(\"hi\")\n}\n";

#[test]
fn a_difference_in_stdout_or_files_fails_with_a_stubbed_reference() {
    let right =
        |_: &str| Ok("stdout \"hi\\n\"\nfile \"/a.txt\" \"x\"\nresult ()\nexit 0\n".to_string());
    assert!(matches!(verdict(WRITES, &right, &|_| {}), Verdict::Agreed));
    let no_file = |_: &str| Ok("stdout \"hi\\n\"\nresult ()\nexit 0\n".to_string());
    assert!(matches!(
        verdict(WRITES, &no_file, &|_| {}),
        Verdict::Differs(_)
    ));
    let other_text =
        |_: &str| Ok("stdout \"hi\\n\"\nfile \"/a.txt\" \"y\"\nresult ()\nexit 0\n".to_string());
    assert!(matches!(
        verdict(WRITES, &other_text, &|_| {}),
        Verdict::Differs(_)
    ));
    let other_stdout =
        |_: &str| Ok("stdout \"ho\\n\"\nfile \"/a.txt\" \"x\"\nresult ()\nexit 0\n".to_string());
    assert!(matches!(
        verdict(WRITES, &other_stdout, &|_| {}),
        Verdict::Differs(_)
    ));
}

#[test]
fn text_is_quoted_as_the_lean_reference_quotes_it() {
    assert_eq!(
        quoted("a\"b\\c\nd\te\rf\u{1}g\u{7f}h\u{e9}"),
        "\"a\\\"b\\\\c\\nd\\te\\rf\\u{1}g\\u{7f}h\u{e9}\""
    );
}

#[test]
fn a_program_outside_oska_core_is_refused_and_one_that_does_not_check_never_runs() {
    let never = |_: &str| -> Result<String, String> { panic!("the reference should not run") };
    // `main` returning a `Result` is outside Oska Core.
    let prints = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n";
    assert!(matches!(verdict(prints, &never, &|_| {}), Verdict::Refused));
    assert!(matches!(
        verdict("fn main() {\n    let x: Int = true\n}\n", &never, &|_| {}),
        Verdict::NotChecked
    ));
}

#[test]
fn the_corpus_reads_every_kind_of_string_literal() {
    let src = r####"
        // "in a comment"
        /* "in /* a nested */ block" */
        let c = '"';
        let e = '\'';
        fn f<'a>(x: &'a str) {}
        let plain = "a\n\"b\" \x41\u{1F600} \
                     c";
        let raw = r#"fn main() { "q" }"#;
        let bytes = b"by";
    "####;
    assert_eq!(
        string_literals(src),
        vec![
            "a\n\"b\" A\u{1F600} c".to_string(),
            "fn main() { \"q\" }".to_string(),
            "by".to_string(),
        ]
    );
}

/// Core IR that is not version 5, each with what the refusal must name.
const MALFORMED: [(&str, &str, &str); 25] = [
    (
        "a wrong version",
        r#"{"core_ir_version": 6, "types": [], "functions": []}"#,
        "core_ir_version 6",
    ),
    (
        "version 4",
        r#"{"core_ir_version": 4, "types": [], "functions": []}"#,
        "core_ir_version 4",
    ),
    (
        "version 3",
        r#"{"core_ir_version": 3, "types": [], "functions": []}"#,
        "core_ir_version 3",
    ),
    (
        "version 2",
        r#"{"core_ir_version": 2, "types": [], "functions": []}"#,
        "core_ir_version 2",
    ),
    (
        "version 1",
        r#"{"core_ir_version": 1, "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": []}}]}"#,
        "core_ir_version 1",
    ),
    (
        "an unknown node",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Loop"}}]}"#,
        "unknown expression kind `Loop`",
    ),
    (
        "an Int out of range",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Let", "binds": false, "type": "Int", "value": {"kind": "Int", "value": 9223372036854775808}}]}}]}"#,
        "outside Int",
    ),
    (
        "an arity mismatch",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Call", "function": "main", "args": [{"kind": "Unit"}]}}]}"#,
        "with 1 argument(s), but it has 0 parameter(s)",
    ),
    (
        "trailing junk",
        r#"{"core_ir_version": 5, "types": [], "functions": []} junk"#,
        "not JSON",
    ),
    (
        "an unknown field",
        r#"{"core_ir_version": 5, "types": [], "functions": [], "extra": 1}"#,
        "unknown field `extra`",
    ),
    (
        "an undeclared variant",
        r#"{"core_ir_version": 5, "types": [{"kind": "Enum", "name": "Light", "variants": [{"name": "Red", "fields": []}]}], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Expr", "value": {"kind": "Construct", "type": "Light", "variant": "Blue", "fields": []}}]}}]}"#,
        "the enum `Light` has no variant `Blue`",
    ),
    (
        "a pattern with too many fields",
        r#"{"core_ir_version": 5, "types": [{"kind": "Struct", "name": "P", "fields": ["Int"]}], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Match", "scrutinee": {"kind": "Unit"}, "arms": [{"pattern": {"kind": "Construct", "type": "P", "fields": [{"kind": "Wildcard"}, {"kind": "Wildcard"}]}, "body": {"kind": "Unit"}}]}}]}"#,
        "a constructor of `P` with 2 field(s), but it has 1",
    ),
    (
        "an unknown built-in",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Builtin", "name": "Stdin.read_line", "args": []}}]}"#,
        "unknown built-in `Stdin.read_line`",
    ),
    (
        "a capability field",
        r#"{"core_ir_version": 5, "types": [{"kind": "Struct", "name": "Logger", "fields": ["Stdout"]}], "functions": []}"#,
        "a field is a capability",
    ),
    (
        "a function field",
        r#"{"core_ir_version": 5, "types": [{"kind": "Struct", "name": "Hook", "fields": [{"kind": "Fn", "cap": false, "params": [], "ret": "()"}]}], "functions": []}"#,
        "a field is a function",
    ),
    (
        "a capture at the closure's height",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Expr", "value": {"kind": "Closure", "params": [], "captures": [0], "height": 0, "cap": false, "ret": "()", "body": {"kind": "Unit"}}}]}}]}"#,
        "slot 0 is not below the closure's height 0",
    ),
    (
        "captures out of order",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [{"type": "Int", "binds": true}, {"type": "Int", "binds": true}], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Expr", "value": {"kind": "Closure", "params": [], "captures": [1, 0], "height": 2, "cap": false, "ret": "Int", "body": {"kind": "Binary", "op": "+", "lhs": {"kind": "Local", "slot": 0}, "rhs": {"kind": "Local", "slot": 1}}}}]}}]}"#,
        "is not in strictly ascending order",
    ),
    (
        "a read slot left out of the captures",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [{"type": "Int", "binds": true}], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Expr", "value": {"kind": "Closure", "params": [], "captures": [], "height": 1, "cap": false, "ret": "Int", "body": {"kind": "Local", "slot": 0}}}]}}]}"#,
        "the body reads slot 0, which the closure does not capture",
    ),
    (
        "a type named IoError",
        r#"{"core_ir_version": 5, "types": [{"kind": "Struct", "name": "IoError", "fields": []}], "functions": []}"#,
        "a type is named `IoError`",
    ),
    (
        "an unknown type kind",
        r#"{"core_ir_version": 5, "types": [{"kind": "Struct", "name": "P", "fields": [{"kind": "Option", "some": "Int"}]}], "functions": []}"#,
        "unknown type kind `Option`",
    ),
    (
        "a Result that holds a capability",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Let", "binds": false, "type": {"kind": "Result", "ok": "Int", "err": "Stdout"}, "value": {"kind": "Unit"}}]}}]}"#,
        "a `Result` that holds a capability or a function",
    ),
    (
        "a Result that holds a function, inside another",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Block", "stmts": [{"kind": "Let", "binds": false, "type": {"kind": "Result", "ok": {"kind": "Result", "ok": {"kind": "Fn", "cap": false, "params": [], "ret": "()"}, "err": "Int"}, "err": "Int"}, "value": {"kind": "Unit"}}]}}]}"#,
        "a `Result` that holds a capability or a function",
    ),
    (
        "an Err pattern on a type that is not a Result",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Match", "scrutinee": {"kind": "Int", "value": 1}, "arms": [{"pattern": {"kind": "Err", "type": "Int", "value": {"kind": "Wildcard"}}, "body": {"kind": "Unit"}}]}}]}"#,
        "an `Err` pattern on a type that is not a `Result`",
    ),
    (
        "a ? that converts with an undefined function",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "main", "params": [], "ret": "()", "body": {"kind": "Try", "operand": {"kind": "Ok", "value": {"kind": "Unit"}}, "convert": "<AppError as From<Int>>.from"}}]}"#,
        "`?` converts with `<AppError as From<Int>>.from`, which the program does not define",
    ),
    (
        "a ? that converts with a function of two parameters",
        r#"{"core_ir_version": 5, "types": [], "functions": [{"name": "two", "params": [{"type": "Int", "binds": true}, {"type": "Int", "binds": true}], "ret": "Int", "body": {"kind": "Local", "slot": 0}}, {"name": "main", "params": [], "ret": "()", "body": {"kind": "Try", "operand": {"kind": "Ok", "value": {"kind": "Unit"}}, "convert": "two"}}]}"#,
        "`?` converts with `two`, which takes 2 parameter(s), not 1",
    ),
];

#[test]
fn the_lean_reference_refuses_malformed_core_ir_with_exit_2() {
    let Some(exe) = reference_or_skip("lean_reference (malformed core IR)") else {
        return;
    };
    for (what, ir, message) in MALFORMED {
        let run = run_exe(exe, ir).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(run.code, Some(2), "{what}: the exit code, with {run:?}");
        assert_eq!(run.stdout, "", "{what}: nothing on stdout");
        assert!(
            run.stderr.starts_with("error: not core IR version 5: ")
                && run.stderr.contains(message),
            "{what}: stderr should name {message:?}: {:?}",
            run.stderr
        );
    }
}
