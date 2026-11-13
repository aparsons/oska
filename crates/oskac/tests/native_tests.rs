//! Native `test` blocks and the test host (ticket #139), run on both
//! backends: `oskac test --interpret` and `oskac test`, native by default
//! on aarch64 Linux (#141), must print the same
//! report, text and JSON, with and without `--integration`, the same
//! stderr, and exit with the same code.
//!
//! Each case runs in two identical temporary directories, so an
//! integration test that writes files leaves the same tree on both sides.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What one `oskac test` run showed.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    tree: BTreeMap<String, String>,
}

/// The flag sets every case runs with.
const RUNS: [&[&str]; 4] = [
    &[],
    &["--format=json"],
    &["--integration"],
    &["--integration", "--format=json"],
];

/// Run `oskac test` on `target` among `files`, on both backends and with
/// every flag set, and require the same outcomes. Returns the
/// interpreter's outcome of the plain run.
fn assert_same_tests(name: &str, files: &[(&str, &str)], target: &str) -> Outcome {
    let mut first = None;
    for (run, flags) in RUNS.iter().enumerate() {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("native_tests")
            .join(format!("{name}-{run}"));
        let _ = std::fs::remove_dir_all(&root);
        let interpreted = oskac_test(
            &fresh(&root.join("interpreted"), files),
            target,
            &[*flags, &["--interpret"][..]].concat(),
        );
        let native = oskac_test(&fresh(&root.join("native"), files), target, flags);
        assert!(
            interpreted == native,
            "case `{name}` {flags:?}: the backends differ\n--- interpreter\n{interpreted:#?}\n--- native\n{native:#?}"
        );
        first.get_or_insert(interpreted);
    }
    first.expect("at least one run")
}

/// A single-file program, `main.osk`, on both backends.
fn assert_same_file(name: &str, src: &str) -> Outcome {
    assert_same_tests(name, &[("main.osk", src)], "main.osk")
}

fn fresh(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    std::fs::create_dir_all(dir).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir.to_path_buf()
}

fn oskac_test(dir: &Path, target: &str, flags: &[&str]) -> Outcome {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(dir)
        .arg("test")
        .arg(target)
        .args(flags)
        .env_clear()
        .output()
        .expect("oskac should run");
    let mut tree = BTreeMap::new();
    read_tree(dir, "", &mut tree);
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        tree,
    }
}

fn read_tree(dir: &Path, prefix: &str, tree: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(dir).expect("should list the directory") {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            tree.insert(format!("{name}/"), String::new());
            read_tree(&path, &format!("{name}/"), tree);
        } else {
            let text = std::fs::read(&path).expect("should read the file");
            tree.insert(name, String::from_utf8_lossy(&text).into_owned());
        }
    }
}

#[test]
fn a_passing_and_a_failing_assertion() {
    let outcome = assert_same_file(
        "assert",
        r#"
test "holds" {
    t.assert(1 + 1 == 2)
}

test "does not hold" {
    t.assert(1 + 1 == 3)
}
"#,
    );
    assert!(outcome.stdout.contains("test holds ... ok\n"));
    assert!(outcome.stdout.contains("error[P0010]"));
    assert_eq!(outcome.exit_code, Some(1));
}

