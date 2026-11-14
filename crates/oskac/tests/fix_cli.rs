//! `oskac fix` (spec #185, ticket #193): applying the fixes that are safe
//! to apply automatically, one at a time with a check after each, and
//! writing each changed file once, or nothing when a rule refuses it.
//!
//! Each case writes its files into a fresh scratch directory, runs the
//! real binary there, and compares the exit code, stdout, stderr and the
//! files afterwards with `tests/golden/fix/<case>.txt`. To regenerate the
//! golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test fix_cli` and review the
//! diff.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh directory for one case's files.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fix_cli_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the scratch directory");
    dir
}

/// `files`, each a path relative to `dir` and its text, written there.
fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(file, text).expect("should write the file");
    }
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

/// The text of `path`, relative to `dir`.
fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).expect("should read the file")
}

/// Every regular file under `dir`, relative to it with `/`, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((at, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&at).expect("should list the directory") {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type().expect("a file type");
            if kind.is_dir() {
                stack.push((entry.path(), relative));
            } else if kind.is_file() {
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

/// Run `oskac args` in `dir` and compare what an agent sees, the exit
/// code, stdout, stderr and every file afterwards, with the golden file
/// `tests/golden/fix/<name>.txt`. Returns the output.
fn assert_golden(name: &str, dir: &Path, args: &[&str]) -> Output {
    let output = oskac(dir, args);
    let mut actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        text(&output.stdout),
        text(&output.stderr),
    );
    for file in files_under(dir) {
        actual.push_str(&format!("--- file {file}\n{}", read(dir, &file)));
    }
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/fix")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
    output
}

/// A program with two semicolons, each a parse error with a `machine`
/// fix: the check reports only the first until it is gone.
const TWO_SEMICOLONS: &str = "\
fn main(out: Stdout) -> Result<(), IoError> {
    let greeting = \"hi\";
    out.println(greeting);
}
";

