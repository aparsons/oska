//! A method called qualified by its type, `Type.method(receiver, ...)`,
//! or taken as a function value, `Type.method`, goes through the same
//! checks as `receiver.method(...)` (review fix F2 of #93): it is listed
//! as an attenuation, recorded as an audit site, and refused with `E0273`
//! when it is a removed path method. Before the fix,
//! `FsRead.restrict_to(r, "/etc")` was missing from the authority report,
//! `String.assume_trusted(a)` was no audit site, and
//! `FsRead.read_to_string(r, p)` got `E0228`.

mod common;

use common::{assert_audit, assert_errors, check};
use oska_check::AttenuationRoot::{self, Absent, Computed, Literal};

/// Each attenuation of `src`: method, source kind, result kind, root, and
/// the source text its span covers.
fn attenuations(src: &str) -> Vec<(String, String, String, AttenuationRoot, String)> {
    let report = check(src);
    assert!(
        report.diagnostics.is_empty(),
        "{src:?} should check cleanly, found {:#?}",
        report.diagnostics
    );
    report
        .attenuations
        .into_iter()
        .map(|a| {
            let text = src[a.span.start..a.span.end].to_string();
            (a.method, a.source, a.result, a.root, text)
        })
        .collect()
}

fn entry(
    method: &str,
    source: &str,
    result: &str,
    root: AttenuationRoot,
    text: &str,
) -> (String, String, String, AttenuationRoot, String) {
    (
        method.to_string(),
        source.to_string(),
        result.to_string(),
        root,
        text.to_string(),
    )
}

// ---- Attenuations ----------------------------------------------------------------------

#[test]
fn a_qualified_restrict_to_is_an_attenuation_with_its_root() {
    // The reviewer's probe: it narrowed to /etc and read the host name,
    // with nothing in the report.
    assert_eq!(
        attenuations("fn etc(r: FsRead) -> DirRead {\n    FsRead.restrict_to(r, \"/etc\")\n}"),
        vec![entry(
            "restrict_to",
            "FsRead",
            "DirRead",
            Literal("/etc".to_string()),
            "FsRead.restrict_to(r, \"/etc\")"
        )]
    );
}

#[test]
fn a_qualified_restrict_to_file_on_a_directory_is_an_attenuation() {
    assert_eq!(
        attenuations(
            "fn page(site: DirWrite) -> FileWrite {\n    \
             DirWrite.restrict_to_file(site, Path.new(\"index.html\"))\n}"
        ),
        vec![entry(
            "restrict_to_file",
            "DirWrite",
            "FileWrite",
            Literal("index.html".to_string()),
            "DirWrite.restrict_to_file(site, Path.new(\"index.html\"))"
        )]
    );
}

#[test]
fn a_qualified_read_only_is_an_attenuation_with_no_root() {
    assert_eq!(
        attenuations("fn reader(fs: Fs) -> FsRead {\n    Fs.read_only(fs)\n}"),
        vec![entry(
            "read_only",
            "Fs",
            "FsRead",
            Absent,
            "Fs.read_only(fs)"
        )]
    );
}

#[test]
fn restrict_to_taken_as_a_value_is_an_attenuation_computed_at_runtime() {
    assert_eq!(
        attenuations(
            "fn etc(r: FsRead) -> DirRead {\n    let narrow = FsRead.restrict_to\n    \
             narrow(r, \"/etc\")\n}"
        ),
        vec![entry(
            "restrict_to",
            "FsRead",
            "DirRead",
            Computed,
            "FsRead.restrict_to"
        )]
    );
}

// ---- Audit sites -----------------------------------------------------------------------

#[test]
fn a_qualified_trust_assertion_is_an_audit_site() {
    assert_audit(
        "fn f(args: Args) -> Option<String> {\n    let a = args.get(1)?\n    \
         Some(String.assume_trusted(a))\n}",
        &[("assume_trusted", "String.assume_trusted(a)")],
    );
}

#[test]
fn a_trust_assertion_taken_as_a_value_is_an_audit_site() {
    assert_audit(
        "fn f(args: Args) -> Option<String> {\n    let a = args.get(1)?\n    \
         let vouch = String.assume_trusted\n    Some(vouch(a))\n}",
        &[("assume_trusted", "String.assume_trusted")],
    );
}

#[test]
fn a_qualified_secret_exposure_is_an_audit_site() {
    assert_audit(
        "fn f(env: Env) -> Option<String> {\n    let key = env.secret(\"KEY\")?\n    \
         Some(Secret.expose(key))\n}",
        &[("expose", "Secret.expose(key)")],
    );
}

// ---- Removed path methods --------------------------------------------------------------

#[test]
fn a_qualified_read_on_fs_read_is_e0273() {
    let src = "fn f(r: FsRead) -> Result<Untrusted<String>, IoError> {\n    \
               FsRead.read_to_string(r, Path.new(\"/etc/hostname\"))\n}";
    assert_errors(
        src,
        &[(
            "E0273",
            "FsRead.read_to_string(r, Path.new(\"/etc/hostname\"))",
        )],
    );
    let message = &check(src).diagnostics[0].message;
    assert!(
        message.contains("`r.restrict_to(\"<root>\").read_to_string(path)`"),
        "{message}"
    );
}

#[test]
fn a_qualified_write_on_fs_write_is_e0273() {
    assert_errors(
        "fn f(w: FsWrite) -> Result<(), IoError> {\n    \
         FsWrite.write(w, Path.new(\"x\"), \"y\")\n}",
        &[("E0273", "FsWrite.write(w, Path.new(\"x\"), \"y\")")],
    );
}

#[test]
fn a_qualified_list_dir_on_fs_is_e0273() {
    assert_errors(
        "fn f(fs: Fs) {\n    let names = Fs.list_dir(fs, Path.new(\".\"))\n}",
        &[("E0273", "Fs.list_dir(fs, Path.new(\".\"))")],
    );
}

#[test]
fn a_removed_method_taken_as_a_value_is_e0273() {
    assert_errors(
        "fn f(r: FsRead) {\n    let read = FsRead.read_to_string\n}",
        &[("E0273", "FsRead.read_to_string")],
    );
}

#[test]
fn a_qualified_read_on_a_directory_capability_still_checks() {
    assert_eq!(
        attenuations(
            "fn f(d: DirRead) -> Result<Untrusted<String>, IoError> {\n    \
             DirRead.read_to_string(d, Path.new(\"a\"))\n}"
        ),
        vec![]
    );
}