#[test]
fn assert_eq_and_assert_ne_show_both_values_as_dbg_does() {
    let outcome = assert_same_file(
        "assert_eq",
        r#"
struct Point { x: Int, y: Int }

impl Eq for Point {
    fn eq(self, other: Point) -> Bool {
        self.x == other.x && self.y == other.y
    }
}

enum Shape {
    Circle(Int),
    Square(Point),
}

impl Eq for Shape {
    fn eq(self, other: Shape) -> Bool {
        match self {
            Shape.Circle(r) => match other {
                Shape.Circle(s) => r == s
                _ => false
            }
            Shape.Square(p) => match other {
                Shape.Square(q) => p == q
                _ => false
            }
        }
    }
}

test "ints equal" {
    t.assert_eq(2 + 2, 4)
}

test "ints differ" {
    t.assert_eq(2 + 2, 5)
}

test "texts differ" {
    t.assert_eq("a\tb", "a\"b")
}

test "lists differ" {
    t.assert_eq([1, 2, 3], [1, 2])
}

test "structs differ" {
    t.assert_eq(Point(x: 1, y: 2), Point(x: 1, y: 3))
}

test "enums differ" {
    t.assert_eq(Shape.Square(Point(x: 0, y: 0)), Shape.Circle(4))
}

test "options differ" {
    let none: Option<Int> = None
    t.assert_eq(Some(1), none)
}

test "bools differ" {
    t.assert_ne(true, false)
    t.assert_ne(false, false)
}

test "texts equal under assert_ne" {
    t.assert_ne("same", "same")
}

test "unit" {
    t.assert_eq((), ())
}
"#,
    );
    assert!(outcome
        .stdout
        .contains("error[P0011]: assertion failed: `actual == expected`: actual 4, expected 5"));
    assert!(outcome
        .stdout
        .contains("error[P0012]: assertion failed: `actual != other`: both are false"));
}

#[test]
fn a_user_eq_impl_decides_equality() {
    let outcome = assert_same_file(
        "user_eq",
        r#"
struct Money { cents: Int, note: String }

impl Eq for Money {
    fn eq(self, other: Money) -> Bool {
        self.cents == other.cents
    }
}

test "notes do not matter" {
    t.assert_eq(Money(cents: 5, note: "a"), Money(cents: 5, note: "b"))
}

test "cents do" {
    t.assert_eq(Money(cents: 5, note: "a"), Money(cents: 6, note: "a"))
}
"#,
    );
    assert!(outcome.stdout.contains("test notes do not matter ... ok\n"));
}

#[test]
fn fail_fails_with_its_message_escaped_in_the_text_report() {
    let outcome = assert_same_file(
        "fail",
        r#"
fn reason(n: Int) -> String {
    "line {n}\nnext\r"
}

test "fails" {
    t.fail(reason(3))
}

test "fails after passing" {
    t.assert(true)
    t.fail("plain")
    t.fail("never")
}
"#,
    );
    assert!(outcome.stdout.contains("error[P0013]: plain"));
}

#[test]
fn a_panicking_test_fails_alone_and_the_rest_still_run() {
    let outcome = assert_same_file(
        "panics",
        r#"
fn divide(a: Int, b: Int) -> Int {
    a / b
}

fn deep(n: Int) -> Int {
    deep(n + 1)
}

test "divides by zero" {
    let _ = divide(1, 0)
}

test "overflows" {
    let big = 9223372036854775807
    let _ = big + 1
}

test "slices out of range" {
    let _ = "abc".slice(1, 9)
}

test "is not written" {
    todo()
}

test "out of bounds" {
    let _ = [1, 2].at(5)
}

test "recurses forever" {
    let _ = deep(0)
}

test "still runs" {
    t.assert_eq(divide(6, 3), 2)
}
"#,
    );
    assert!(outcome.stdout.contains("test still runs ... ok\n"));
    assert!(outcome.stdout.contains("error[P0004]"));
}

#[test]
fn stderr_lines_carry_the_test_name_and_stdout_is_recorded() {
    let outcome = assert_same_file(
        "streams",
        r#"
fn greet(out: Stdout, name: String) -> Result<(), IoError> {
    out.println("hello, {name}")
}

test "dbg goes to stderr" {
    let n = dbg(1 + 2)
    let _ = dbg("two\nlines")
    t.assert_eq(n, 3)
}

test "a name with\ta tab and \0 nul" {
    let _ = dbg([1, 2])
    t.fail("so it shows")
}

test "printed records each line" {
    t.assert_eq(t.printed(), [])
    let _ = greet(t.stdout, "ada")
    let _ = t.stdout.println("two\nlines")
    let _ = t.stdout.println("")
    t.assert_eq(t.printed(), ["hello, ada", "two\nlines", ""])
}

test "printed starts empty in every test" {
    t.assert_eq(t.printed().len(), 0)
    let _ = greet(t.stdout, "bob")
    t.assert_eq(t.printed(), ["hello, ada"])
}
"#,
    );
    assert!(outcome.stderr.contains("[dbg goes to stderr] [dbg] 3\n"));
    assert!(outcome
        .stderr
        .contains("[a name with\\ta tab and \\u{0} nul] "));
    assert!(outcome
        .stdout
        .contains("test printed records each line ... ok\n"));
}