#[test]
fn several_machine_fixes_end_clean_in_one_call() {
    let dir = scratch("several");
    write(&dir, &[("app.osk", TWO_SEMICOLONS)]);
    let output = assert_golden("several", &dir, &["fix", "app.osk"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        read(&dir, "app.osk"),
        "fn main(out: Stdout) -> Result<(), IoError> {\n    let greeting = \"hi\"\n    out.println(greeting)\n}\n"
    );
}

#[test]
fn json_lists_the_applied_fixes_the_files_and_what_remains() {
    let dir = scratch("several_json");
    write(&dir, &[("app.osk", TWO_SEMICOLONS)]);
    let output = assert_golden("several_json", &dir, &["fix", "app.osk", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
}

/// A `match` that leaves out `None`: `E0253`, whose fix adds the arm with
/// `todo()` as its body, a `placeholders` fix.
const MISSING_ARM: &str = "\
fn count(x: Option<Int>) -> Int {
    match x {
        Some(n) => n
    }
}
";

#[test]
fn a_placeholders_fix_is_not_applied_without_the_flag() {
    let dir = scratch("placeholders_off");
    write(&dir, &[("count.osk", MISSING_ARM)]);
    let output = assert_golden("placeholders_off", &dir, &["fix", "count.osk"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(read(&dir, "count.osk"), MISSING_ARM);
}

#[test]
fn with_placeholders_the_missing_arms_are_added_with_todo() {
    let dir = scratch("placeholders_on");
    write(&dir, &[("count.osk", MISSING_ARM)]);
    let output = assert_golden(
        "placeholders_on",
        &dir,
        &["fix", "count.osk", "--placeholders", "--format=json"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(read(&dir, "count.osk").contains("None => todo()"));
}

#[test]
fn a_maybe_fix_is_never_applied() {
    let dir = scratch("maybe");
    let source = "fn shout(s: Strng) -> String {\n    \"{s}!\"\n}\n";
    write(&dir, &[("shout.osk", source)]);
    for args in [
        &["fix", "shout.osk"][..],
        &["fix", "shout.osk", "--placeholders"][..],
    ] {
        let output = oskac(&dir, args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(text(&output.stdout), "", "nothing is fixed");
    }
    assert_golden("maybe", &dir, &["fix", "shout.osk"]);
    assert_eq!(read(&dir, "shout.osk"), source);
}

/// A package whose entry module has a `;`, and uses a function its other
/// module has not made `pub(pkg)` (`E0206`, whose fix is in that module).
const PACKAGE: &[(&str, &str)] = &[
    (
        "demo/oska.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    ),
    (
        "demo/src/main.osk",
        "use pkg.util.{shout}\n\nfn main(out: Stdout) -> Result<(), IoError> {\n    out.println(shout(\"hi\"));\n}\n",
    ),
    (
        "demo/src/util.osk",
        "fn shout(s: String) -> String {\n    \"{s}!\"\n}\n",
    ),
];

#[test]
fn a_package_fix_edits_two_modules() {
    let dir = scratch("package");
    write(&dir, PACKAGE);
    let output = assert_golden("package", &dir, &["fix", "demo"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(read(&dir, "demo/src/util.osk").starts_with("pub(pkg) fn shout"));
    assert_eq!(oskac(&dir, &["check", "demo"]).status.code(), Some(0));
}

#[test]
fn a_package_fix_in_json_names_files_by_their_package_path() {
    let dir = scratch("package_json");
    write(&dir, PACKAGE);
    let output = assert_golden("package_json", &dir, &["fix", "demo", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn the_current_directory_is_the_package_with_no_path() {
    let dir = scratch("package_here");
    write(&dir, PACKAGE);
    let output = oskac(&dir.join("demo"), &["fix"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "src/main.osk:4:29: fixed E0005: remove `;`\n\
         src/main.osk:1:15: fixed E0206: mark `shout` `pub(pkg)`\n"
    );
}

#[test]
fn dry_run_prints_the_diff_and_writes_nothing() {
    let dir = scratch("dry_run");
    write(&dir, PACKAGE);
    let output = assert_golden("dry_run", &dir, &["fix", "demo", "--dry-run"]);
    assert_eq!(output.status.code(), Some(0));
    for (path, source) in PACKAGE {
        assert_eq!(read(&dir, path), *source, "{path} is unchanged");
    }
}

#[test]
fn dry_run_in_json_gives_each_file_s_diff() {
    let dir = scratch("dry_run_json");
    write(&dir, PACKAGE);
    let output = assert_golden(
        "dry_run_json",
        &dir,
        &["fix", "demo", "--dry-run", "--format=json"],
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dry_run_shows_the_change_the_real_run_makes() {
    let dir = scratch("dry_run_same");
    write(&dir, PACKAGE);
    let dry = text(&oskac(&dir, &["fix", "demo", "--dry-run"]).stdout);
    oskac(&dir, &["fix", "demo"]);
    // Every line the diff adds is in the written files, and every line it
    // removes is gone from them.
    let written = format!(
        "{}{}",
        read(&dir, "demo/src/main.osk"),
        read(&dir, "demo/src/util.osk")
    );
    let added: Vec<&str> = dry
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .collect();
    let removed: Vec<&str> = dry
        .lines()
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .collect();
    assert_eq!(added.len(), 2, "{dry}");
    for line in added {
        assert!(written.lines().any(|w| w == &line[1..]), "{line}");
    }
    for line in removed {
        assert!(!written.lines().any(|w| w == &line[1..]), "{line}");
    }
}

#[test]
fn a_fix_that_would_widen_authority_is_never_applied() {
    let dir = scratch("widening");
    // `E0340`'s fix qualifies `Db` as `sql.Db`, a capability: the function
    // that names it would then reach `NetConnect`, which it did not
    // before. A fix that changes authority is `maybe`, so it is left for
    // the agent to read; the no-widening refusal stays behind it.
    let source = "use std.sql\n\nfn query(db: Db) -> Int {\n    1\n}\n";
    write(&dir, &[("app.osk", source)]);
    let output = assert_golden("widening", &dir, &["fix", "app.osk"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(read(&dir, "app.osk"), source);
    let json = oskac(&dir, &["fix", "app.osk", "--format=json"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(text(&json.stdout).contains("\"applied\": []"));
    assert!(text(&json.stdout).contains("\"written\": false"));
    assert_eq!(read(&dir, "app.osk"), source);
}

/// The programs of the authority-diff fixtures, each a path under
/// `tests/fixtures/authority_diff/` and its text: the same fixtures drive
/// `check --baseline` (`authority_baseline.rs`), `fix`'s no-widening
/// refusal (`fix/rounds.rs`), and this.
fn authority_fixtures() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/authority_diff");
    let mut found = Vec::new();
    for case in std::fs::read_dir(&root).expect("the fixtures") {
        let case = case.expect("a case").path();
        for program in ["before.osk", "after.osk"] {
            let path = case.join(program);
            if let Ok(source) = std::fs::read_to_string(&path) {
                let name = path.strip_prefix(&root).expect("under the root");
                found.push((name.to_string_lossy().replace('\\', "/"), source));
            }
        }
    }
    found.sort();
    assert!(found.len() >= 20, "the fixtures: {found:?}");
    found
}

#[test]
fn a_machine_fix_to_a_program_of_any_authority_is_written() {
    // The rule compares the fixed program with the program as it was, not
    // with how much authority it has: each fixture, wide or narrow, takes
    // a syntax fix and is written back as it was before the `;`.
    for (name, source) in authority_fixtures() {
        let dir = scratch(&format!("authority_{}", name.replace(['/', '.'], "_")));
        let at = source
            .find("\n    ")
            .and_then(|start| source[start + 1..].find('\n').map(|end| start + 1 + end))
            .unwrap_or_else(|| panic!("{name}: an indented line"));
        let broken = format!("{};{}", &source[..at], &source[at..]);
        write(&dir, &[("app.osk", &broken)]);
        let output = oskac(&dir, &["fix", "app.osk"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            text(&output.stderr)
        );
        assert_eq!(read(&dir, "app.osk"), source, "{name}");
    }
}

/// A symlink at `dir/at` to the file `target`, a path from the link's
/// directory, or `false`, saying why, where this OS or user may not make
/// one: Windows without the privilege or developer mode.
fn symlink_file(target: &Path, dir: &Path, at: &str) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, dir.join(at));
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, dir.join(at));
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    match made {
        Ok(()) => true,
        Err(err) => {
            println!("skipped: cannot make a symlink here: {err}");
            false
        }
    }
}

#[test]
fn a_symlinked_file_is_refused_and_its_target_is_untouched() {
    let dir = scratch("symlink_file");
    write(&dir, &[("real/app.osk", TWO_SEMICOLONS)]);
    if !symlink_file(&Path::new("real").join("app.osk"), &dir, "link.osk") {
        return;
    }
    let output = assert_golden("symlink_file", &dir, &["fix", "link.osk"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(read(&dir, "real/app.osk"), TWO_SEMICOLONS);
}

#[test]
fn a_symlinked_module_is_refused_and_its_target_is_untouched() {
    let dir = scratch("symlink_module");
    write(&dir, PACKAGE);
    write(&dir, &[("outside/extra.osk", TWO_SEMICOLONS)]);
    let target = Path::new("..").join("..").join("outside").join("extra.osk");
    if !symlink_file(&target, &dir, "demo/src/extra.osk") {
        return;
    }
    let output = assert_golden("symlink_module", &dir, &["fix", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(read(&dir, "outside/extra.osk"), TWO_SEMICOLONS);
}

#[test]
fn a_missing_file_is_an_error() {
    let dir = scratch("missing");
    let output = oskac(&dir, &["fix", "nope.osk"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).starts_with("error: cannot read nope.osk: "));
}

#[test]
fn more_than_one_path_is_a_usage_error() {
    let dir = scratch("usage");
    write(
        &dir,
        &[("a.osk", TWO_SEMICOLONS), ("b.osk", TWO_SEMICOLONS)],
    );
    let output = oskac(&dir, &["fix", "a.osk", "b.osk"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("oskac fix [path]"));
    assert_eq!(read(&dir, "a.osk"), TWO_SEMICOLONS);
}

#[test]
fn a_clean_program_is_left_alone() {
    let dir = scratch("clean");
    let source = "fn main(out: Stdout) -> Result<(), IoError> {\n    out.println(\"hi\")\n}\n";
    write(&dir, &[("app.osk", source)]);
    let output = oskac(&dir, &["fix", "app.osk", "--format=json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        text(&output.stdout),
        "{\n  \"applied\": [],\n  \"files\": [],\n  \"diagnostics\": [],\n  \"rounds\": 0,\n  \"written\": false\n}\n"
    );
}
