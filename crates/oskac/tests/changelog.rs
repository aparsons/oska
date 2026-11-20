//! The changelog (#513, spec #512): `CHANGELOG.md` at the repository root
//! holds every release's notes, and the install guide no longer does.
//!
//! The changelog's `##` headings are `## Unreleased` or `## vX.Y.Z`, with
//! the versions newest first. Each version's first line is its date,
//! `Released YYYY-MM-DD.`. The workspace version in `Cargo.toml` has a
//! section, so a release always has notes. Git merges the changelog with
//! the union driver, which can reorder lines, so the order is checked here.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    let file = repo_root().join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()))
}

/// The workspace version in the manifest, such as `0.6.0`.
fn workspace_version() -> String {
    let manifest = read("Cargo.toml");
    let package = manifest
        .split("[workspace.package]")
        .nth(1)
        .expect("Cargo.toml has a [workspace.package] table");
    package
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .expect("[workspace.package] names a version")
        .trim_matches('"')
        .to_string()
}

/// The headings of `text` that start with `marker`, outside code fences,
/// each with the lines under it up to the next such heading.
fn sections<'a>(text: &'a str, marker: &str) -> Vec<(&'a str, Vec<&'a str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        }
        if !fenced {
            if let Some(title) = line.strip_prefix(marker) {
                out.push((title.trim(), Vec::new()));
                continue;
            }
        }
        if let Some((_, body)) = out.last_mut() {
            body.push(line);
        }
    }
    out
}

/// `X.Y.Z` as numbers, from a heading `vX.Y.Z`.
fn version_of(heading: &str) -> Option<(u64, u64, u64)> {
    let mut parts = heading.strip_prefix('v')?.split('.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    match parts.next() {
        None => Some(version),
        Some(_) => None,
    }
}