/// What every filesystem case shows: each operation's result, printed as
/// a line to `t.stdout`, which a test then shows by failing on purpose.
const SHOWN: &str = r#"
fn done(r: Result<(), IoError>) -> String {
    match r {
        Ok(()) => "ok"
        Err(e) => refused(e)
    }
}

fn read(r: Result<Untrusted<String>, IoError>) -> String {
    match r {
        Ok(text) => {
            let shown = text.assume_trusted()
            "read {shown}"
        }
        Err(e) => refused(e)
    }
}

fn listed(r: Result<List<Untrusted<String>>, IoError>) -> String {
    match r {
        Ok(names) => {
            let joined = ", ".join(names).assume_trusted()
            "listed [{joined}]"
        }
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> String {
    let message = e.message.assume_trusted()
    "error: {message}"
}

fn show(t: TestContext, line: String) {
    let _ = t.stdout.println(line)
}
"#;

#[test]
fn a_plain_test_has_an_empty_filesystem_of_its_own() {
    let src = format!(
        "{SHOWN}{}",
        r#"
test "writes, reads and lists" {
    let w = t.fs.write_only().restrict_to(".")
    let r = t.fs.read_only().restrict_to(".")
    show(t, done(w.create_dir(Path.new("out"))))
    show(t, done(w.create_dir(Path.new("out/deep"))))
    show(t, done(w.create_dir(Path.new("out"))))
    show(t, done(w.write(Path.new("out/a.txt"), "one")))
    show(t, done(w.write(Path.new("out/b.txt"), "two")))
    show(t, done(w.write(Path.new("out/a.txt"), "three")))
    show(t, done(w.write(Path.new("top.txt"), "top")))
    show(t, read(r.read_to_string(Path.new("out/a.txt"))))
    show(t, read(r.read_to_string(Path.new("out/./deep/../b.txt"))))
    show(t, listed(r.list_dir(Path.new("."))))
    show(t, listed(r.list_dir(Path.new("out"))))
    show(t, listed(r.list_dir(Path.new("out/deep"))))
    let out = r.restrict_to(Path.new("out"))
    show(t, read(out.read_to_string(Path.new("b.txt"))))
    let file = r.restrict_to_file(Path.new("out/new.txt"))
    show(t, read(file.read_to_string()))
    t.assert_eq(t.printed(), [])
}

test "nothing an earlier test wrote is here" {
    let r = t.fs.read_only().restrict_to(".")
    show(t, listed(r.list_dir(Path.new("."))))
    show(t, read(r.read_to_string(Path.new("out/a.txt"))))
    t.assert_eq(t.printed(), [])
}

test "refusals" {
    let w = t.fs.write_only().restrict_to("/")
    let r = t.fs.read_only().restrict_to("/")
    show(t, done(w.write(Path.new("file.txt"), "f")))
    show(t, done(w.create_dir(Path.new("dir"))))
    show(t, done(w.write(Path.new("../up.txt"), "x")))
    show(t, done(w.write(Path.new("/abs.txt"), "x")))
    show(t, done(w.write(Path.new(""), "x")))
    show(t, done(w.write(Path.new("dir/"), "x")))
    show(t, done(w.write(Path.new("dir"), "x")))
    show(t, done(w.write(Path.new("missing/a.txt"), "x")))
    show(t, done(w.write(Path.new("file.txt/a.txt"), "x")))
    show(t, done(w.create_dir(Path.new("file.txt"))))
    show(t, done(w.create_dir(Path.new("missing/sub"))))
    show(t, read(r.read_to_string(Path.new("."))))
    show(t, read(r.read_to_string(Path.new("dir"))))
    show(t, read(r.read_to_string(Path.new("missing.txt"))))
    show(t, listed(r.list_dir(Path.new("file.txt"))))
    show(t, listed(r.list_dir(Path.new("missing"))))
    show(t, listed(r.list_dir(Path.new("dir/../file.txt/x"))))
    t.assert_eq(t.printed(), [])
}

test "attenuations that reach nothing" {
    let _ = t.fs.write_only().restrict_to(".").write(Path.new("file.txt"), "f")
    let r = t.fs.read_only()
    show(t, listed(r.restrict_to("missing").list_dir(Path.new("."))))
    show(t, listed(r.restrict_to("file.txt").list_dir(Path.new("."))))
    show(t, listed(r.restrict_to("").list_dir(Path.new("."))))
    show(t, listed(r.restrict_to("a/../..").list_dir(Path.new("."))))
    show(t, listed(r.restrict_to("/missing/../.").list_dir(Path.new("."))))
    let root = r.restrict_to(".")
    show(t, read(root.restrict_to(Path.new("nope")).read_to_string(Path.new("a"))))
    show(t, read(root.restrict_to(Path.new("..")).read_to_string(Path.new("a"))))
    show(t, read(root.restrict_to(Path.new("file.txt/x")).read_to_string(Path.new("a"))))
    show(t, read(root.restrict_to(Path.new("file.txt")).read_to_string(Path.new("a"))))
    show(t, read(root.restrict_to(Path.new("nope")).restrict_to(Path.new("a")).read_to_string(Path.new("a"))))
    t.assert_eq(t.printed(), [])
}

test "file capabilities" {
    let w = t.fs.write_only()
    let r = t.fs.read_only()
    show(t, done(w.restrict_to(".").create_dir(Path.new("d"))))
    let file = r.restrict_to_file("/d/f.txt")
    show(t, read(file.read_to_string()))
    show(t, done(w.restrict_to_file("d/../d/f.txt").write("contents")))
    show(t, read(file.read_to_string()))
    show(t, read(r.restrict_to(".").restrict_to_file(Path.new("d/f.txt")).read_to_string()))
    show(t, read(r.restrict_to_file("/").read_to_string()))
    show(t, read(r.restrict_to_file("..").read_to_string()))
    show(t, read(r.restrict_to_file("").read_to_string()))
    show(t, read(r.restrict_to_file("missing/f.txt").read_to_string()))
    show(t, read(r.restrict_to_file("d/f.txt/g").read_to_string()))
    show(t, done(w.restrict_to_file("d").write("x")))
    show(t, read(r.restrict_to(".").restrict_to_file(Path.new("d/")).read_to_string()))
    show(t, read(r.restrict_to(".").restrict_to_file(Path.new("no/f.txt")).read_to_string()))
    show(t, read(r.restrict_to(".").restrict_to_file(Path.new("../f.txt")).read_to_string()))
    show(t, read(r.restrict_to("missing").restrict_to_file(Path.new("f.txt")).read_to_string()))
    t.assert_eq(t.printed(), [])
}
"#
    );
    let outcome = assert_same_file("fake_fs", &src);
    assert!(
        outcome
            .stdout
            .contains("no such file or directory: /out/new.txt"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn the_clock_is_at_the_epoch_and_random_starts_from_the_fixed_seed() {
    let outcome = assert_same_file(
        "clock_random",
        r#"
test "the clock" {
    t.assert_eq(t.clock.now_millis(), 1)
}

test "random" {
    let a = t.random.next_int()
    let b = t.random.next_int()
    t.assert_eq([a, b, t.random.next_int()], [])
}

test "random again" {
    let a = t.random.next_int()
    t.assert_eq(a, 0)
}
"#,
    );
    assert!(outcome.stdout.contains("actual 0, expected 1"));
}

#[test]
fn integration_tests_run_on_the_real_os_only_when_asked() {
    let outcome = assert_same_file(
        "integration",
        r#"
fn shown(r: Result<(), IoError>) -> String {
    match r {
        Ok(()) => "ok"
        Err(e) => {
            let message = e.message.assume_trusted()
            "error: {message}"
        }
    }
}

test "plain" {
    t.assert_eq(t.clock.now_millis(), 0)
}

#[integration]
test "the clock has moved on" {
    t.assert(t.clock.now_millis() > 0)
}

#[integration]
test "writes a real file" {
    let root = t.fs.write_only().restrict_to(".")
    t.assert_eq(shown(root.write(Path.new("written.txt"), "real")), "ok")
    let _ = dbg("wrote")
    let _ = t.stdout.println("recorded, not shown")
    t.assert_eq(t.printed(), ["recorded, not shown"])
    t.assert_eq(shown(root.write(Path.new("../escape.txt"), "x")), "ok")
}

#[integration]
test "random is not the fixed seed's" {
    t.assert_ne(t.random.next_int(), 1975979253378891327)
}
"#,
    );
    assert!(outcome.stdout.contains("skipped (#[integration]"));
}

// ---- The corpus ----------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `dir`, by its path relative to `dir` after `prefix`.
fn files_under(dir: &Path, prefix: &str, files: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("should list the directory")
        .map(|e| e.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = format!(
            "{prefix}{}",
            path.file_name().expect("a name").to_string_lossy()
        );
        if path.is_dir() {
            files_under(&path, &format!("{name}/"), files);
        } else {
            let text = std::fs::read(&path).expect("should read the file");
            files.push((name, String::from_utf8_lossy(&text).into_owned()));
        }
    }
}

fn has_tests(text: &str) -> bool {
    text.contains("test \"")
}

/// Run a corpus program on both backends.
fn assert_same_corpus(name: &str, files: &[(String, String)], target: &str) {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let outcome = assert_same_tests(name, &files, target);
    // A real report, not two refusals that happen to agree.
    assert!(
        outcome.stdout.contains(" tests: "),
        "case `{name}` was refused: {outcome:#?}"
    );
    // And a native one: `oskac test` runs a program the backend refuses
    // on the interpreter.
    assert!(
        oska_native::build_tests(&checked(&files, target), oska_native::Target::LinuxAarch64)
            .is_ok(),
        "case `{name}`: the native backend refuses its tests"
    );
}

/// The checked program `target` names among `files`: a source file, or
/// a package directory.
fn checked(files: &[(&str, &str)], target: &str) -> oska_check::program::Program {
    let report = if let Some((_, src)) = files.iter().find(|(path, _)| *path == target) {
        oska_check::check_module(src)
    } else {
        let prefix = format!("{target}/src/");
        let package = oska_check::Package {
            manifest: files
                .iter()
                .find(|(path, _)| *path == format!("{target}/oska.toml"))
                .map(|(_, text)| text.to_string()),
            modules: files
                .iter()
                .filter_map(|(path, text)| {
                    let module = path.strip_prefix(&prefix)?.strip_suffix(".osk")?;
                    Some(oska_check::ModuleSource::in_package(
                        &module.replace('/', "."),
                        text,
                    ))
                })
                .collect(),
        };
        oska_check::check_package(&package, oska_check::CheckOptions::default())
    };
    report.program.expect("a checked program")
}

/// Every test-bearing package among the fixtures and the examples: a
/// directory with an `oska.toml`, any of whose sources has a `test`.
fn test_packages() -> Vec<PathBuf> {
    let root = repo_root();
    let mut found = Vec::new();
    for parent in ["crates/oskac/tests/fixtures/packages", "examples"] {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join(parent))
            .expect("should list the packages")
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| p.join("oska.toml").is_file() && p.join("src").is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let mut files = Vec::new();
            files_under(&dir.join("src"), "", &mut files);
            if files.iter().any(|(_, text)| has_tests(text)) {
                found.push(dir);
            }
        }
    }
    found
}

#[test]
fn every_test_bearing_package_in_the_corpus_reports_the_same() {
    let packages = test_packages();
    assert!(packages.len() >= 7, "{packages:?}");
    for dir in packages {
        let name = dir
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let mut files = Vec::new();
        files_under(&dir, &format!("{name}/"), &mut files);
        assert_same_corpus(&format!("package-{name}"), &files, &name);
    }
}

#[test]
fn every_test_bearing_single_file_in_the_corpus_reports_the_same() {
    let root = repo_root();
    let mut count = 0;
    for dir in ["crates/oskac/tests/fixtures", "examples"] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .expect("should list the directory")
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| p.extension().is_some_and(|e| e == "osk"))
            .collect();
        paths.sort();
        for path in paths {
            let src = std::fs::read_to_string(&path).expect("should read the source");
            if !has_tests(&src) {
                continue;
            }
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            assert_same_corpus(&format!("file-{name}"), &[(name.clone(), src)], &name);
            count += 1;
        }
    }
    assert!(count >= 1);
}

/// Every complete Oska program in the guides (`docs/guide`, which
/// `guides.rs` checks) that has a `test`.
fn doc_programs() -> Vec<(String, String)> {
    let mut pages = Vec::new();
    let mut stack = vec![repo_root().join("docs/guide")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("should list the docs") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                pages.push(path);
            }
        }
    }
    pages.sort();
    let mut programs = Vec::new();
    for page in pages {
        let text = std::fs::read_to_string(&page).expect("should read the page");
        let stem = page
            .file_stem()
            .expect("a stem")
            .to_string_lossy()
            .into_owned();
        // The block being read, and whether it is a complete program:
        // `error=` and `fragment` blocks are not (`guides.rs`).
        let mut block: Option<(bool, String)> = None;
        let mut index = 0;
        for line in text.lines() {
            let fence = line.trim_start();
            match &mut block {
                None if fence.starts_with("```") => {
                    let program = fence.starts_with("```oska")
                        && !fence.contains("error=")
                        && !fence.contains("fragment");
                    block = Some((program, String::new()));
                }
                Some((program, src)) if fence.starts_with("```") => {
                    if *program && has_tests(src) {
                        programs.push((format!("{stem}-{index}"), src.clone()));
                    }
                    index += 1;
                    block = None;
                }
                Some((_, src)) => {
                    src.push_str(line);
                    src.push('\n');
                }
                None => {}
            }
        }
    }
    programs
}

