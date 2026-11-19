//! No `machine` fix widens authority (spec #185): "A fix that would change
//! authority is never given the `machine` applicability."
//!
//! Every code's catalog example, and the programs below that show a fix
//! the example does not, is checked; each `machine` fix the check offers
//! is applied, the program is checked again, and the authority report
//! before and after goes through `oska_check::authority_diff`, the module
//! `oskac fix` uses to refuse a widening. A fix that widens belongs in
//! `maybe`, where `oskac fix` never applies it.
//!
//! As in `oskac fix`, a program is compared only once every module
//! parses: a module that does not parse reports no functions, so a syntax
//! fix would make every function look new.

mod common;

use std::path::Path;
use std::process::Command;

use common::Json;
use oska_check::authority_diff::{diff, AuthorityRecord};
use oska_check::{CheckOptions, CheckReport, ModuleSource, Package, MANIFEST_PATH};
use oska_syntax::{Applicability, Diagnostic};

/// Programs that get the fix of a code whose catalog example shows none,
/// as `diagnostic_json.rs` lists them, or whose example's fix happens not
/// to touch authority when another program's does: a fix is classified
/// once, for every program it is offered in.
const FIX_EXAMPLES: [(&str, &str); 3] = [
    (
        "E0265",
        "fn same<T>(a: T, b: T) -> Bool {\n    a == b\n}\n\nfn main() {}\n",
    ),
    // Qualifying `Db` names a capability: `query` then reaches the network.
    (
        "E0340",
        "use std.sql\n\nfn query(db: Db) -> Int {\n    1\n}\n\nfn main() {}\n",
    ),
    (
        "E0400",
        "==> oska.toml <==\n\u{feff}[package]\nname = \"demo\"\nversion = \"0.1.0\"\n==> src/main.osk <==\nfn main() {}\n",
    ),
];

/// Every code the checker's catalog, `docs/checker.md`, has a row for.
fn checker_codes() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc = std::fs::read_to_string(root.join("docs/checker.md")).expect("the checker notes");
    doc.lines()
        .filter_map(|line| {
            let code = line.strip_prefix("| `")?.split('`').next()?;
            (code.len() == 5 && code.starts_with('E')).then(|| code.to_string())
        })
        .collect()
}

/// The example of `code`, as `oskac explain` gives it.
fn example_of(code: &str) -> Option<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["explain", code, "--format=json"])
        .output()
        .expect("oskac should run");
    assert!(output.status.success(), "explain {code}");
    let entry = Json::parse(&String::from_utf8(output.stdout).expect("UTF-8"));
    match entry.get("example") {
        Some(Json::Str(example)) => Some(example.clone()),
        _ => None,
    }
}

