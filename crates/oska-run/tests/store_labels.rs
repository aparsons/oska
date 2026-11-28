// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s record labels (#850, spec #842, ADR 0040): each record
//! carries a classification (`Public` until ADR 0029 is built) and a trust
//! flag in its authenticated header, and a read restores the higher of the
//! stored label and the capability's. The programs are Oska programs, and
//! each runs on the interpreter and natively (`common/mod.rs`). The tests
//! read the file's bytes and attack the label: edit it, with the hash made
//! to match.

mod common;

use common::{assert_refused, run_differential};
use oska_run::{run, FakeHost};

const KEY: &str = "Secret.new(\"label key\".to_bytes())";

/// `main` with `io`, `read` and `dir` on `/data`, `key`, an untrusted
/// codec `codec` and a validating one `vcodec`; `body` runs inside it.
fn program(body: &str) -> String {
    format!(
        r#"use std.json
use std.store

struct Item {{ name: Untrusted<String>, qty: Int }}

fn to_value(item: Item) -> json.Value {{
    json.Value.Object([
        json.Member(key: "name", value: json.Value.Text(item.name)),
        json.Member(key: "qty", value: json.Value.Number(json.Number.from_int(item.qty))),
    ])
}}

fn from_value(v: json.Value) -> Option<Item> {{
    let name = v.text("name")?
    let qty = v.int("qty")?
    Some(Item(name: name, qty: qty))
}}

fn label_text(l: Option<store.Label>) -> String {{
    match l {{
        Some(label) => {{
            let c = label.classification_name()
            if label.is_trusted() {{ "{{c}} trusted" }} else {{ "{{c}} untrusted" }}
        }}
        None => "none"
    }}
}}

fn show(io: Stdout, rd: store.Reader<Item>, key: String) -> Result<(), IoError> {{
    let l = label_text(rd.label(key))
    io.println("{{key}}: {{l}}")?
    Ok(())
}}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{
    let read = fs.read_only().restrict_to("/data")
    let dir = fs.write_only().restrict_to("/data")
    let key = {KEY}
    let codec = store.Codec.new(to_value, from_value)
    let vcodec = store.Codec.validated(to_value, from_value)
{body}
    Ok(())
}}
"#
    )
}

fn run_with(host: &mut FakeHost, body: &str) -> String {
    let out = run_differential(host, &program(body), &[]);
    assert!(
        out.diagnostics.is_empty() && out.panic.is_none() && out.exit_code == 0,
        "{body}: {out:#?}"
    );
    out.stdout
}

/// Put `alpha` through `codec` (untrusted) and `beta` through `vcodec`.
const WRITE: &str = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("alpha", Item(name: "Zebediah", qty: 7))
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }
    let r2 = store.open(read, dir, key, vcodec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("beta", Item(name: "Zebediah", qty: 8))
        "done"
    })
    match r2 {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;

const SHOW_ALL: &str = r#"    match store.read(read, key, codec) {
        Ok(rd) => {
            show(io, rd, "alpha")?
            show(io, rd, "beta")?
            show(io, rd, "gamma")?
        }
        Err(e) => io.println(e.message())?
    }"#;

fn written() -> FakeHost {
    let mut host = FakeHost::new().with_dir("data");
    assert_eq!(run_with(&mut host, WRITE), "done\ndone\n");
    host
}

fn log(host: &FakeHost) -> Vec<String> {
    host.file("data/log")
        .expect("the log")
        .lines()
        .map(str::to_string)
        .collect()
}

fn body_of(line: &str) -> &str {
    line.split_once(' ').expect("hash and body").1
}

fn sha256_hex(text: &str) -> String {
    let src = format!(
        "use std.sha2\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(sha2.sha256(\"{}\".to_bytes()).to_hex())?\n    Ok(())\n}}\n",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('{', "\\{")
            .replace('}', "\\}")
    );
    let out = run(&src, &mut FakeHost::new(), &["prog.osk".to_string()]);
    assert_eq!(out.exit_code, 0, "{out:#?}");
    out.stdout.trim().to_string()
}

/// A line with its body changed and its hash made to match: what an
/// attacker without the key can do.
fn rehash(body: &str) -> String {
    format!("{} {body}", sha256_hex(body))
}

fn opens_with(lines: &[String]) -> String {
    let mut host = FakeHost::new()
        .with_dir("data")
        .with_file("data/log", &format!("{}\n", lines.join("\n")));
    run_with(&mut host, SHOW_ALL)
}

// ---- the label is written with each record -------------------------------------------

#[test]
fn a_record_written_from_untrusted_text_is_untrusted_on_disk_and_after_a_restart() {
    let mut host = written();
    let lines = log(&host);
    assert!(
        body_of(&lines[0]).contains("\"trust\":\"untrusted\""),
        "{lines:?}"
    );
    assert!(
        body_of(&lines[0]).contains("\"class\":\"public\""),
        "{lines:?}"
    );
    assert_eq!(
        run_with(&mut host, SHOW_ALL),
        "alpha: public untrusted\nbeta: public trusted\ngamma: none\n"
    );
}

#[test]
fn a_record_written_through_a_validating_codec_is_trusted_on_disk() {
    let host = written();
    let lines = log(&host);
    assert!(
        body_of(&lines[1]).contains("\"trust\":\"trusted\""),
        "{lines:?}"
    );
}

