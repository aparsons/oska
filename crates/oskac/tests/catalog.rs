//! The error-code catalog (spec #70, ticket #76).
//!
//! Every diagnostic code the toolchain can report is documented in
//! `docs/checker.md` (syntax and checker codes) or `docs/runtime.md`
//! (refusals and panics): a table row says what the code means and
//! whether a machine-applicable fix exists, and a pair of fenced blocks
//! under the table gives a short program that fails with that code and
//! the same program fixed:
//!
//! ```text
//! #### `E0300`
//!
//! ```oska error=E0300
//! ...
//! ```
//!
//! ```oska fixed=E0300
//! ...
//! ```
//! ```
//!
//! These tests keep four things in step: the codes the source defines, the
//! documented entries, `oskac explain`, and the site's `codes.json` and
//! per-code pages. They also run every example against the real
//! toolchain.
//!
//! A block may add the word `release` (checked as `oskac check --release`)
//! or `tests` (run as `oskac test`, for the `P001x` codes that only a
//! `test` block reaches).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use oska_run::{FakeHost, TestOptions, TestStatus};

/// Codes no program can reach, so they have no example: `E0170` is for the
/// prelude only (user code gets `E0132` instead), `E0360` needs a `std`
/// module with a private name (none has one yet), `P0005` is a second
/// line of defence behind the checker (`E0292`), `E0411` is a symlink,
/// which a documentation example cannot write, and `E0391` is an output
/// path, which is no part of a program.
const UNREACHABLE: [&str; 5] = ["E0170", "E0360", "E0391", "E0411", "P0005"];

/// How the meaning of a code with no example ends, after the reason why.
/// `oskac explain` and the code's page both show the meaning.
const NO_EXAMPLE: &str = "so it has no example.";

/// The docs that hold the catalog.
const CATALOG_DOCS: [&str; 2] = ["docs/checker.md", "docs/runtime.md"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn is_code(word: &str) -> bool {
    word.len() == 5 && word.starts_with(['E', 'P']) && word[1..].chars().all(|c| c.is_ascii_digit())
}

// ---- What the source defines ---------------------------------------------------

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {dir:?}: {e}")) {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every code written as a string literal, such as `"E0300"`, in the
/// source of the toolchain's crates.
fn defined_codes() -> BTreeSet<String> {
    let mut files = Vec::new();
    for krate in ["oska-syntax", "oska-check", "oska-run", "oska-native"] {
        rust_files(
            &repo_root().join("crates").join(krate).join("src"),
            &mut files,
        );
    }
    let mut codes = BTreeSet::new();
    for file in files {
        // A code is a quote, five characters, and a quote. Splitting on
        // quotes would lose its place at a `'"'` character literal.
        let text = read(&file);
        for (at, _) in text.match_indices('"') {
            if let Some(literal) = text.get(at + 1..at + 6) {
                if is_code(literal) && text[at + 6..].starts_with('"') {
                    codes.insert(literal.to_string());
                }
            }
        }
    }
    assert!(
        codes.contains("E0001") && codes.contains("E0300") && codes.contains("P0001"),
        "the source should be found and parsed, got {codes:?}"
    );
    codes
}

// ---- What the docs say ---------------------------------------------------------

/// One table row: a code, its meaning, and its fix cell, when the table
/// has one.
struct Row {
    code: String,
    meaning: String,
    fix: Option<String>,
}

/// An example block under a code.
struct Example {
    code: String,
    /// `error` for the failing program, `fixed` for the fix.
    kind: String,
    words: Vec<String>,
    body: String,
    at: String,
}

fn rows_of(doc: &str) -> Vec<Row> {
    doc.lines()
        .filter_map(|line| {
            let inner = line.strip_prefix("| `")?.strip_suffix(" |")?;
            let (code, rest) = inner.split_once("` | ")?;
            if !is_code(code) {
                return None;
            }
            let cells: Vec<&str> = rest.split(" | ").collect();
            let unescape = |cell: &str| cell.replace("\\|", "|");
            Some(Row {
                code: code.to_string(),
                meaning: unescape(cells[0]),
                fix: cells.get(1).map(|c| unescape(c)),
            })
        })
        .collect()
}

fn examples_of(doc: &str, file: &str) -> Vec<Example> {
    let mut out = Vec::new();
    let mut open: Option<Example> = None;
    for (index, line) in doc.lines().enumerate() {
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(info)) => {
                let words: Vec<String> = info.split_whitespace().map(str::to_string).collect();
                let tagged = words.iter().skip(1).find_map(|w| {
                    let (kind, code) = w.split_once('=')?;
                    (["error", "fixed"].contains(&kind) && is_code(code))
                        .then(|| (kind.to_string(), code.to_string()))
                });
                open = Some(match tagged {
                    Some((kind, code)) => Example {
                        code,
                        kind,
                        words,
                        body: String::new(),
                        at: format!("{file}:{}", index + 1),
                    },
                    // Some other block: read it, then drop it.
                    None => Example {
                        code: String::new(),
                        kind: String::new(),
                        words,
                        body: String::new(),
                        at: format!("{file}:{}", index + 1),
                    },
                });
            }
            (Some(_), Some(rest)) if rest.trim().is_empty() => {
                if let Some(done) = open.take().filter(|e| !e.code.is_empty()) {
                    out.push(done);
                }
            }
            (Some(block), _) => {
                block.body.push_str(line);
                block.body.push('\n');
            }
            (None, None) => {}
        }
    }
    assert!(open.is_none(), "{file}: a code block is never closed");
    out
}

