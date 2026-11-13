//! The whole corpus is differential (spec #125, #140): every program in
//! the repository runs the same as a native executable as on the
//! interpreter (`differential::assert_same`): the examples, every complete
//! program in the guides, the CLI's fixture files and packages, and the
//! site generator building the real site, byte for byte.
//!
//! A corpus program is one `oskac build` compiles. One the checker
//! refuses, or a library, is not a program and is left out; one the
//! native backend refuses fails the test unless the interpreter refuses it
//! too (a `std` module neither has a runtime for). The `test` blocks of
//! the same corpus are compared by `native_tests.rs`.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;
/// The one skip list of the differential tests, shared with the
/// interpreter's runtime tests.
#[path = "../../oska-run/tests/common/skips.rs"]
mod skips;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use differential::{assert_same, Case};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every file under `dir`, by its path relative to `dir` after `prefix`,
/// sorted.
fn files_under(dir: &Path, prefix: &str, files: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
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
            let bytes = std::fs::read(&path).expect("should read the file");
            files.push((name, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}

/// The entries of `dir` that `keep` accepts, sorted.
fn entries(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| keep(p))
        .collect();
    paths.sort();
    paths
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .expect("a name")
        .to_string_lossy()
        .into_owned()
}

/// `oskac` in `dir` with `args`: its exit code.
fn oskac(dir: &Path, args: &[&str]) -> Option<i32> {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("oskac should run")
        .status
        .code()
}

/// Whether `case` is a program of the corpus: `oskac build` compiles it.
/// A program the native backend refuses fails the test, unless the
/// interpreter refuses it too.
fn is_program(case: &Case, name: &str) -> bool {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("corpus-probe")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in case.files() {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    let exe = dir.with_extension("exe");
    let built = oskac(
        &dir,
        &["build", case.target(), "-o", &exe.to_string_lossy()],
    );
    let program = match built {
        Some(0) => true,
        Some(2) => {
            let interpreted = oskac(&dir, &["run", "--interpret", case.target()]);
            assert_eq!(
                interpreted,
                Some(2),
                "`{name}`: the native backend refuses a program the interpreter runs"
            );
            false
        }
        _ => false,
    };
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&exe);
    program
}

/// Run `case`, named `label` in the skip list, on both backends if it
/// is a program: whether it was.
fn assert_same_if_program(case: &Case, label: &str) -> bool {
    if !is_program(case, label) {
        return false;
    }
    let skip = format!("native_corpus::{label}");
    if !skips::SKIPS.iter().any(|(name, _)| *name == skip) {
        assert_same(case);
    }
    true
}

#[test]
fn every_example_runs_the_same() {
    let examples = repo_root().join("examples");
    let mut count = 0;
    for path in entries(&examples, |p| p.extension().is_some_and(|e| e == "osk")) {
        let name = name_of(&path);
        let src = read(&path);
        let case = Case::file_at(&format!("example-{name}"), &name, &src);
        if assert_same_if_program(&case, &format!("example-{name}")) {
            count += 1;
        }
    }
    // `capabilities.osk` counts the lines of the file it is given, and
    // refuses a missing one and no argument at all.
    let src = read(&examples.join("capabilities.osk"));
    for (label, args) in [
        ("itself", &["capabilities.osk"][..]),
        ("missing", &["nope.txt"]),
    ] {
        let case = Case::file_at(
            &format!("example-capabilities-{label}"),
            "capabilities.osk",
            &src,
        )
        .with_args(args);
        let outcome = assert_same(&case);
        assert!(!outcome.stdout.is_empty() || !outcome.stderr.is_empty());
    }
    for dir in entries(&examples, |p| p.join("oska.toml").is_file()) {
        let name = name_of(&dir);
        let mut files = Vec::new();
        files_under(&dir, &format!("{name}/"), &mut files);
        let case = Case::package_at(&format!("example-{name}"), &name, files);
        if assert_same_if_program(&case, &format!("example-{name}")) {
            count += 1;
        }
    }
    assert!(count >= 3, "{count} example programs");
}

#[test]
fn every_cli_fixture_program_runs_the_same() {
    let fixtures = repo_root().join("crates/oskac/tests/fixtures");
    let mut count = 0;
    for path in entries(&fixtures, |p| p.extension().is_some_and(|e| e == "osk")) {
        let name = name_of(&path);
        let case = Case::file_at(&format!("fixture-{name}"), &name, &read(&path));
        if assert_same_if_program(&case, &format!("fixture-{name}")) {
            count += 1;
        }
    }
    for dir in entries(&fixtures.join("packages"), |p| p.is_dir()) {
        let name = name_of(&dir);
        let mut files = Vec::new();
        files_under(&dir, &format!("{name}/"), &mut files);
        let case = Case::package_at(&format!("fixture-package-{name}"), &name, files);
        if assert_same_if_program(&case, &format!("fixture-package-{name}")) {
            count += 1;
        }
    }
    assert!(count >= 8, "{count} fixture programs");
}

/// A complete program in a guide: its file name and source, and the
/// arguments its `console cmd=run` block passes, if one follows it.
struct GuideProgram {
    name: String,
    file: String,
    src: String,
    args: Vec<String>,
}

/// Every complete Oska program in the guides (`docs/guide`, which
/// `guides.rs` checks): not a `fragment` and not an `error=` block.
fn guide_programs() -> Vec<GuideProgram> {
    let mut pages = Vec::new();
    let mut stack = vec![repo_root().join("docs/guide")];
    while let Some(dir) = stack.pop() {
        for path in entries(&dir, |_| true) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                pages.push(path);
            }
        }
    }
    pages.sort();
    let mut programs: Vec<GuideProgram> = Vec::new();
    for page in pages {
        let text = read(&page);
        let stem = page
            .file_stem()
            .expect("a stem")
            .to_string_lossy()
            .into_owned();
        // The open block's info string and text.
        let mut block: Option<(String, String)> = None;
        let mut index = 0;
        // Whether the block just closed was a program, to give it the
        // arguments of a `console cmd=run` block straight after.
        let mut last_was_program = false;
        for line in text.lines() {
            let fence = line.trim_start();
            match &mut block {
                None if fence.starts_with("```") => {
                    block = Some((fence.to_string(), String::new()));
                }
                Some((info, src)) if fence.starts_with("```") => {
                    let words: Vec<&str> = info.split_whitespace().collect();
                    let is_program = words.first() == Some(&"```oska")
                        && !words
                            .iter()
                            .any(|w| *w == "fragment" || w.starts_with("error="));
                    if is_program {
                        let file = words
                            .iter()
                            .find_map(|w| w.strip_prefix("file="))
                            .unwrap_or("main.osk");
                        programs.push(GuideProgram {
                            name: format!("{stem}-{index}"),
                            file: file.to_string(),
                            src: src.clone(),
                            args: Vec::new(),
                        });
                    } else if last_was_program && words.first() == Some(&"```console") {
                        let run = words.contains(&"cmd=run");
                        if let (true, Some(at)) = (run, words.iter().position(|w| *w == "--")) {
                            if let Some(program) = programs.last_mut() {
                                program.args =
                                    words[at + 1..].iter().map(|w| w.to_string()).collect();
                            }
                        }
                    }
                    last_was_program = is_program;
                    index += 1;
                    block = None;
                }
                Some((_, src)) => {
                    src.push_str(line);
                    src.push('\n');
                }
                None if !fence.is_empty() => last_was_program = false,
                None => {}
            }
        }
    }
    programs
}

