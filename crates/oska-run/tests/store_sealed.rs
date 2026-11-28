// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s sealing (#849, spec #842, ADR 0040): each record is sealed
//! at rest under a key derived from the store key and its sequence number.
//! The programs are Oska programs, and each runs on the interpreter and
//! natively (`common/mod.rs`). The tests read the file's bytes and attack
//! each property: a wrong key, a changed seal, a changed header.

mod common;

use common::{assert_refused, run_differential};
use oska_run::{run, FakeHost};

const RIGHT: &str = "Secret.new(\"right key\".to_bytes())";
const WRONG: &str = "Secret.new(\"wrong key\".to_bytes())";

/// The record type and its codec, and `main` with `io`, `read` and `dir`
/// on `/data` and `key` the given secret; `body` runs inside it.
fn program(key: &str, body: &str) -> String {
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

fn show(io: Stdout, rd: store.Reader<Item>, key: String) -> Result<(), IoError> {{
    match rd.get(key) {{
        Some(item) => {{
            let n = item.qty
            io.println("{{key}}={{n}}")?
        }}
        None => io.println("{{key}}=-")?
    }}
    Ok(())
}}

fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{
    let read = fs.read_only().restrict_to("/data")
    let dir = fs.write_only().restrict_to("/data")
    let key = {key}
    let codec = store.Codec.new(to_value, from_value)
{body}
    Ok(())
}}
"#
    )
}

const SHOW_ALL: &str = r#"    match store.read(read, key, codec) {
        Ok(rd) => {
            show(io, rd, "alpha")?
            show(io, rd, "beta")?
        }
        Err(e) => io.println(e.message())?
    }"#;

const PUT_TWO: &str = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("alpha", Item(name: "Zebediah", qty: 7))
        let _ = st.put("beta", Item(name: "Zebediah", qty: 8))
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;

fn run_with(host: &mut FakeHost, key: &str, body: &str) -> String {
    let out = run_differential(host, &program(key, body), &[]);
    assert!(
        out.diagnostics.is_empty() && out.panic.is_none() && out.exit_code == 0,
        "{body}: {out:#?}"
    );
    out.stdout
}

/// A world with two records sealed under `RIGHT`.
fn sealed_world() -> FakeHost {
    let mut host = FakeHost::new().with_dir("data");
    assert_eq!(run_with(&mut host, RIGHT, PUT_TWO), "done\n");
    host
}

fn log(host: &FakeHost) -> Vec<String> {
    host.file("data/log")
        .expect("the log")
        .lines()
        .map(str::to_string)
        .collect()
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

fn body_of(line: &str) -> &str {
    line.split_once(' ').expect("hash and body").1
}

/// What `SHOW_ALL` prints for a log with these lines.
fn opens_with(key: &str, lines: &[String]) -> String {
    let mut host = FakeHost::new()
        .with_dir("data")
        .with_file("data/log", &format!("{}\n", lines.join("\n")));
    run_with(&mut host, key, SHOW_ALL)
}

fn flip(text: &str) -> String {
    let (head, last) = text.split_at(text.len() - 1);
    format!("{head}{}", if last == "0" { "1" } else { "0" })
}

// ---- a record is not readable without the key ---------------------------------------

#[test]
fn a_record_on_disk_does_not_show_its_key_or_its_value() {
    let host = sealed_world();
    let bytes = host.file("data/log").expect("the log").to_string();
    for secret in ["alpha", "beta", "Zebediah", "qty", "name", "put"] {
        assert!(!bytes.contains(secret), "{secret} is readable: {bytes}");
    }
    // The header is in the clear, so the chain checks without the key.
    let lines = log(&host);
    assert_eq!(lines.len(), 2);
    assert!(body_of(&lines[0]).contains("\"seq\":1"), "{bytes}");
    assert!(body_of(&lines[1]).contains("\"seq\":2"), "{bytes}");
    let hash1 = lines[0].split_once(' ').expect("hash").0;
    assert!(body_of(&lines[1]).contains(&format!("\"prev\":\"{hash1}\"")));
}

#[test]
fn the_right_key_reads_the_records_back_after_a_restart() {
    let mut host = sealed_world();
    assert_eq!(
        run_with(&mut host, RIGHT, SHOW_ALL),
        "alpha=7\nbeta=8\n",
        "{:#?}",
        log(&host)
    );
}

#[test]
fn the_same_record_under_the_same_name_is_sealed_differently_each_time() {
    let mut host = FakeHost::new().with_dir("data");
    let twice = PUT_TWO
        .replace(
            "Item(name: \"Zebediah\", qty: 8)",
            "Item(name: \"Zebediah\", qty: 7)",
        )
        .replace("\"beta\"", "\"alpha\"");
    assert_eq!(run_with(&mut host, RIGHT, &twice), "done\n");
    let lines = log(&host);
    let seal = |l: &str| body_of(l).rsplit_once(' ').expect("seal").1.to_string();
    assert_ne!(seal(&lines[0]), seal(&lines[1]));
}

#[test]
fn the_key_is_in_no_file_of_the_store() {
    let host = sealed_world();
    let world = host.world();
    let all = format!("{world:?}");
    assert!(!all.contains("right key"));
}

// ---- a wrong key fails on open, with no partial data --------------------------------

#[test]
fn a_wrong_key_fails_on_open_and_reveals_nothing() {
    let mut host = sealed_world();
    let out = run_with(&mut host, WRONG, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    assert!(out.contains("does not open"), "{out}");
    assert!(!out.contains("alpha=") && !out.contains("beta="), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[test]
fn a_wrong_key_does_not_open_a_writer_and_leaves_the_log_alone() {
    let mut host = sealed_world();
    let before = host.file("data/log").expect("log").to_string();
    let out = run_with(&mut host, WRONG, PUT_TWO);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    assert_eq!(host.file("data/log"), Some(before.as_str()));
}

#[test]
fn an_empty_key_is_a_wrong_key() {
    let mut host = sealed_world();
    let out = run_with(&mut host, "Secret.new(Bytes.new())", SHOW_ALL);
    assert!(out.contains("does not open"), "{out}");
}

#[test]
fn a_wrong_key_fails_even_when_only_the_last_record_is_sealed_under_it() {
    // Records one and two under the right key, a third appended under
    // another: the open fails on the third and shows nothing.
    let mut host = sealed_world();
    let more = r#"    let r = store.open(read, dir, key, codec, |s: store.Store<Item>| {
        var st = s
        let _ = st.put("gamma", Item(name: "x", qty: 1))
        "done"
    })
    match r {
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }"#;
    // A reader's open with the wrong key fails at line 1, so build the
    // third record under the wrong key on a log that holds only it.
    let mut other = FakeHost::new().with_dir("data");
    assert_eq!(run_with(&mut other, WRONG, more), "done\n");
    let third = log(&other);
    let mut lines = log(&host);
    lines.push(third[0].clone());
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 3:"), "{out}");
    assert!(!out.contains("alpha="), "no partial data: {out}");
    let _ = &mut host;
}

// ---- a changed seal or header is detected --------------------------------------------

#[test]
fn a_changed_seal_is_an_error_even_when_the_hash_is_made_to_match() {
    let host = sealed_world();
    let mut lines = log(&host);
    // The last record: no later record to break the chain.
    lines[1] = rehash(&flip(body_of(&lines[1])));
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("does not open"), "{out}");
    assert!(!out.contains("alpha="), "no partial data: {out}");
}

#[test]
fn a_changed_seal_without_a_new_hash_is_an_error() {
    let host = sealed_world();
    let mut lines = log(&host);
    lines[0] = flip(&lines[0]);
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    assert!(out.contains("hash does not match"), "{out}");
}

#[test]
fn a_changed_sequence_number_is_detected() {
    let host = sealed_world();
    let mut lines = log(&host);
    lines[1] = rehash(&body_of(&lines[1]).replace("\"seq\":2", "\"seq\":3"));
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("sequence number"), "{out}");
}

