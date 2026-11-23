// SPDX-License-Identifier: Apache-2.0
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
