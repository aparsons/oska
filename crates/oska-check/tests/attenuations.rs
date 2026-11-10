//! Attenuations in the authority report (#98, ADR 0008): every call of a
//! prelude attenuation method (`restrict_to`, `restrict_to_file`,
//! `read_only`, `write_only`) is listed with the function it is in, the
//! kind it narrows from and to, and its root when that root is a string
//! literal or a module constant.

mod common;

use common::{assert_report_json, check};
use oska_check::AttenuationRoot::{Absent, Computed, Literal};

/// What the report says about `src`'s attenuations: function, method,
/// source kind, result kind, root, and the source text the span covers.
type Entry = (
    String,
    String,
    String,
    String,
    oska_check::AttenuationRoot,
    String,
);

fn attenuations(src: &str) -> Vec<Entry> {
    check(src)
        .attenuations
        .into_iter()
        .map(|a| {
            let text = src[a.span.start..a.span.end].to_string();
            (a.function, a.method, a.source, a.result, a.root, text)
        })
        .collect()
}

fn entry(
    function: &str,
    method: &str,
    source: &str,
    result: &str,
    root: oska_check::AttenuationRoot,
    text: &str,
) -> Entry {
    (
        function.to_string(),
        method.to_string(),
        source.to_string(),
        result.to_string(),
        root,
        text.to_string(),
    )
}

// ---- Roots ---------------------------------------------------------------------------

#[test]
fn a_literal_root_is_listed_with_its_text() {
    assert_eq!(
        attenuations("fn docs(read: FsRead) -> DirRead {\n    read.restrict_to(\"docs\")\n}"),
        vec![entry(
            "docs",
            "restrict_to",
            "FsRead",
            "DirRead",
            Literal("docs".to_string()),
            "read.restrict_to(\"docs\")"
        )]
    );
}

#[test]
fn a_constant_root_is_listed_with_the_constants_text() {
    // A module constant cannot be named in a body yet (#83, E0221), but
    // the report still resolves it, so it is right the moment #83 lands.
    let src = "const SITE: String = \"_site\"\n\
               fn out(write: FsWrite) -> DirWrite {\n    write.restrict_to(SITE)\n}";
    assert_eq!(
        attenuations(src),
        vec![entry(
            "out",
            "restrict_to",
            "FsWrite",
            "DirWrite",
            Literal("_site".to_string()),
            "write.restrict_to(SITE)"
        )]
    );
}

#[test]
fn a_computed_root_is_listed_as_computed() {
    assert_eq!(
        attenuations(
            "fn pick(read: FsRead, name: String) -> DirRead {\n    read.restrict_to(name)\n}"
        ),
        vec![entry(
            "pick",
            "restrict_to",
            "FsRead",
            "DirRead",
            Computed,
            "read.restrict_to(name)"
        )]
    );
}

#[test]
fn a_root_with_an_interpolation_is_computed() {
    let found = attenuations(
        "fn pick(read: FsRead, name: String) -> DirRead {\n    read.restrict_to(\"docs/{name}\")\n}",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].4, Computed);
}

#[test]
fn a_local_that_shadows_a_constant_is_computed() {
    let found = attenuations(
        "const SITE: String = \"_site\"\n\
         fn out(write: FsWrite, SITE: String) -> DirWrite {\n    write.restrict_to(SITE)\n}",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].4, Computed);
}

#[test]
fn narrowing_from_an_untrusted_path_is_computed() {
    assert_eq!(
        attenuations(
            "fn user_dir(uploads: Dir, name: Untrusted<String>) -> Dir {\n    \
             uploads.restrict_to(Path.from(name))\n}"
        ),
        vec![entry(
            "user_dir",
            "restrict_to",
            "Dir",
            "Dir",
            Computed,
            "uploads.restrict_to(Path.from(name))"
        )]
    );
}

#[test]
fn narrowing_with_a_literal_path_has_that_literal_as_its_root() {
    let found = attenuations(
        "fn sub(docs: DirRead) -> DirRead {\n    docs.restrict_to(Path.new(\"guide\"))\n}",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].4, Literal("guide".to_string()));
    assert_eq!(
        (found[0].2.as_str(), found[0].3.as_str()),
        ("DirRead", "DirRead")
    );
}

// ---- Methods without a root ----------------------------------------------------------

#[test]
fn read_only_and_write_only_have_no_root() {
    assert_eq!(
        attenuations(
            "fn reader(app: Dir) -> DirRead {\n    app.read_only()\n}\n\
             fn writer(app: Dir) -> DirWrite {\n    app.write_only()\n}\n\
             fn whole(fs: Fs) -> FsRead {\n    fs.read_only()\n}"
        ),
        vec![
            entry(
                "reader",
                "read_only",
                "Dir",
                "DirRead",
                Absent,
                "app.read_only()"
            ),
            entry(
                "writer",
                "write_only",
                "Dir",
                "DirWrite",
                Absent,
                "app.write_only()"
            ),
            entry(
                "whole",
                "read_only",
                "Fs",
                "FsRead",
                Absent,
                "fs.read_only()"
            ),
        ]
    );
}