/// Whether `line` is `Released YYYY-MM-DD.`.
fn is_released_line(line: &str) -> bool {
    let Some(date) = line
        .strip_prefix("Released ")
        .and_then(|rest| rest.strip_suffix('.'))
    else {
        return false;
    };
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// What is wrong with `changelog` for the workspace version `version`:
/// one line per problem, empty when it is right.
fn changelog_problems(changelog: &str, version: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut previous: Option<(u64, u64, u64)> = None;
    let mut seen_version = false;
    for (index, (heading, body)) in sections(changelog, "## ").iter().enumerate() {
        if *heading == "Unreleased" {
            if index != 0 {
                problems.push("`## Unreleased` must be the first section".to_string());
            }
            continue;
        }
        let Some(number) = version_of(heading) else {
            problems.push(format!(
                "`## {heading}` is neither `## Unreleased` nor `## vX.Y.Z`"
            ));
            continue;
        };
        if let Some(before) = previous {
            if number >= before {
                problems.push(format!(
                    "`## {heading}` is not older than the section above it"
                ));
            }
        }
        previous = Some(number);
        if *heading == format!("v{version}") {
            seen_version = true;
        }
        let first = body.iter().find(|line| !line.trim().is_empty());
        if !first.is_some_and(|line| is_released_line(line)) {
            problems.push(format!(
                "`## {heading}` does not start with `Released YYYY-MM-DD.`"
            ));
        }
    }
    if !seen_version {
        problems.push(format!(
            "no `## v{version}` section for the workspace version in Cargo.toml"
        ));
    }
    problems
}

/// What is wrong with the install guide `guide`: it may not hold an
/// "Upgrading to" section, and it links to the changelog.
fn install_guide_problems(guide: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for marker in ["# ", "## ", "### ", "#### "] {
        for (heading, _) in sections(guide, marker) {
            if heading.starts_with("Upgrading to") {
                problems.push(format!("`{marker}{heading}` belongs in CHANGELOG.md"));
            }
        }
    }
    if !guide.contains("../../CHANGELOG.md") {
        problems.push("the install guide does not link to ../../CHANGELOG.md".to_string());
    }
    problems
}

// ---- The real files ---------------------------------------------------------------

#[test]
fn the_changelog_has_a_dated_section_for_each_version_newest_first() {
    let problems = changelog_problems(&read("CHANGELOG.md"), &workspace_version());
    assert!(
        problems.is_empty(),
        "CHANGELOG.md:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_install_guide_links_to_the_changelog_and_holds_no_upgrading_section() {
    let problems = install_guide_problems(&read("docs/guide/install.md"));
    assert!(
        problems.is_empty(),
        "docs/guide/install.md:\n{}",
        problems.join("\n")
    );
}

// ---- What the checks refuse -------------------------------------------------------

/// A changelog with an unreleased section and two versions, newest first.
const GOOD: &str = "# Changelog\n\nThe notes.\n\n\
## Unreleased\n\n- A change.\n\n\
## v1.10.0\n\nReleased 2026-12-01.\n\nA summary.\n\n### Security fixes\n\n- A fix.\n\n\
## v1.9.0\n\nReleased 2026-11-01.\n\nA summary.\n";

#[test]
fn a_changelog_with_dated_versions_newest_first_passes() {
    assert_eq!(changelog_problems(GOOD, "1.10.0"), Vec::<String>::new());
    assert_eq!(changelog_problems(GOOD, "1.9.0"), Vec::<String>::new());
}

#[test]
fn a_changelog_without_a_section_for_the_workspace_version_fails() {
    assert_eq!(
        changelog_problems(GOOD, "1.11.0"),
        ["no `## v1.11.0` section for the workspace version in Cargo.toml"]
    );
}

#[test]
fn a_version_without_its_released_line_fails() {
    let changelog = GOOD.replace("Released 2026-11-01.", "Summary first.");
    assert_eq!(
        changelog_problems(&changelog, "1.10.0"),
        ["`## v1.9.0` does not start with `Released YYYY-MM-DD.`"]
    );
    let changelog = GOOD.replace("Released 2026-11-01.", "Released 1 November 2026.");
    assert_eq!(
        changelog_problems(&changelog, "1.10.0"),
        ["`## v1.9.0` does not start with `Released YYYY-MM-DD.`"]
    );
}

#[test]
fn a_heading_that_is_neither_unreleased_nor_a_version_fails() {
    for heading in [
        "## Upgrading to v1.9.0",
        "## 1.9.0",
        "## v1.9",
        "## v1.9.0-rc.1",
    ] {
        let changelog = GOOD.replace("## v1.9.0", heading);
        let name = heading.trim_start_matches("## ");
        assert_eq!(
            changelog_problems(&changelog, "1.10.0"),
            [format!(
                "`## {name}` is neither `## Unreleased` nor `## vX.Y.Z`"
            )],
            "{heading}"
        );
    }
}

#[test]
fn versions_out_of_order_fail_compared_as_numbers() {
    // The union merge driver can leave sections out of order.
    let swapped = "## v1.9.0\n\nReleased 2026-11-01.\n\n## v1.10.0\n\nReleased 2026-12-01.\n";
    assert_eq!(
        changelog_problems(swapped, "1.10.0"),
        ["`## v1.10.0` is not older than the section above it"]
    );
    let twice = "## v1.9.0\n\nReleased 2026-11-01.\n\n## v1.9.0\n\nReleased 2026-11-01.\n";
    assert_eq!(
        changelog_problems(twice, "1.9.0"),
        ["`## v1.9.0` is not older than the section above it"]
    );
    let late =
        GOOD.replace("## Unreleased\n\n- A change.\n\n", "") + "\n## Unreleased\n\n- A change.\n";
    assert_eq!(
        changelog_problems(&late, "1.10.0"),
        ["`## Unreleased` must be the first section"]
    );
}

#[test]
fn a_heading_inside_a_code_fence_is_not_a_section() {
    let changelog = GOOD.replace("- A fix.\n", "```markdown\n## Not a heading\n```\n");
    assert_eq!(
        changelog_problems(&changelog, "1.10.0"),
        Vec::<String>::new()
    );
}

#[test]
fn an_install_guide_with_an_upgrading_section_or_no_changelog_link_fails() {
    let guide = "# Install\n\nSee the [changelog](../../CHANGELOG.md).\n\n## Update\n\nRun it.\n";
    assert_eq!(install_guide_problems(guide), Vec::<String>::new());
    let upgrading = format!("{guide}\n## Upgrading to v1.9.0\n\nA change.\n");
    assert_eq!(
        install_guide_problems(&upgrading),
        ["`## Upgrading to v1.9.0` belongs in CHANGELOG.md"]
    );
    let unlinked = guide.replace("../../CHANGELOG.md", "https://example.com/");
    assert_eq!(
        install_guide_problems(&unlinked),
        ["the install guide does not link to ../../CHANGELOG.md"]
    );
}

#[test]
fn git_merges_the_changelog_with_the_union_driver() {
    let attributes = read(".gitattributes");
    assert!(
        attributes
            .lines()
            .any(|line| line.split_whitespace().eq(["CHANGELOG.md", "merge=union"])),
        ".gitattributes:\n{attributes}"
    );
}