#[test]
fn the_text_a_store_gives_back_stays_untrusted() {
    // Stored text decoded from the file never fills a trusted slot.
    let src = program(
        "    match store.read(read, key, codec) {\n        Ok(rd) => {\n            \
         match rd.get(\"alpha\") {\n                Some(item) => {\n                    \
         let n: String = item.name\n                    io.println(n)?\n                }\n                \
         None => {}\n            }\n        }\n        Err(_) => {}\n    }",
    );
    assert_refused(&src, "E0300");
}

#[test]
fn a_writer_sees_the_label_of_each_record_and_a_delete_drops_it() {
    let mut host = written();
    let body = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let before = label_text(st.label("beta"))
        let _ = st.put("beta", Item(name: "again", qty: 9))
        let after = label_text(st.label("beta"))
        let _ = st.delete("alpha")
        let gone = label_text(st.label("alpha"))
        "{before}, {after}, {gone}"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    assert_eq!(
        run_with(&mut host, body),
        "public trusted, public untrusted, none\n"
    );
    assert_eq!(
        run_with(&mut host, SHOW_ALL),
        "alpha: none\nbeta: public untrusted\ngamma: none\n"
    );
}

// ---- the higher of two labels --------------------------------------------------------

#[test]
fn the_higher_of_two_labels_is_untrusted_when_either_is() {
    let body = r#"    let t = store.Label.new(store.Classification.Public, store.Trust.Trusted)
    let u = store.Label.new(store.Classification.Public, store.Trust.Untrusted)
    let a = label_text(Some(store.higher(t, u)))
    let b = label_text(Some(store.higher(u, t)))
    let c = label_text(Some(store.higher(t, t)))
    let d = label_text(Some(store.higher(u, u)))
    io.println("{a}, {b}, {c}, {d}")?"#;
    let mut host = FakeHost::new().with_dir("data");
    assert_eq!(
        run_with(&mut host, body),
        "public untrusted, public untrusted, public trusted, public untrusted\n"
    );
}

#[test]
fn a_write_never_has_a_lower_label_than_a_read_would_give() {
    // A record written untrusted, then read: never trusted.
    let mut host = written();
    let out = run_with(&mut host, SHOW_ALL);
    assert!(out.starts_with("alpha: public untrusted\n"), "{out}");
}

// ---- editing the file cannot change a label ------------------------------------------

#[test]
fn an_untrusted_label_edited_to_trusted_is_detected_on_open() {
    let host = written();
    let mut lines = log(&host);
    let body = body_of(&lines[0]).to_string();
    lines[0] = rehash(&body.replace("\"trust\":\"untrusted\"", "\"trust\":\"trusted\""));
    let out = opens_with(&lines);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    assert!(out.contains("does not open"), "{out}");
    assert!(!out.contains("alpha:"), "no partial data: {out}");
}

#[test]
fn a_trusted_label_edited_to_untrusted_is_detected_on_open() {
    let host = written();
    let mut lines = log(&host);
    let body = body_of(&lines[1]).to_string();
    lines[1] = rehash(&body.replace("\"trust\":\"trusted\"", "\"trust\":\"untrusted\""));
    let out = opens_with(&lines);
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("does not open"), "{out}");
}

#[test]
fn a_label_removed_or_made_unknown_is_detected_on_open() {
    let host = written();
    for edit in [
        ("\"trust\":\"untrusted\"", "\"trust\":\"bogus\""),
        ("\"trust\":\"untrusted\"", "\"trusted\":\"untrusted\""),
        ("\"class\":\"public\"", "\"class\":\"declassified\""),
    ] {
        let mut lines = log(&host);
        let body = body_of(&lines[0]).to_string();
        assert!(body.contains(edit.0), "{body}");
        lines[0] = rehash(&body.replace(edit.0, edit.1));
        let out = opens_with(&lines);
        assert!(
            out.starts_with("store: corrupt log: line 1:"),
            "{edit:?}: {out}"
        );
    }
}

#[test]
fn a_label_copied_from_another_record_is_detected_on_open() {
    // Record two's header under record one's seal: a label moved between
    // records does not open.
    let host = written();
    let lines = log(&host);
    let seal = |l: &str| body_of(l).rsplit_once(' ').expect("seal").1.to_string();
    let header = |l: &str| body_of(l).rsplit_once(' ').expect("header").0.to_string();
    let moved = rehash(&format!("{} {}", header(&lines[1]), seal(&lines[0])));
    let out = opens_with(&[lines[0].clone(), moved]);
    assert!(out.contains("line 2:"), "{out}");
}

#[test]
fn the_label_of_a_record_a_wrong_key_cannot_open_is_not_shown() {
    let host = written();
    let lines = log(&host);
    let mut other = FakeHost::new()
        .with_dir("data")
        .with_file("data/log", &format!("{}\n", lines.join("\n")));
    let src = program(SHOW_ALL).replace(KEY, "Secret.new(\"other key\".to_bytes())");
    let out = run_differential(&mut other, &src, &[]);
    assert!(
        out.stdout.starts_with("store: corrupt log: line 1:"),
        "{out:#?}"
    );
    assert!(!out.stdout.contains("public"), "{out:#?}");
}

// ---- the module keeps its rules ------------------------------------------------------

#[test]
fn the_label_code_adds_no_audit_site() {
    let source = include_str!("../../oska-check/src/std/store.osk");
    assert!(!source.contains(".assume_trusted("));
    let exposures = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains(".expose("))
        .count();
    assert_eq!(exposures, 1);
}