// ---- Where an entry is listed --------------------------------------------------------

#[test]
fn a_chain_lists_each_step_in_source_order() {
    let found = attenuations(
        "fn chain(fs: Fs) -> DirRead {\n    fs.restrict_to(\"/srv\").read_only().restrict_to(Path.new(\"docs\"))\n}",
    );
    let steps: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|e| (e.1.as_str(), e.2.as_str(), e.3.as_str()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("restrict_to", "Fs", "Dir"),
            ("read_only", "Dir", "DirRead"),
            ("restrict_to", "DirRead", "DirRead"),
        ]
    );
}

#[test]
fn a_method_is_named_type_dot_method() {
    let found = attenuations(
        "struct Site { name: String }\n\
         impl Site {\n    fn out(self, write: FsWrite) -> DirWrite {\n        write.restrict_to(\"_site\")\n    }\n}",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "Site.out");
}

#[test]
fn an_attenuation_inside_a_closure_belongs_to_the_enclosing_function() {
    let found = attenuations(
        "fn make(read: FsRead) -> fn() -> DirRead {\n    || read.restrict_to(\"docs\")\n}",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "make");
}

#[test]
fn a_method_of_that_name_on_a_program_type_is_not_an_attenuation() {
    assert_eq!(
        attenuations(
            "struct Plan { n: Int }\n\
             impl Plan {\n    fn restrict_to(self, name: String) -> Plan { self }\n    fn read_only(self) -> Plan { self }\n}\n\
             fn f(p: Plan) -> Plan {\n    p.restrict_to(\"x\").read_only()\n}"
        ),
        Vec::new()
    );
}

#[test]
fn code_that_attenuates_nothing_lists_nothing() {
    assert_eq!(
        attenuations("fn f(docs: DirRead) -> Result<Int, IoError> {\n    Ok(docs.list_dir(Path.new(\".\"))?.len())\n}"),
        Vec::new()
    );
}

// ---- The JSON report -----------------------------------------------------------------

#[test]
fn the_json_report_has_a_top_level_attenuations_array() {
    let src = "fn f(read: FsRead, dir: Dir, name: String) -> DirRead {\n    \
               let a = read.restrict_to(\"docs\")\n    \
               let b = read.restrict_to(name)\n    \
               dir.read_only()\n}";
    let report = check(src);
    let span = |needle: &str| {
        let start = src.find(needle).expect("in the source");
        (start, start + needle.len())
    };
    let (s1, e1) = span("read.restrict_to(\"docs\")");
    let (s2, e2) = span("read.restrict_to(name)");
    let (s3, e3) = span("dir.read_only()");
    assert_report_json(
        src,
        &format!(
            r#"{{
              "diagnostics": [],
              "functions": [{}],
              "audit": [],
              "attenuations": [
                {{"function":"f","method":"restrict_to","span":{{"start":{s1},"end":{e1}}},"source":"FsRead","result":"DirRead","root":"docs"}},
                {{"function":"f","method":"restrict_to","span":{{"start":{s2},"end":{e2}}},"source":"FsRead","result":"DirRead","root":null}},
                {{"function":"f","method":"read_only","span":{{"start":{s3},"end":{e3}}},"source":"Dir","result":"DirRead","root":null}}
              ]
            }}"#,
            report.functions[0].to_json()
        ),
    );
}

#[test]
fn a_root_is_escaped_in_json() {
    let report = check("fn f(read: FsRead) -> DirRead {\n    read.restrict_to(\"a\\\"b\\\\c\")\n}");
    assert!(
        report.to_json().contains(r#""root":"a\"b\\c""#),
        "{}",
        report.to_json()
    );
}

// ---- The human report ----------------------------------------------------------------

#[test]
fn the_human_report_lists_attenuations_under_their_function() {
    let src = "fn f(read: FsRead, dir: Dir, name: String) -> DirRead {\n    \
               let a = read.restrict_to(\"docs\")\n    \
               let b = read.restrict_to(name)\n    \
               dir.read_only()\n}\n\
               fn g() -> Int { 1 }";
    let human = check(src).authority_to_human("prog.osk");
    let at = |needle: &str| {
        let start = src.find(needle).expect("in the source");
        format!("bytes {}..{}", start, start + needle.len())
    };
    let expected = format!(
        "fn f: FsRead (read), Dir (dir)\n\
         \x20 FsRead -> DirRead: restrict_to \"docs\" ({})\n\
         \x20 FsRead -> DirRead: restrict_to computed at runtime ({})\n\
         \x20 Dir -> DirRead: read_only ({})\n\
         fn g: pure\n",
        at("read.restrict_to(\"docs\")"),
        at("read.restrict_to(name)"),
        at("dir.read_only()")
    );
    assert_eq!(human, expected);
}