struct Catalog {
    rows: Vec<Row>,
    examples: Vec<Example>,
}

fn catalog() -> Catalog {
    let mut rows = Vec::new();
    let mut examples = Vec::new();
    for file in CATALOG_DOCS {
        let doc = read(&repo_root().join(file));
        rows.extend(rows_of(&doc));
        examples.extend(examples_of(&doc, file));
    }
    Catalog { rows, examples }
}

impl Catalog {
    fn codes(&self) -> BTreeSet<String> {
        self.rows.iter().map(|r| r.code.clone()).collect()
    }

    fn example(&self, code: &str, kind: &str) -> Option<&Example> {
        self.examples
            .iter()
            .find(|e| e.code == code && e.kind == kind)
    }
}

// ---- Running an example --------------------------------------------------------

/// The codes `src` is refused or stopped with, per the toolchain, and
/// whether it ran to the end: for a program that fails, the codes of its
/// diagnostics or its panic; for one that does not, none.
fn codes_of(code: &str, example: &Example) -> BTreeSet<String> {
    let src = &example.body;
    let mut found = BTreeSet::new();
    if example.words.iter().any(|w| w == "package") {
        return package_codes_of(code, example);
    }
    if example.words.iter().any(|w| w == "tests") {
        let report = oska_run::test(src, &mut FakeHost::new(), TestOptions::default());
        found.extend(report.diagnostics.iter().map(|d| d.code.to_string()));
        for test in &report.tests {
            assert!(
                test.status != TestStatus::Skipped,
                "{}: a catalog test must not be skipped",
                example.at
            );
            found.extend(test.failure.iter().map(|d| d.code.to_string()));
        }
        assert!(!report.tests.is_empty(), "{}: no tests ran", example.at);
    } else if code.starts_with("E039") {
        // A native build refusal: the program checks, and the backend
        // refuses it.
        let report = oska_check::check_module(src);
        found.extend(report.diagnostics.iter().map(|d| d.code.to_string()));
        if let Some(program) = &report.program {
            if let Err(refusal) = oska_native::build(program) {
                found.insert(refusal.code.to_string());
            }
        }
    } else if code.starts_with('P') || ["E0370", "E0371", "E0380"].contains(&code) {
        let outcome = oska_run::run(src, &mut FakeHost::new(), &["main.osk".to_string()]);
        found.extend(outcome.diagnostics.iter().map(|d| d.code.to_string()));
        found.extend(outcome.panic.iter().map(|d| d.code.to_string()));
    } else {
        let release = example.words.iter().any(|w| w == "release");
        let report = oska_check::check_module_with(src, oska_check::CheckOptions { release });
        found.extend(report.diagnostics.iter().map(|d| d.code.to_string()));
    }
    found
}