#[test]
fn every_test_bearing_program_in_the_guides_reports_the_same() {
    let programs = doc_programs();
    assert!(programs.len() >= 5, "{}", programs.len());
    for (name, src) in programs {
        assert_same_corpus(
            &format!("doc-{name}"),
            &[("main.osk".to_string(), src)],
            "main.osk",
        );
    }
}

// ---- The test executable and the host it runs on -------------------------------

/// `src` built as a test executable (`oska_native::build_tests`) at a
/// path named after `name`.
fn test_executable(name: &str, src: &str) -> PathBuf {
    let program = oska_check::check_module(src)
        .program
        .expect("the program should check");
    let bytes = oska_native::build_tests(&program, oska_native::Target::LinuxAarch64)
        .expect("the tests should build");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("native_tests_exe");
    std::fs::create_dir_all(&dir).expect("should create the directory");
    let exe = dir.join(name);
    let _ = std::fs::remove_file(&exe);
    std::fs::write(&exe, bytes).expect("should write the executable");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
        .expect("should make it executable");
    exe
}

const TWO_TESTS: &str = r#"
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("main ran")
}

test "prints" {
    let _ = t.stdout.println("recorded")
    t.assert_eq(t.printed(), ["recorded"])
}

test "fails" {
    t.fail("on purpose")
}
"#;

