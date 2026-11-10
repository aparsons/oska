//! The one end-to-end test of `oskac parse`: the JSON export of
//! `examples/capabilities.osk` must match the checked-in golden file, and
//! must satisfy the schema invariants in `docs/ast.md`.
//!
//! To regenerate the golden file after an intended format change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test parse_golden` and review the
//! diff.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::Json;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/capabilities.json")
}

fn parse_capabilities_example() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["parse", "examples/capabilities.osk", "--format=json"])
        .current_dir(workspace_root())
        .output()
        .expect("oskac should run");
    assert!(
        output.status.success(),
        "oskac parse failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("output should be UTF-8")
}

#[test]
fn capabilities_example_matches_golden_json() {
    let actual = parse_capabilities_example();
    let golden = golden_path();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden).expect("golden file should exist");
    assert!(
        actual == expected,
        "`oskac parse --format=json examples/capabilities.osk` differs from {}; \
         rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );

    // Schema invariants, over the same output.
    let envelope = Json::parse(&actual);
    assert_eq!(envelope.get("ast_version"), Some(&Json::Num(1)));
    assert_eq!(
        envelope.get("file"),
        Some(&Json::Str("examples/capabilities.osk".into()))
    );
    let Some(Json::Arr(items)) = envelope.get("items") else {
        panic!("the envelope should have an `items` array")
    };
    assert!(!items.is_empty());
    let mut kinds = BTreeSet::new();
    for item in items {
        check_node(item, &mut kinds);
    }

    let docs = std::fs::read_to_string(workspace_root().join("docs/ast.md"))
        .expect("docs/ast.md should exist");
    let undocumented: Vec<&String> = kinds
        .iter()
        .filter(|kind| !docs.contains(&format!("#### `{kind}`")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "node kinds missing a `#### `Kind`` heading in docs/ast.md: {undocumented:?}"
    );
}

/// Assert that `node` has a `kind` and a `span`, then check every node
/// nested in its other fields. Collects the kinds seen.
fn check_node(node: &Json, kinds: &mut BTreeSet<String>) {
    let Json::Obj(fields) = node else {
        panic!("expected a node object, found {node:?}")
    };
    let Some(Json::Str(kind)) = node.get("kind") else {
        panic!("node without a string `kind`: {node:?}")
    };
    let Some(span) = node.get("span") else {
        panic!("`{kind}` node without a `span`")
    };
    let (Some(Json::Num(start)), Some(Json::Num(end))) = (span.get("start"), span.get("end"))
    else {
        panic!("`{kind}` node has a malformed span: {span:?}")
    };
    assert!(start <= end, "`{kind}` span runs backwards");
    assert!(
        !fields.iter().any(|(_, v)| *v == Json::Null),
        "`{kind}` has a null field; omit absent optional fields instead"
    );
    kinds.insert(kind.clone());
    for (key, value) in fields {
        if key == "kind" || key == "span" {
            continue;
        }
        match value {
            Json::Obj(_) => check_node(value, kinds),
            Json::Arr(elements) => {
                for element in elements {
                    check_node(element, kinds);
                }
            }
            _ => {}
        }
    }
}
