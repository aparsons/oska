//! Source from standard input (#447, spec #446): `oskac check -` and
//! `oskac run -` read one module from standard input, as an embedder
//! pipes a snippet, and report it as the same bytes in a file, named
//! `<stdin>`.
//!
//! One seam, the real `oskac`, fed on standard input. Outputs are
//! compared with the same bytes checked as a file and with golden files
//! under `tests/golden/code_mode_stdin/`. To regenerate them after an
//! intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test code_mode_stdin` and
//! review the diff.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::portable::Scratch;
use common::Json;

/// A clean snippet with authority: it prints its `args` and what it
/// reads from its own standard input.
const ECHO: &str = "\
fn main(io: Stdout, args: Args, input: Stdin) -> Result<(), IoError> {
    let name = args.get(0).unwrap_or(\"none\")
    let first = args.get(1).unwrap_or(\"none\")
    let line = match input.read_line()? {
        Some(_) => \"a line\"
        None => \"end of file\"
    }
    io.println(\"{name} {first} {line}\")
}
";

/// A snippet with diagnostics.
const BROKEN: &str = "fn main() -> Int {\n    \"not an int\"\n}\n";

/// A case: its own folder, which is also its own build cache's parent.
struct Case {
    dir: Scratch,
}

impl Case {
    fn new(name: &str) -> Case {
        Case {
            dir: Scratch::new("code-mode-stdin", name),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
        command
            .args(args)
            .current_dir(self.dir.path())
            .env("OSKA_CACHE_DIR", self.dir.join("cache"))
            .env_remove("OSKA_CACHE")
            .env_remove("OSKA_CACHE_LIMIT");
        command
    }

    /// Run `oskac args` with `input` on its standard input.
    fn piped(&self, args: &[&str], input: &[u8]) -> Output {
        for _ in 0..50 {
            let spawned = self
                .command(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match spawned {
                // The executable is busy (`ETXTBSY`): see `output_of`.
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                result => result.expect("oskac should start"),
            };
            let mut stdin = child.stdin.take().expect("a stdin pipe");
            // A command that refuses `-` may exit before it reads.
            let _ = stdin.write_all(input);
            drop(stdin);
            return child.wait_with_output().expect("oskac should finish");
        }
        panic!("oskac stayed busy");
    }

    /// Run `oskac args` on `input` written to `file` in the case folder.
    fn on_file(&self, args: &[&str], file: &str, input: &[u8]) -> Output {
        std::fs::write(self.dir.join(file), input).expect("should write the file");
        let mut command = self.command(args);
        command.arg(file).stdin(Stdio::null());
        common::portable::output_of(&mut command)
    }

    /// `oskac cache info --format=json`'s count `field`.
    fn count(&self, field: &str) -> u64 {
        let output =
            common::portable::output_of(&mut self.command(&["cache", "info", "--format=json"]));
        match Json::parse(&text(&output.stdout)).get(field) {
            Some(Json::Num(n)) => *n,
            other => panic!("cache info has no number {field}: {other:?}"),
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// An output as one text: exit code, stdout and stderr.
fn shown(output: &Output) -> String {
    format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        output
            .status
            .code()
            .map_or("none".to_string(), |c| c.to_string()),
        text(&output.stdout),
        text(&output.stderr)
    )
}

/// `output` matches the golden file `name`.
fn assert_golden(output: &Output, name: &str) {
    let actual = shown(output);
    let golden: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/code_mode_stdin")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a folder");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// `check -` on `input` prints what `check` on a file with the same
/// bytes prints, with `<stdin>` for the file's path, and matches `golden`.
fn assert_checks_as_a_file(case: &Case, extra: &[&str], input: &[u8], golden: &str) {
    let mut args = vec!["check", "-"];
    args.extend_from_slice(extra);
    let piped = case.piped(&args, input);
    let mut args = vec!["check"];
    args.extend_from_slice(extra);
    let filed = case.on_file(&args, "snippet.osk", input);
    assert_eq!(
        shown(&piped),
        shown(&filed).replace("snippet.osk", "<stdin>"),
        "`check -` differs from `check snippet.osk` apart from the path"
    );
    assert_golden(&piped, golden);
}

#[test]
fn check_dash_reports_a_clean_snippet_as_a_file_named_stdin() {
    let case = Case::new("clean");
    assert_checks_as_a_file(&case, &["--no-cache"], ECHO.as_bytes(), "check_clean.txt");
    assert_checks_as_a_file(
        &case,
        &["--no-cache", "--format=json"],
        ECHO.as_bytes(),
        "check_clean.json",
    );
}

#[test]
fn check_dash_reports_diagnostics_as_a_file_named_stdin() {
    let case = Case::new("broken");
    assert_checks_as_a_file(
        &case,
        &["--no-cache"],
        BROKEN.as_bytes(),
        "check_broken.txt",
    );
    assert_checks_as_a_file(
        &case,
        &["--no-cache", "--format=json"],
        BROKEN.as_bytes(),
        "check_broken.json",
    );
}

#[test]
fn an_empty_stdin_is_checked_as_an_empty_module() {
    let case = Case::new("empty");
    assert_checks_as_a_file(&case, &["--format=json"], b"", "check_empty.json");
}

#[test]
fn stdin_that_is_not_utf8_is_the_error_a_file_that_is_not_gives() {
    let case = Case::new("not-utf8");
    assert_checks_as_a_file(&case, &[], b"fn main() {}\n\xff\n", "check_not_utf8.txt");
}

#[test]
fn run_dash_runs_the_snippet_with_stdin_as_its_path_and_its_own_stdin_empty() {
    let case = Case::new("run");
    for extra in [&[][..], &["--interpret"][..]] {
        let mut args = vec!["run", "-"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["--", "x", "y"]);
        let output = case.piped(&args, ECHO.as_bytes());
        assert_eq!(
            shown(&output),
            "exit: 0\n--- stdout\n<stdin> x end of file\n--- stderr\n",
            "{args:?}"
        );
    }
}

#[test]
fn run_dash_refuses_a_snippet_with_diagnostics() {
    let case = Case::new("run-broken");
    let output = case.piped(&["run", "-"], BROKEN.as_bytes());
    assert_golden(&output, "run_broken.txt");
}

#[test]
fn dash_with_any_other_command_is_a_usage_error() {
    let case = Case::new("usage");
    for command in ["test", "build", "fix", "lex", "parse"] {
        let output = case.piped(&[command, "-"], BROKEN.as_bytes());
        assert_eq!(
            output.status.code(),
            Some(2),
            "{command} -: {}",
            shown(&output)
        );
        assert!(
            text(&output.stderr).starts_with("oskac - the Oska compiler"),
            "{command} -: {}",
            shown(&output)
        );
    }
}

#[test]
fn a_second_check_dash_of_the_same_bytes_is_a_cache_hit_with_identical_output() {
    let case = Case::new("cache");
    for (args, golden) in [
        (&["check", "-"][..], "check_clean.txt"),
        (&["check", "-", "--format=json"][..], "check_clean.json"),
    ] {
        let first = case.piped(args, ECHO.as_bytes());
        let hits = case.count("hits");
        let misses = case.count("misses");
        let second = case.piped(args, ECHO.as_bytes());
        assert_eq!(
            case.count("hits"),
            hits + 1,
            "{args:?}: the second is a hit"
        );
        assert_eq!(case.count("misses"), misses, "{args:?}: and not a miss");
        assert_eq!(shown(&first), shown(&second), "{args:?}");
        assert_golden(&second, golden);
    }
}