#[test]
fn every_guide_program_runs_the_same() {
    let mut count = 0;
    for program in guide_programs() {
        let args: Vec<&str> = program.args.iter().map(String::as_str).collect();
        let case = Case::file_at(
            &format!("guide-{}", program.name),
            &program.file,
            &program.src,
        )
        .with_args(&args);
        if assert_same_if_program(&case, &format!("guide-{}", program.name)) {
            count += 1;
        }
    }
    assert!(count >= 20, "{count} guide programs");
}

/// The repository files the site generator reads, by path relative to
/// the root, with the authority report the Pages workflow writes first.
fn site_inputs() -> Vec<(String, String)> {
    let root = repo_root();
    let mut files = Vec::new();
    files_under(&root.join("docs"), "docs/", &mut files);
    files_under(
        &root.join("site/agent-setup"),
        "site/agent-setup/",
        &mut files,
    );
    for file in [
        "README.md",
        "GLOSSARY.md",
        "Cargo.toml",
        "site/style.css",
        "site/search.js",
    ] {
        files.push((file.to_string(), read(&root.join(file))));
    }
    let authority = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(&root)
        .args(["check", "site/build.osk"])
        .output()
        .expect("oskac check should run");
    assert!(authority.status.success(), "{authority:?}");
    files.push((
        "site/authority.txt".to_string(),
        String::from_utf8(authority.stdout).expect("the report is UTF-8"),
    ));
    files
}

#[test]
fn the_site_generator_builds_the_real_site_byte_for_byte() {
    let build = read(&repo_root().join("site/build.osk"));
    let case = site_inputs().into_iter().fold(
        Case::file_at("site", "site/build.osk", &build),
        |case, (path, text)| case.with_file(&path, &text),
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(0), "{}", outcome.stderr);
    for page in [
        "_site/index.html",
        "_site/authority.html",
        "_site/llms-full.txt",
    ] {
        assert!(
            outcome.tree.get(page).is_some_and(|text| text.is_some()),
            "the site should have {page}"
        );
    }
}

#[test]
fn building_the_site_generator_takes_well_under_a_second() {
    let root = repo_root();
    let src = read(&root.join("site/build.osk"));
    let report =
        oska_check::check_file("site/build.osk", &src, oska_check::CheckOptions::default());
    let program = report.program.expect("the generator checks");
    let started = Instant::now();
    let bytes = oska_native::build(&program, oska_native::Target::LinuxAarch64, &|d| d)
        .expect("the generator builds");
    let took = started.elapsed();
    assert!(!bytes.is_empty());
    // A release build of `oskac` takes about 0.1 s on a Raspberry Pi 5,
    // and this unoptimized test build about 0.6 s: the bound only
    // catches a slowdown by a large factor.
    assert!(
        took < Duration::from_secs(5),
        "building site/build.osk took {took:?}"
    );
}

/// Every label the corpus tests give a program, program or not.
fn corpus_labels() -> Vec<String> {
    let root = repo_root();
    let mut labels = Vec::new();
    for path in entries(&root.join("examples"), |_| true) {
        labels.push(format!("example-{}", name_of(&path)));
    }
    let fixtures = root.join("crates/oskac/tests/fixtures");
    for path in entries(&fixtures, |p| p.is_file()) {
        labels.push(format!("fixture-{}", name_of(&path)));
    }
    for path in entries(&fixtures.join("packages"), |p| p.is_dir()) {
        labels.push(format!("fixture-package-{}", name_of(&path)));
    }
    for program in guide_programs() {
        labels.push(format!("guide-{}", program.name));
    }
    labels
}

#[test]
fn the_skip_list_names_only_programs_of_the_corpus() {
    let labels = corpus_labels();
    for (case, reason) in skips::SKIPS {
        let Some(label) = case.strip_prefix("native_corpus::") else {
            continue;
        };
        assert!(!reason.trim().is_empty(), "`{case}` needs a reason");
        assert!(
            labels.iter().any(|l| l == label),
            "`{case}` names no program of the corpus"
        );
    }
}