#[test]
fn a_test_executable_runs_only_the_test_its_argument_names() {
    let exe = test_executable("two_tests", TWO_TESTS);
    let passed = Command::new(&exe).arg("0").output().expect("should run");
    assert_eq!(passed.status.code(), Some(0));
    assert_eq!(passed.stdout, b"");
    assert_eq!(passed.stderr, b"");
    let failed = Command::new(&exe).arg("1").output().expect("should run");
    assert_eq!(failed.status.code(), Some(101));
    let report = String::from_utf8_lossy(&failed.stdout);
    assert!(
        report.starts_with("error[P0013]: on purpose (bytes "),
        "{report}"
    );
    let failure = oska_native::test_failure(&failed.stdout).expect("a report");
    assert_eq!(
        (failure.code, failure.message.as_str()),
        ("P0013", "on purpose")
    );
    assert_eq!(failure.location, None);
    for args in [&[][..], &["2"], &["-1"], &["x"], &["+1"], &["0", "1"]] {
        let refused = Command::new(&exe).args(args).output().expect("should run");
        assert_eq!(refused.status.code(), Some(2), "{args:?}");
        assert_eq!(refused.stdout, b"", "{args:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).starts_with("error: a test executable takes"),
            "{args:?}"
        );
    }
}

#[test]
fn a_programs_executable_never_runs_its_tests_nor_on_the_test_host() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("native_tests_program");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the directory");
    std::fs::write(dir.join("main.osk"), TWO_TESTS).expect("should write the program");
    let build = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(&dir)
        .args(["build", "main.osk", "-o", "program"])
        .output()
        .expect("oskac should run");
    assert!(build.status.success(), "{build:?}");
    let run = Command::new(dir.join("program"))
        .arg("1")
        .output()
        .expect("should run");
    assert_eq!(run.status.code(), Some(0));
    // Stdout is the real one, not recorded.
    assert_eq!(String::from_utf8_lossy(&run.stdout), "main ran\n");
}