/// The files of a package example: a line `==> path <==` starts a file.
fn package_files(body: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        match line
            .strip_prefix("==> ")
            .and_then(|rest| rest.strip_suffix(" <=="))
        {
            Some(path) => files.push((path.to_string(), String::new())),
            None => {
                let (_, text) = files
                    .last_mut()
                    .expect("a package example starts with a `==> path <==` line");
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    files
}

/// The codes a package example gives: its files are written to a
/// directory, and `oskac check` (or `run`, or `test`, as the block says)
/// is run on it.
fn package_codes_of(code: &str, example: &Example) -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("catalog_package_{code}_{}", example.kind));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the temp directory");
    for (path, text) in package_files(&example.body) {
        let file = dir.join(&path);
        std::fs::create_dir_all(file.parent().expect("a parent"))
            .expect("should create a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    let command = ["run", "test"]
        .into_iter()
        .find(|c| {
            example
                .words
                .iter()
                .any(|w| w == c || (*c == "test" && w == "tests"))
        })
        .unwrap_or("check");
    let output = oskac(&[command, &dir.to_string_lossy(), "--format=json"]);
    let mut found = BTreeSet::new();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for (at, _) in text.match_indices("\"code\":\"") {
        found.insert(text[at + 8..at + 13].to_string());
    }
    found
}

// ---- Tests ---------------------------------------------------------------------

#[test]
fn the_docs_document_exactly_the_codes_the_source_defines() {
    let catalog = catalog();
    let listed: Vec<&str> = catalog.rows.iter().map(|r| r.code.as_str()).collect();
    let documented = catalog.codes();
    assert_eq!(
        listed.len(),
        documented.len(),
        "each code should have one row across {CATALOG_DOCS:?}"
    );
    let defined = defined_codes();
    let undocumented: Vec<&String> = defined.difference(&documented).collect();
    let nonexistent: Vec<&String> = documented.difference(&defined).collect();
    assert!(
        undocumented.is_empty() && nonexistent.is_empty(),
        "codes with no row: {undocumented:?}; rows for codes that do not exist: {nonexistent:?}"
    );
}

#[test]
fn every_code_has_a_meaning_and_an_example_with_its_fix() {
    let catalog = catalog();
    let mut problems = Vec::new();
    for row in &catalog.rows {
        if row.meaning.trim().is_empty() {
            problems.push(format!("{}: no meaning", row.code));
        }
        let unreachable = UNREACHABLE.contains(&row.code.as_str());
        for kind in ["error", "fixed"] {
            let count = catalog
                .examples
                .iter()
                .filter(|e| e.code == row.code && e.kind == kind)
                .count();
            let wanted = usize::from(!unreachable);
            if count != wanted {
                problems.push(format!(
                    "{}: expected {wanted} `{kind}=` block, found {count}",
                    row.code
                ));
            }
        }
    }
    let documented = catalog.codes();
    for example in &catalog.examples {
        if !documented.contains(&example.code) {
            problems.push(format!(
                "{}: an example for {}, which has no row",
                example.at, example.code
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn every_example_fails_with_exactly_its_code_and_every_fix_passes() {
    let catalog = catalog();
    let mut problems = Vec::new();
    for example in &catalog.examples {
        let found = codes_of(&example.code, example);
        let expected: BTreeSet<String> = match example.kind.as_str() {
            "error" => BTreeSet::from([example.code.clone()]),
            _ => BTreeSet::new(),
        };
        if found != expected {
            problems.push(format!(
                "{}: {} should give {expected:?}, but gives {found:?}",
                example.at, example.code
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

// ---- oskac explain -------------------------------------------------------------

fn oskac(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

fn stdout_of(args: &[&str]) -> String {
    let output = oskac(args);
    assert!(
        output.status.success(),
        "oskac {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("output should be UTF-8")
}

#[test]
fn explain_prints_the_entry_of_every_code() {
    let catalog = catalog();
    for row in &catalog.rows {
        let human = stdout_of(&["explain", &row.code]);
        assert!(human.starts_with(&format!("{}: ", row.code)), "{human}");
        assert!(human.contains(&row.meaning), "{}: {human}", row.code);
        for kind in ["error", "fixed"] {
            if let Some(example) = catalog.example(&row.code, kind) {
                for line in example.body.lines().filter(|l| !l.is_empty()) {
                    assert!(
                        human.contains(line),
                        "{}: `oskac explain` should show the {kind} example line {line:?}:\n{human}",
                        row.code
                    );
                }
            }
        }
        let json = stdout_of(&["explain", &row.code, "--format=json"]);
        assert!(
            json.starts_with(&format!("{{\"code\":\"{}\",\"message\":\"", row.code)),
            "{json}"
        );
        assert!(json.ends_with("}\n") && json.lines().count() == 1, "{json}");
        let machine = row.fix.as_ref().is_some_and(|f| !f.starts_with("None"));
        assert!(
            json.contains(&format!("\"machine_fix\":{machine},")),
            "{}: {json}",
            row.code
        );
    }
}

#[test]
fn explain_accepts_a_code_in_lower_case() {
    assert_eq!(
        stdout_of(&["explain", "e0300"]),
        stdout_of(&["explain", "E0300"])
    );
}

/// The site generator, written in Oska, has no way to tell one control
/// character from another, so it escapes a line break, a tab, and a
/// carriage return in JSON and nothing else. `oskac explain` escapes all of
/// them. They agree as long as the catalog holds no other.
#[test]
fn the_catalog_docs_hold_no_control_character_but_line_breaks_and_tabs() {
    for file in CATALOG_DOCS {
        let doc = read(&repo_root().join(file));
        let stray: Vec<char> = doc
            .chars()
            .filter(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            .collect();
        assert!(stray.is_empty(), "{file} holds {stray:?}");
    }
}

// ---- The site ------------------------------------------------------------------

/// The real generator, run on the real docs in a fake filesystem: what it
/// writes under `_site/`.
fn built_site() -> BTreeMap<String, String> {
    fn add(host: FakeHost, root: &Path, relative: &str) -> FakeHost {
        let path = root.join(relative);
        if path.is_dir() {
            let mut host = host.with_dir(relative);
            let mut names: Vec<_> = std::fs::read_dir(&path)
                .expect("a readable directory")
                .map(|e| e.expect("a directory entry").file_name())
                .collect();
            names.sort();
            for name in names {
                host = add(
                    host,
                    root,
                    &format!("{relative}/{}", name.to_string_lossy()),
                );
            }
            host
        } else {
            host.with_file(relative, &read(&path))
        }
    }
    let root = repo_root();
    let mut host = FakeHost::new();
    for relative in [
        "docs",
        "README.md",
        "GLOSSARY.md",
        "site/style.css",
        "site/search.js",
        "Cargo.toml",
    ] {
        host = add(host, &root, relative);
    }
    let build = read(&root.join("site/build.osk"));
    let outcome = oska_run::run(&build, &mut host, &["site/build.osk".to_string()]);
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
    host.files_under("_site")
}

#[test]
fn a_code_without_an_example_says_why_wherever_it_is_explained() {
    let catalog = catalog();
    let site = built_site();
    let docs: String = CATALOG_DOCS
        .iter()
        .map(|doc| read(&repo_root().join(doc)))
        .collect();
    let mut problems = Vec::new();
    for row in &catalog.rows {
        let has_example = catalog.examples.iter().any(|e| e.code == row.code);
        if has_example {
            if row.meaning.contains(NO_EXAMPLE) {
                problems.push(format!(
                    "{}: has an example, but says it has none",
                    row.code
                ));
            }
            continue;
        }
        if !row.meaning.contains(NO_EXAMPLE) {
            problems.push(format!(
                "{}: no example, and its meaning does not end `<why>, {NO_EXAMPLE}`",
                row.code
            ));
        }
        let note = format!("`{}` has no example: ", row.code);
        if !docs.contains(&note) {
            problems.push(format!(
                "{}: no `{note}<why>` sentence under its examples heading",
                row.code
            ));
        }
        if !stdout_of(&["explain", &row.code]).contains(NO_EXAMPLE) {
            problems.push(format!("{}: `oskac explain` does not say why", row.code));
        }
        let page = site.get(&format!("codes/{}.html", row.code));
        if !page.is_some_and(|page| page.contains(NO_EXAMPLE)) {
            problems.push(format!("{}: its page does not say why", row.code));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn the_site_has_a_page_per_code_and_codes_json_matches_explain() {
    let catalog = catalog();
    let site = built_site();
    let codes_json = site
        .get("codes.json")
        .expect("the site should publish codes.json");
    assert!(codes_json.starts_with("[\n") && codes_json.ends_with("\n]\n"));
    assert_eq!(
        codes_json.matches("{\"code\":\"").count(),
        catalog.rows.len(),
        "codes.json should hold one entry per code"
    );
    for row in &catalog.rows {
        let entry = stdout_of(&["explain", &row.code, "--format=json"]);
        assert!(
            codes_json.contains(entry.trim_end()),
            "codes.json should hold the entry `oskac explain {} --format=json` prints:\n{entry}",
            row.code
        );
        let page = site
            .get(&format!("codes/{}.html", row.code))
            .unwrap_or_else(|| panic!("the site should have a page for {}", row.code));
        assert!(page.contains(&format!("<h1 id=\"{}\">", row.code.to_lowercase())));
    }
    let index = site.get("codes/index.html").expect("an index of the codes");
    for row in &catalog.rows {
        assert!(
            index.contains(&format!("href=\"/codes/{}.html\"", row.code)),
            "the index should link {}",
            row.code
        );
    }
}
