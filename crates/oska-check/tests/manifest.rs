//! `oska.toml`, read by our own parser for a strict subset of TOML (spec
//! #115, ticket #117). Text in; a manifest or diagnostics out. Every
//! diagnostic names the manifest's path, line and column.

use oska_check::{parse_manifest, Manifest};
use oska_syntax::Diagnostic;

fn parse(text: &str) -> Result<Manifest, Vec<Diagnostic>> {
    parse_manifest("oska.toml", text)
}

fn manifest(text: &str) -> Manifest {
    parse(text).unwrap_or_else(|d| panic!("{text:?} should parse: {d:#?}"))
}

/// The single diagnostic `text` gives: its code, line, column, and the
/// manifest text it spans.
fn error(text: &str) -> (&'static str, usize, usize, String, Diagnostic) {
    let diagnostics = parse(text).expect_err("the manifest should be refused");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let d = diagnostics.into_iter().next().expect("one diagnostic");
    let location = d.location.clone().expect("a manifest error names its file");
    assert_eq!(location.path, "oska.toml");
    let spanned = text[d.span.start..d.span.end].to_string();
    (d.code, location.line, location.column, spanned, d)
}

#[test]
fn a_package_table_with_a_name_and_a_version_is_a_manifest() {
    let m = manifest("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    assert_eq!(m.name, "demo");
    assert_eq!(m.version, "0.1.0");
}

#[test]
fn comments_blank_lines_literal_strings_and_escapes_are_accepted() {
    let m = manifest(
        "# a package\n\n[package]   # the only table\nname = 'demo_2' # snake\nversion=\"1.0.0-rc.1+build.5\"\n",
    );
    assert_eq!(m.name, "demo_2");
    assert_eq!(m.version, "1.0.0-rc.1+build.5");
    let m = manifest("[package]\nname = \"a\\u0062\"\nversion = \"1.2.3\"\n");
    assert_eq!(m.name, "ab");
}

#[test]
fn an_unknown_key_names_its_line_and_has_a_fix_when_one_key_is_close() {
    let text = "[package]\nname = \"demo\"\nversoin = \"0.1.0\"\n";
    let diagnostics = parse(text).expect_err("refused");
    let unknown = &diagnostics[0];
    let location = unknown.location.as_ref().expect("located");
    assert_eq!(
        (unknown.code, location.line, location.column),
        ("E0402", 3, 1)
    );
    assert_eq!(&text[unknown.span.start..unknown.span.end], "versoin");
    let fix = unknown.fix.as_ref().expect("a close key has a fix");
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].replacement, "version");
}

#[test]
fn an_unknown_key_with_no_close_match_has_no_fix() {
    let (code, _, _, spanned, d) =
        error("[package]\nname = \"demo\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n");
    assert_eq!((code, spanned.as_str()), ("E0402", "license"));
    assert!(d.fix.is_none());
    assert!(d.message.contains("`name`") && d.message.contains("`version`"));
}

#[test]
fn an_unknown_table_is_an_error_with_a_did_you_mean_fix() {
    let text = "[packge]\nname = \"demo\"\nversion = \"0.1.0\"\n";
    let diagnostics = parse(text).expect_err("refused");
    let d = &diagnostics[0];
    let location = d.location.as_ref().expect("located");
    assert_eq!((d.code, location.line, location.column), ("E0402", 1, 2));
    assert_eq!(&text[d.span.start..d.span.end], "packge");
    assert_eq!(
        d.fix.as_ref().expect("a fix").edits[0].replacement,
        "package"
    );
}

#[test]
fn a_key_outside_any_table_is_an_unknown_key() {
    let diagnostics = parse("name = \"demo\"\n").expect_err("refused");
    let d = &diagnostics[0];
    assert_eq!(d.code, "E0402");
    assert_eq!(d.location.as_ref().map(|l| l.line), Some(1));
}

