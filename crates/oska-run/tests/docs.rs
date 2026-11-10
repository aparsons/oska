//! `docs/runtime.md` documents exactly the codes the runtime defines, each
//! once and in order, and the runtime reports no code that is not defined.

use std::path::{Path, PathBuf};

/// The codes in the first column of `docs/runtime.md`'s tables: cells
/// such as `` `P0001` ``.
fn documented_codes() -> Vec<String> {
    codes_in_doc("../../docs/runtime.md")
}

/// The codes in the first column of the tables of the doc at `relative`.
fn codes_in_doc(relative: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    doc.lines()
        .filter_map(|line| {
            let first = line.trim().strip_prefix('|')?.split('|').next()?.trim();
            let code = first.strip_prefix('`')?.strip_suffix('`')?;
            let is_code = code.len() == 5
                && code.starts_with(['P', 'E'])
                && code[1..].chars().all(|c| c.is_ascii_digit());
            is_code.then(|| code.to_string())
        })
        .collect()
}

#[test]
fn runtime_md_documents_exactly_the_defined_codes() {
    let defined: Vec<String> = oska_run::codes::ALL.iter().map(|c| c.to_string()).collect();
    assert_eq!(
        documented_codes(),
        defined,
        "docs/runtime.md should list every runtime code once, in order"
    );
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("should read the source directory") {
        let path = entry.expect("should read a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The code literals, such as `"P0001"`, in `text`.
fn quoted_codes(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            s.len() == 5 && s.starts_with(['P', 'E']) && s[1..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn the_runtime_writes_no_code_missing_from_the_defined_list() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    for file in files {
        let text = std::fs::read_to_string(&file).expect("should read a source file");
        for code in quoted_codes(&text) {
            assert!(
                oska_run::codes::ALL.contains(&code.as_str()),
                "{} writes the code {code}, which is not in `codes::ALL` or `docs/runtime.md`",
                file.display()
            );
        }
    }
}

#[test]
fn every_runtime_refusal_is_documented_and_no_runtime_code_is_a_checker_code() {
    for code in ["E0370", "E0371", "E0380"] {
        assert!(
            documented_codes().iter().any(|c| c == code),
            "docs/runtime.md should document {code}"
        );
    }
    let checker = codes_in_doc("../../docs/checker.md");
    for code in oska_run::codes::ALL {
        assert!(
            !checker.iter().any(|c| c == code),
            "{code} is documented in docs/checker.md too: runtime and checker codes must differ"
        );
    }
}