/// The example program `body`: a package when it is written as files,
/// each after a `==> path <==` line, or the single file `app.osk`.
fn program_of(body: &str) -> Package {
    if !body.starts_with("==> ") {
        return Package::single_file("app.osk", body);
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        match line
            .strip_prefix("==> ")
            .and_then(|rest| rest.strip_suffix(" <=="))
        {
            Some(path) => files.push((path.to_string(), String::new())),
            None => {
                let (_, text) = files.last_mut().expect("a file");
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    let mut program = Package {
        manifest: None,
        modules: Vec::new(),
    };
    for (path, source) in files {
        if path == MANIFEST_PATH {
            program.manifest = Some(source);
        } else if let Some(stem) = path.strip_suffix(".osk") {
            let module = stem.strip_prefix("src/").unwrap_or(stem).replace('/', ".");
            program.modules.push(ModuleSource {
                module,
                path,
                source,
            });
        }
    }
    program.modules.sort_by(|a, b| a.path.cmp(&b.path));
    program
}

fn check(program: &Package) -> CheckReport {
    oska_check::check_package(program, CheckOptions { release: true })
}

fn parses(program: &Package) -> bool {
    program
        .modules
        .iter()
        .all(|module| oska_syntax::parse_module(&module.source).is_ok())
}

/// `program` with the fix of `diagnostic` applied, or `None` when it
/// edits a file the program does not have.
fn fixed(program: &Package, diagnostic: &Diagnostic) -> Option<Package> {
    let fix = diagnostic.fix.as_ref()?;
    let path = fix
        .path
        .clone()
        .or_else(|| diagnostic.location.as_ref().map(|l| l.path.clone()))?;
    let mut program = program.clone();
    let text = if path == MANIFEST_PATH && program.manifest.is_some() {
        program.manifest.as_mut()?
    } else {
        &mut program.modules.iter_mut().find(|m| m.path == path)?.source
    };
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|edit| std::cmp::Reverse((edit.span.start, edit.span.end)));
    for edit in edits {
        text.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    Some(program)
}

#[test]
fn no_machine_fix_in_the_catalog_widens_authority() {
    let mut programs: Vec<(String, String)> = checker_codes()
        .into_iter()
        .filter_map(|code| example_of(&code).map(|example| (code, example)))
        .collect();
    programs.extend(
        FIX_EXAMPLES
            .iter()
            .map(|(code, example)| (code.to_string(), example.to_string())),
    );
    let mut compared = Vec::new();
    let mut problems = Vec::new();
    for (code, example) in &programs {
        let start = program_of(example);
        let report = check(&start);
        let machine = report.diagnostics.iter().filter(|d| {
            d.fix
                .as_ref()
                .is_some_and(|fix| fix.applicability == Applicability::Machine)
        });
        for diagnostic in machine {
            let Some(after) = fixed(&start, diagnostic) else {
                problems.push(format!(
                    "{code}: the fix of {} edits a file the example lacks",
                    diagnostic.code
                ));
                continue;
            };
            if !parses(&start) {
                continue;
            }
            let difference = diff(
                &AuthorityRecord::of(&report),
                &AuthorityRecord::of(&check(&after)),
            );
            match difference {
                Ok(difference) if difference.fails() => problems.push(format!(
                    "{code}'s example: the `machine` fix of {} widens authority: {}",
                    diagnostic.code,
                    difference.widenings().join("; ")
                )),
                Ok(_) => compared.push(diagnostic.code),
                Err(message) => problems.push(format!("{code}'s example: {message}")),
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    // The comparison ran, and on a fix that is about authority: `E0210`
    // declares a type `cap`, which its report already counts.
    assert!(compared.contains(&"E0210"), "{compared:?}");
}

// A module wrapped in one Markdown fence (ticket #449): the checker
// refuses it with `E0001` on the first backtick, and its `machine` fix
// removes the opening and the closing fence lines.

/// The unfenced module the fence cases wrap: it checks with no diagnostic.
const UNFENCED: &str = "fn main() {\n    let x = 1\n}\n";

/// `oskac args` run in `dir`.
fn oskac_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// What `oskac check app.osk --format=json` says of `source`: the exit
/// code and stdout.
fn check_json(dir: &Path, source: &str) -> (Option<i32>, String) {
    std::fs::write(dir.join("app.osk"), source).expect("should write app.osk");
    let output = oskac_in(dir, &["check", "app.osk", "--format=json"]);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    (output.status.code(), stdout)
}

/// The one diagnostic `oskac check` reports on `source`, as JSON.
fn only_diagnostic(dir: &Path, source: &str) -> Json {
    let (_, stdout) = check_json(dir, source);
    match Json::parse(&stdout).get("diagnostics") {
        Some(Json::Arr(list)) if list.len() == 1 => list[0].clone(),
        other => panic!("one diagnostic expected: {other:?}"),
    }
}

/// `source` in `app.osk` after `oskac fix app.osk`, checked as the
/// unfenced module is: the file afterwards, and the same check result.
fn fence_fixed(name: &str, source: &str) -> String {
    let dir = common::portable::Scratch::new("machine_fixes", name);
    let diagnostic = only_diagnostic(&dir, source);
    assert_eq!(diagnostic.get("code"), Some(&Json::Str("E0001".into())));
    let applicability = diagnostic
        .get("fix")
        .and_then(|fix| fix.get("applicability"))
        .cloned();
    assert_eq!(
        applicability,
        Some(Json::Str("machine".into())),
        "{diagnostic:?}"
    );
    std::fs::write(dir.join("app.osk"), source).expect("should write app.osk");
    let fix = oskac_in(&dir, &["fix", "app.osk"]);
    assert!(fix.status.success(), "oskac fix: {fix:?}");
    let after = std::fs::read_to_string(dir.join("app.osk")).expect("should read app.osk");
    let fixed_check = check_json(&dir, &after);
    assert_eq!(fixed_check, check_json(&dir, UNFENCED));
    after
}

#[test]
fn a_fence_with_an_info_string_is_removed_by_its_machine_fix() {
    let fenced = format!("```oska\n{UNFENCED}```\n");
    assert_eq!(fence_fixed("info", &fenced), UNFENCED);
}

#[test]
fn a_fence_with_no_info_string_is_removed_by_its_machine_fix() {
    let fenced = format!("````\n{UNFENCED}````\n");
    assert_eq!(fence_fixed("bare", &fenced), UNFENCED);
}

#[test]
fn a_fence_with_windows_line_endings_is_removed_by_its_machine_fix() {
    let unfenced = UNFENCED.replace('\n', "\r\n");
    let fenced = format!("```oska\r\n{unfenced}```\r\n");
    let dir = common::portable::Scratch::new("machine_fixes", "crlf");
    let diagnostic = only_diagnostic(&dir, &fenced);
    assert!(diagnostic.get("fix").is_some(), "{diagnostic:?}");
    std::fs::write(dir.join("app.osk"), &fenced).expect("should write app.osk");
    assert!(oskac_in(&dir, &["fix", "app.osk"]).status.success());
    let after = std::fs::read_to_string(dir.join("app.osk")).expect("should read app.osk");
    assert_eq!(after, unfenced);
}

/// The fix of the one diagnostic `oskac check` reports on `source`.
fn fix_of(name: &str, source: &str) -> (Json, Option<Json>) {
    let dir = common::portable::Scratch::new("machine_fixes", name);
    let diagnostic = only_diagnostic(&dir, source);
    let code = diagnostic.get("code").cloned().expect("a code");
    (code, diagnostic.get("fix").cloned())
}

#[test]
fn a_backtick_that_is_not_a_whole_module_fence_gets_no_fix() {
    let e0001 = Json::Str("E0001".into());
    let cases = [
        ("stray", "fn main() {\n    let x = `1`\n}\n".to_string()),
        ("unclosed", format!("```oska\n{UNFENCED}")),
        ("short_close", format!("````oska\n{UNFENCED}```\n")),
        (
            "text_after",
            format!("```oska\n{UNFENCED}```\nRun it with oskac.\n"),
        ),
        (
            "text_before",
            format!("Here it is:\n```oska\n{UNFENCED}```\n"),
        ),
        (
            "closed_inside",
            format!("```oska\n{UNFENCED}```\n{UNFENCED}```\n"),
        ),
        ("backtick_in_info", format!("```os`ka\n{UNFENCED}```\n")),
        ("two_backticks", format!("``\n{UNFENCED}``\n")),
        ("one_line", "```oska```\n".to_string()),
    ];
    for (name, source) in cases {
        assert_eq!(fix_of(name, &source), (e0001.clone(), None), "{name}");
    }
}