#[test]
fn a_changed_previous_hash_is_detected() {
    let host = sealed_world();
    let mut lines = log(&host);
    let zeros = "0".repeat(64);
    let body = body_of(&lines[1]).to_string();
    let hash1 = lines[0].split_once(' ').expect("hash").0.to_string();
    lines[1] = rehash(&body.replace(&hash1, &zeros));
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("does not follow"), "{out}");
}

#[test]
fn a_changed_label_in_the_header_is_detected_by_the_seal() {
    let host = sealed_world();
    let mut lines = log(&host);
    let body = body_of(&lines[1]).to_string();
    assert!(body.contains("\"class\":\"public\""), "{body}");
    lines[1] = rehash(&body.replace("\"class\":\"public\"", "\"class\":\"secret\""));
    let out = opens_with(RIGHT, &lines);
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("does not open"), "{out}");
}

#[test]
fn a_seal_moved_under_another_header_is_detected() {
    let host = sealed_world();
    let lines = log(&host);
    let seal = |l: &str| body_of(l).rsplit_once(' ').expect("seal").1.to_string();
    let header = |l: &str| body_of(l).rsplit_once(' ').expect("header").0.to_string();
    // Record two's seal under record one's header, and the other way.
    let a = rehash(&format!("{} {}", header(&lines[0]), seal(&lines[1])));
    let out = opens_with(RIGHT, &[a, lines[1].clone()]);
    assert!(
        out.contains("line 1:") && out.contains("does not open"),
        "{out}"
    );
}

#[test]
fn swapping_or_removing_a_sealed_record_is_detected() {
    let host = sealed_world();
    let lines = log(&host);
    let out = opens_with(RIGHT, &[lines[1].clone(), lines[0].clone()]);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    let out = opens_with(RIGHT, &[lines[1].clone()]);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
}

// ---- the key is a secret -------------------------------------------------------------

#[test]
fn the_store_key_cannot_be_logged_or_compared() {
    let logged = program(RIGHT, "    io.println(\"{key}\")?");
    assert_refused(&logged, "E0350");
    let compared = program(RIGHT, "    let same = key == key");
    assert_refused(&compared, "E0351");
}

#[test]
fn a_plain_text_key_is_not_accepted() {
    let src = program("\"right key\"", SHOW_ALL);
    let out = run(
        &src,
        &mut FakeHost::new().with_dir("data"),
        &["prog.osk".to_string()],
    );
    assert!(!out.diagnostics.is_empty(), "{out:#?}");
}

#[test]
fn the_module_has_one_secret_exposure() {
    let source = include_str!("../../oska-check/src/std/store.osk");
    let uses: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && line.contains(".expose("))
        .collect();
    assert_eq!(uses.len(), 1, "{uses:?}");
    assert!(!source.contains(".assume_trusted("));
}