#[test]
fn test_failure_reads_back_only_a_panic_report() {
    let read = oska_native::test_failure(b"error[P0011]: a (bytes 1..2) b (bytes 3..4)\n")
        .expect("a report");
    assert_eq!(read.code, "P0011");
    assert_eq!(read.message, "a (bytes 1..2) b");
    assert_eq!((read.span.start, read.span.end), (3, 4));
    for not_one in [
        &b""[..],
        b"error[P0011]: no span\n",
        b"error[E0390]: a (bytes 1..2)\n",
        b"error[P0011]: a (bytes 1..2)",
        b"the native runtime is out of memory\n",
        b"error[P0011]: a (bytes x..2)\n",
    ] {
        assert!(oska_native::test_failure(not_one).is_none(), "{not_one:?}");
    }
}

#[test]
fn the_native_panic_codes_are_the_interpreters() {
    let interpreters: Vec<&str> = oska_run::codes::ALL
        .iter()
        .copied()
        .filter(|code| code.starts_with('P'))
        .collect();
    assert_eq!(oska_native::codes::PANICS.to_vec(), interpreters);
}

#[test]
fn tasks_in_a_test_reach_the_test_host() {
    let outcome = assert_same_file(
        "tasks",
        r#"
fn shout(out: Stdout, word: String) -> Int {
    let _ = out.println(word)
    word.len()
}

test "tasks print to the recorded stdout" {
    let total = scope |s| {
        let a = s.spawn(|| shout(t.stdout, "one"))
        let b = s.spawn(|| shout(t.stdout, "three"))
        a.join() + b.join()
    }
    t.assert_eq(total, 8)
    t.assert_eq(t.printed(), ["one", "three"])
}

test "a task panics" {
    let _ = scope |s| {
        let task = s.spawn(|| 1 / (t.clock.now_millis()))
        task.join()
    }
}
"#,
    );
    assert!(outcome
        .stdout
        .contains("test tasks print to the recorded stdout ... ok\n"));
}