#[test]
fn a_dependencies_table_says_dependencies_arrive_in_milestone_5() {
    let (code, line, column, spanned, d) =
        error("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
    assert_eq!(
        (code, line, column, spanned.as_str()),
        ("E0403", 5, 2, "dependencies")
    );
    assert!(d.message.contains("milestone 5"), "{}", d.message);
}

#[test]
fn unsupported_toml_syntax_is_an_error_that_names_the_feature() {
    let cases = [
        (
            "[package]\nname.first = \"demo\"\n",
            "dotted keys",
            "name.first",
        ),
        ("[package.meta]\n", "dotted keys", "package.meta"),
        ("[package]\nsince = 1979-05-27\n", "dates", "1979-05-27"),
        (
            "[package]\nname = \"\"\"demo\"\"\"\n",
            "multiline strings",
            "\"\"\"",
        ),
        ("[package]\nname = '''demo'''\n", "multiline strings", "'''"),
        (
            "[package]\nname = { first = \"demo\" }\n",
            "inline tables",
            "{",
        ),
        ("[[package]]\n", "arrays of tables", "[[package]]"),
        ("[package]\nname = 3\n", "integers", "3"),
        ("[package]\nname = 1.5\n", "floats", "1.5"),
        ("[package]\nname = true\n", "booleans", "true"),
        (
            "[package]\n\"name\" = \"demo\"\n",
            "quoted keys",
            "\"name\"",
        ),
    ];
    for (text, feature, spanned_text) in cases {
        let diagnostics = parse(text).expect_err("refused");
        let d = &diagnostics[0];
        assert_eq!(d.code, "E0401", "{text:?}: {diagnostics:#?}");
        assert!(d.message.contains(feature), "{text:?}: {}", d.message);
        assert_eq!(&text[d.span.start..d.span.end], spanned_text, "{text:?}");
    }
}

#[test]
fn a_manifest_error_carries_the_line_and_column_of_the_text() {
    let text = "# c\n[package]\nname = \"demo\"\nversion = \"1.0.0\"\nx  = 1979-05-27\n";
    let diagnostics = parse(text).expect_err("refused");
    let d = diagnostics.last().expect("a diagnostic");
    let location = d.location.as_ref().expect("located");
    assert_eq!(d.code, "E0401");
    assert_eq!((location.line, location.column), (5, 6));
    assert_eq!(&text[d.span.start..d.span.end], "1979-05-27");
}

#[test]
fn malformed_text_is_a_syntax_error() {
    let cases = [
        ("[package\n", "[package"),
        ("[package]\nname\n", "name"),
        ("[package]\nname = \"demo\n", "\"demo"),
        ("[package]\nname = \"demo\" extra\n", "extra"),
        ("[package]\nname =\n", ""),
        ("[package]\nname = \"a\\q\"\n", "\\q"),
    ];
    for (text, spanned_text) in cases {
        let diagnostics = parse(text).expect_err("refused");
        let d = &diagnostics[0];
        assert_eq!(d.code, "E0400", "{text:?}: {diagnostics:#?}");
        assert_eq!(&text[d.span.start..d.span.end], spanned_text, "{text:?}");
    }
}

#[test]
fn arrays_of_strings_parse_but_milestone_1_has_no_key_that_takes_one() {
    // The syntax is read (milestone 5 needs it); `name` wants a string.
    let text = "[package]\nname = [\"a\", 'b',\n  \"c\", # note\n]\nversion = \"1.0.0\"\n";
    let diagnostics = parse(text).expect_err("refused");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let d = &diagnostics[0];
    assert_eq!(d.code, "E0405");
    let spanned = &text[d.span.start..d.span.end];
    assert!(
        spanned.starts_with('[') && spanned.ends_with(']'),
        "{spanned}"
    );
    let (code, _, _, _, d) = error("[package]\nname = [\"a\", 3]\nversion = \"1.0.0\"\n");
    assert_eq!(code, "E0401");
    assert!(d.message.contains("integers"), "{}", d.message);
}

#[test]
fn the_package_table_needs_a_name_and_a_version() {
    let (code, line, _, _, d) = error("[package]\nversion = \"0.1.0\"\n");
    assert_eq!((code, line), ("E0404", 1));
    assert!(d.message.contains("`name`"), "{}", d.message);
    let (code, _, _, _, d) = error("[package]\nname = \"demo\"\n");
    assert_eq!(code, "E0404");
    assert!(d.message.contains("`version`"), "{}", d.message);
    let (code, line, column, _, d) = error("# empty\n");
    assert_eq!((code, line, column), ("E0404", 1, 1));
    assert!(d.message.contains("[package]"), "{}", d.message);
}

#[test]
fn a_name_must_be_snake_case() {
    for bad in ["Demo", "my-pkg", "my pkg", "1abc", "a__b", "_a", "a_", ""] {
        let (code, line, column, spanned, _) = error(&format!(
            "[package]\nname = \"{bad}\"\nversion = \"0.1.0\"\n"
        ));
        assert_eq!((code, line, column), ("E0405", 2, 8), "{bad:?}");
        assert_eq!(spanned, format!("\"{bad}\""));
    }
    for good in ["a", "demo", "my_pkg", "a1_b2"] {
        manifest(&format!(
            "[package]\nname = \"{good}\"\nversion = \"0.1.0\"\n"
        ));
    }
}

#[test]
fn a_version_must_be_semver() {
    for bad in [
        "1", "1.0", "01.0.0", "1.0.0.0", "v1.0.0", "1.0.0-", "1.0.0-01", "1.0.0+", "a.b.c",
    ] {
        let (code, line, _, _, d) = error(&format!(
            "[package]\nname = \"demo\"\nversion = \"{bad}\"\n"
        ));
        assert_eq!((code, line), ("E0405", 3), "{bad:?}");
        assert!(d.message.contains("semver"), "{}", d.message);
    }
    for good in [
        "0.0.0",
        "1.2.3",
        "10.20.30",
        "1.0.0-alpha.1",
        "1.0.0-0.3.7",
        "1.0.0+build",
    ] {
        manifest(&format!(
            "[package]\nname = \"demo\"\nversion = \"{good}\"\n"
        ));
    }
}

#[test]
fn a_repeated_key_or_table_is_an_error() {
    let (code, line, _, spanned, _) =
        error("[package]\nname = \"a\"\nname = \"b\"\nversion = \"1.0.0\"\n");
    assert_eq!((code, line, spanned.as_str()), ("E0406", 3, "name"));
    let (code, line, _, spanned, _) =
        error("[package]\nname = \"a\"\nversion = \"1.0.0\"\n[package]\n");
    assert_eq!((code, line, spanned.as_str()), ("E0406", 4, "package"));
}

#[test]
fn every_error_is_reported_not_just_the_first() {
    let diagnostics =
        parse("[package]\nnaem = \"a\"\nversion = \"1\"\n[dependencies]\n").expect_err("refused");
    let found: Vec<(&str, usize)> = diagnostics
        .iter()
        .map(|d| (d.code, d.location.as_ref().map_or(0, |l| l.line)))
        .collect();
    assert_eq!(
        found,
        [("E0402", 2), ("E0405", 3), ("E0403", 4), ("E0404", 1)]
    );
}

// ---- The minimum toolchain: `oska` (spec #191, ticket #204) ---------------------

/// `text` parsed as `running`, the version of the `oskac` reading it.
fn parse_as(running: &str, text: &str) -> Result<Manifest, Vec<Diagnostic>> {
    oska_check::parse_manifest_for("oska.toml", text, running)
}

fn needing(oska: &str) -> String {
    format!("[package]\nname = \"demo\"\nversion = \"0.1.0\"\noska = \"{oska}\"\n")
}

#[test]
fn a_minimum_toolchain_version_is_a_manifest_key_and_is_optional() {
    let m = manifest("[package]\nname = \"demo\"\nversion = \"0.1.0\"\noska = \"0.1.0\"\n");
    assert_eq!(m.oska.as_deref(), Some("0.1.0"));
    let m = manifest("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    assert_eq!(m.oska, None);
}

#[test]
fn a_toolchain_older_than_the_minimum_is_e0407_naming_both_versions() {
    let text = needing("0.5.0");
    let diagnostics = parse_as("0.4.0", &text).expect_err("0.4.0 is older than 0.5.0");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let d = &diagnostics[0];
    let location = d.location.as_ref().expect("located");
    assert_eq!(
        (d.code, location.path.as_str(), location.line),
        ("E0407", "oska.toml", 4)
    );
    assert_eq!(&text[d.span.start..d.span.end], "\"0.5.0\"");
    assert!(
        d.message.contains("0.5.0") && d.message.contains("0.4.0"),
        "{}",
        d.message
    );
    assert!(
        d.message.contains("https://oskalang.dev/install.sh"),
        "{}",
        d.message
    );
    assert!(d.fix.is_none());
}

#[test]
fn a_toolchain_at_or_above_the_minimum_reads_the_manifest() {
    for (running, minimum) in [
        ("0.5.0", "0.5.0"),
        ("0.5.1", "0.5.0"),
        ("1.0.0", "0.9.9"),
        ("0.5.0", "0.5.0-rc.1"),
        ("0.5.0-rc.2", "0.5.0-rc.1"),
        ("0.5.0+build.7", "0.5.0+build.1"),
        ("0.5.0", "0.5.0+build.1"),
    ] {
        let m = parse_as(running, &needing(minimum))
            .unwrap_or_else(|d| panic!("{running} runs a package needing {minimum}: {d:#?}"));
        assert_eq!(m.oska.as_deref(), Some(minimum));
    }
}

#[test]
fn the_minimum_is_compared_by_semver_precedence() {
    for (running, minimum) in [
        ("0.4.0", "0.5.0"),
        ("0.9.0", "0.10.0"),
        ("0.4.0-pre.1", "0.4.0"),
        ("0.3.0", "0.4.0-pre.1"),
        ("0.4.0-alpha", "0.4.0-alpha.1"),
        ("0.4.0-alpha.1", "0.4.0-alpha.beta"),
        ("0.4.0-alpha.2", "0.4.0-alpha.10"),
        ("0.4.0-beta.2", "0.4.0-beta.11"),
        ("0.4.0-rc.1", "0.4.0"),
        ("1.2.3", "2.0.0"),
    ] {
        let diagnostics = parse_as(running, &needing(minimum))
            .expect_err(&format!("{running} is older than {minimum}"));
        assert_eq!(diagnostics[0].code, "E0407", "{running} < {minimum}");
    }
    // And a pre-release sorts above the release before it.
    parse_as("0.4.0-pre.1", &needing("0.3.0")).expect("0.4.0-pre.1 is above 0.3.0");
}

#[test]
fn the_running_toolchain_is_the_version_of_this_build() {
    let version = env!("CARGO_PKG_VERSION");
    manifest(&needing(version));
    let (code, _, _, _, d) = error(&needing("999.0.0"));
    assert_eq!(code, "E0407");
    assert!(d.message.contains(version), "{}", d.message);
}

#[test]
fn a_toolchain_that_is_too_old_reports_only_the_toolchain() {
    // The rest of the manifest may be written for the newer toolchain, so
    // its keys are not reported as unknown.
    let text =
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\noska = \"9.0.0\"\nfuture = \"x\"\n";
    let diagnostics = parse_as("0.4.0", text).expect_err("refused");
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0407"]);
}

#[test]
fn a_minimum_that_is_not_semver_or_not_a_string_is_e0405_like_version() {
    for bad in ["1.0", "latest", ">=0.4.0", "", "v0.4.0"] {
        let (code, line, column, _, d) = error(&needing(bad));
        assert_eq!((code, line, column), ("E0405", 4, 8), "{bad:?}");
        assert!(
            d.message.contains("`oska` must be a semver version"),
            "{}",
            d.message
        );
    }
    let (code, line, _, _, d) =
        error("[package]\nname = \"demo\"\nversion = \"0.1.0\"\noska = [\"0.4.0\"]\n");
    assert_eq!((code, line), ("E0405", 4));
    assert!(
        d.message.contains("`oska` must be a string"),
        "{}",
        d.message
    );
}

#[test]
fn the_minimum_written_twice_is_e0406() {
    let text = format!("{}oska = \"0.2.0\"\n", needing("0.1.0"));
    let (code, line, _, spanned, _) = error(&text);
    assert_eq!((code, line, spanned.as_str()), ("E0406", 5, "oska"));
}

#[test]
fn an_unknown_key_lists_the_minimum_among_the_keys_and_a_typo_of_it_has_a_fix() {
    let (_, _, _, _, d) =
        error("[package]\nname = \"demo\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n");
    assert!(
        d.message.contains("`name`, `version` and `oska`"),
        "{}",
        d.message
    );
    let text = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nosk = \"0.1.0\"\n";
    let (code, _, _, spanned, d) = error(text);
    assert_eq!((code, spanned.as_str()), ("E0402", "osk"));
    assert!(d.message.ends_with("did you mean `oska`?"), "{}", d.message);
    assert_eq!(d.fix.as_ref().expect("a fix").edits[0].replacement, "oska");
}
