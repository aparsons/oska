//! `docs/runtime.md` documents exactly the codes the runtime defines, each
//! once and in order.

use std::path::Path;

/// The codes in the first column of `docs/runtime.md`'s tables: cells
/// such as `` `P0001` ``.
fn documented_codes() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/runtime.md");
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
