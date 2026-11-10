//! The site generator, `site/build.osk`, run through the interpreter on a
//! [`FakeHost`].
//!
//! Each directory in `site/tests/` is a golden case: its `input/` files
//! are put in the fake filesystem as if they were the repository, the
//! generator runs with an empty `_site/`, and what it writes there must
//! match `expected/` file for file. To add a case, add a directory with
//! its `input/`; to regenerate `expected/` after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oska-run --test site` and review the
//! diff.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oska_run::{run, FakeHost, RunOutcome};

/// The generator's source.
const BUILD: &str = include_str!("../../../site/build.osk");

/// Where the generator writes, relative to the repository root.
const OUTPUT: &str = "_site";

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/tests")
}

/// Every file under `dir`, by its path relative to `dir`, with `/`
/// between components.
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("should list {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(root, &path, out);
                continue;
            }
            let relative = path.strip_prefix(root).expect("under the root");
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("should read {}: {e}", path.display()));
            out.insert(name, text);
        }
    }
    let mut out = BTreeMap::new();
    if dir.is_dir() {
        walk(dir, dir, &mut out);
    }
    out
}

/// What a run of the generator did: how it ended, and the files it left
/// in `_site/`, by path relative to it.
struct Build {
    outcome: RunOutcome,
    site: BTreeMap<String, String>,
}

/// Run the generator on a repository holding `inputs`, paths relative to
/// its root, with an empty `_site/`.
fn build(inputs: &BTreeMap<String, String>) -> Build {
    let mut host = inputs
        .iter()
        .fold(FakeHost::new().with_dir(OUTPUT), |host, (path, text)| {
            host.with_file(path, text)
        });
    let outcome = run(BUILD, &mut host, &["site/build.osk".to_string()]);
    Build {
        outcome,
        site: host.files_under(OUTPUT),
    }
}

/// [`build`], asserting that the generator succeeded.
fn build_ok(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let inputs = inputs
        .iter()
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    built.site
}

fn assert_succeeded(outcome: &RunOutcome) {
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
}

/// Assert that the generator, run on the case `name`'s `input/`, writes
/// exactly its `expected/`.
fn assert_golden(name: &str) {
    let case = cases_dir().join(name);
    let inputs = read_tree(&case.join("input"));
    assert!(!inputs.is_empty(), "{} has no input files", case.display());
    let built = build(&inputs);
    assert_succeeded(&built.outcome);
    let expected_dir = case.join("expected");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let _ = std::fs::remove_dir_all(&expected_dir);
        for (path, text) in &built.site {
            let file = expected_dir.join(path);
            let parent = file.parent().expect("a file has a parent");
            std::fs::create_dir_all(parent).expect("should create the expected directory");
            std::fs::write(&file, text).expect("should write the expected file");
        }
    }
    assert_eq!(
        built.site,
        read_tree(&expected_dir),
        "`_site/` differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended",
        expected_dir.display()
    );
}

#[test]
fn every_golden_case_builds_its_expected_site() {
    let entries = std::fs::read_dir(cases_dir()).expect("site/tests should exist");
    let mut names: Vec<String> = entries
        .map(|e| e.expect("a directory entry").file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "site/tests has no cases");
    for name in names {
        assert_golden(&name);
    }
}

#[test]
fn the_readme_becomes_the_home_page_and_the_domain_its_cname() {
    let site = build_ok(&[("README.md", "# Oska\n\nA language.\n")]);
    assert_eq!(
        site.keys().map(String::as_str).collect::<Vec<_>>(),
        ["CNAME", "index.html"]
    );
    assert_eq!(site["CNAME"], "oskalang.dev\n");
    assert!(
        site["index.html"].contains("# Oska\n\nA language.\n"),
        "{}",
        site["index.html"]
    );
}

#[test]
fn hostile_markup_in_the_readme_comes_out_escaped() {
    let site = build_ok(&[(
        "README.md",
        "<script>alert(1)</script>\n<img src=x onerror=\"alert(2)\">\n",
    )]);
    let page = &site["index.html"];
    assert!(
        page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{page}"
    );
    assert!(
        page.contains("&lt;img src=x onerror=&quot;alert(2)&quot;&gt;"),
        "{page}"
    );
    assert!(!page.contains("<script"), "{page}");
    assert!(!page.contains("<img"), "{page}");
}

#[test]
fn the_generator_reaches_only_the_filesystem_read_only_and_write_only() {
    let report = oska_check::check_module(BUILD);
    assert_eq!(report.diagnostics, Vec::new());
    let main = report
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("the generator has a `main`");
    assert_eq!(main.authority, ["FsRead", "FsWrite"]);
    for function in &report.functions {
        for capability in &function.authority {
            assert!(
                capability == "FsRead" || capability == "FsWrite",
                "`{}` reaches `{capability}`",
                function.name
            );
        }
    }
}
