// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s log (#848, spec #842, ADR 0040): typed records in an
//! append-only, hash-chained log, one writer, many readers, and recovery
//! of the longest valid prefix after a crash. The programs are Oska
//! programs. The restart and tamper tests also run natively on Linux and
//! must print the same (`common/mod.rs`); the crash tests run on the
//! interpreter over `oska_run::CrashHost` (`formal/tla/store-log.md`).

mod common;

use common::run_differential;
use oska_run::{run, CrashHost, FakeHost, Tears};

/// The record type and its codec, and `main` with `io`, `read` and `dir`
/// on `/data`; `body` runs inside it.
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
    let codec = store.Codec.new(to_value, from_value)
{body}
    Ok(())
}}
"#
    )
}

/// Print every key `k1` to `k4`, or the error.
const SHOW_ALL: &str = r#"    match store.read(read, codec) {
        Ok(rd) => {
            show(io, rd, "k1")?
            show(io, rd, "k2")?
            show(io, rd, "k3")?
            show(io, rd, "k4")?
        }
        Err(e) => io.println(e.message())?
    }"#;

/// Open for writing and run `ops`, then print "done" or the error.
fn writing(ops: &str) -> String {
    format!(
        r#"    let r = store.open(read, dir, codec, |s: store.Store<Item>| {{
        var st = s
{ops}
        "done"
    }})
    match r {{
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }}"#
    )
}

fn put(key: &str, qty: i64) -> String {
    format!("        let _ = st.put(\"{key}\", Item(name: \"{key}\", qty: {qty}))\n")
}

fn world() -> FakeHost {
    FakeHost::new().with_dir("data")
}

fn stdout(host: &mut FakeHost, body: &str) -> String {
    let out = run_differential(host, &program(body), &[]);
    assert!(
        out.diagnostics.is_empty() && out.panic.is_none() && out.exit_code == 0,
        "{body}: {out:#?}"
    );
    out.stdout
}

fn copy(host: &FakeHost) -> FakeHost {
    FakeHost::from_world(host.world())
}

/// A world whose log holds `puts` puts, `k1` to `k<puts>`.
fn world_with_log(puts: i64) -> FakeHost {
    let mut host = world();
    let ops: String = (1..=puts).map(|i| put(&format!("k{i}"), i)).collect();
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    host
}

fn lines(host: &FakeHost) -> Vec<String> {
    host.file("data/log")
        .expect("the log")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn put_get_and_delete_survive_a_restart() {
    let mut host = world();
    let ops = format!("{}{}{}", put("k1", 1), put("k2", 2), put("k1", 10));
    let ops = format!("{ops}        let _ = st.delete(\"k2\")\n");
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    // A new run is a restart: nothing is left of the first but the files.
    assert_eq!(
        stdout(&mut host, SHOW_ALL),
        "k1=10\nk2=-\nk3=-\nk4=-\n",
        "the log: {:#?}",
        lines(&host)
    );
    // The writer sees what it wrote before, and adds to it.
    let more =
        "        let n = st.len()\n        let _ = st.put(\"k3\", Item(name: \"k3\", qty: n))\n";
    assert_eq!(stdout(&mut host, &writing(more)), "done\n");
    assert_eq!(stdout(&mut host, SHOW_ALL), "k1=10\nk2=-\nk3=1\nk4=-\n");
    assert_eq!(lines(&host).len(), 5);
}

#[test]
fn the_log_is_a_chain_of_hashed_records() {
    let host = world_with_log(2);
    let log = lines(&host);
    assert_eq!(log.len(), 2);
    let (hash1, body1) = log[0].split_once(' ').expect("hash and body");
    let (_, body2) = log[1].split_once(' ').expect("hash and body");
    assert_eq!(hash1.len(), 64);
    assert!(body1.contains(&format!("\"prev\":\"{}\"", "0".repeat(64))));
    assert!(body2.contains(&format!("\"prev\":\"{hash1}\"")), "{body2}");
    assert!(body2.contains("\"seq\":2"));
}

#[test]
fn a_second_writer_is_refused_while_the_first_holds_the_store() {
    let mut host = world();
    let ops = "        let again = store.open(read, dir, codec, |t: store.Store<Item>| 1)\n\
         match again {\n\
         \x20   Ok(_) => { let _ = io.println(\"second writer\") }\n\
         \x20   Err(e) => { let _ = io.println(e.message()) }\n\
         }\n";
    // The inner open runs inside the outer one's closure, which holds the
    // lock. `io` is captured, so the closure is a `cap fn`.
    let body = format!(
        r#"    let r = store.open(read, dir, codec, |s: store.Store<Item>| {{
{ops}        0
    }})
    match r {{
        Ok(_) => io.println("outer done")?
        Err(e) => io.println(e.message())?
    }}"#
    );
    let out = stdout(&mut host, &body);
    assert!(out.contains("L0001"), "{out}");
    assert!(out.ends_with("outer done\n"), "{out}");
}

#[test]
fn a_reader_changes_nothing_and_ignores_a_torn_last_line() {
    let host = world_with_log(2);
    let log = host.file("data/log").expect("log").to_string();
    let torn = format!("{log}abc123 {{\"seq\":3,\"pr");
    let mut host = world().with_file("data/log", &torn);
    assert_eq!(stdout(&mut host, SHOW_ALL), "k1=1\nk2=2\nk3=-\nk4=-\n");
    assert_eq!(host.file("data/log"), Some(torn.as_str()));
}

#[test]
fn opening_for_writing_cuts_a_torn_last_line_off_and_goes_on() {
    let host = world_with_log(2);
    let log = host.file("data/log").expect("log").to_string();
    let mut host = world().with_file("data/log", &format!("{log}abc123 {{\"seq\":3"));
    assert_eq!(stdout(&mut host, &writing(&put("k3", 3))), "done\n");
    assert_eq!(stdout(&mut host, SHOW_ALL), "k1=1\nk2=2\nk3=3\nk4=-\n");
    assert_eq!(lines(&host).len(), 3);
}

// ---- tamper ----------------------------------------------------------------------------

fn opens_with(log: &str) -> String {
    let mut host = world().with_file("data/log", log);
    stdout(&mut host, SHOW_ALL)
}

#[test]
fn a_changed_record_is_an_error_on_open() {
    let host = world_with_log(3);
    let log = lines(&host);
    // The value of the middle record is changed, its hash left alone.
    let mut changed = log.clone();
    changed[1] = changed[1].replace("\"qty\":2", "\"qty\":99");
    let out = opens_with(&format!("{}\n", changed.join("\n")));
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    assert!(out.contains("hash does not match"), "{out}");
    assert!(!out.contains("k1="), "no partial data: {out}");
    // Changed and re-hashed: the next record no longer links to it.
    let (_, body) = log[1].split_once(' ').expect("record");
    let body = body.replace("\"qty\":2", "\"qty\":99");
    let rehashed = format!("{} {body}", sha256_hex(&body));
    let mut forged = log.clone();
    forged[1] = rehashed;
    let out = opens_with(&format!("{}\n", forged.join("\n")));
    assert!(out.starts_with("store: corrupt log: line 3:"), "{out}");
    assert!(out.contains("does not follow"), "{out}");
}

#[test]
fn a_reordered_or_removed_record_is_an_error_on_open() {
    let host = world_with_log(3);
    let log = lines(&host);
    let swapped = [log[0].clone(), log[2].clone(), log[1].clone()];
    let out = opens_with(&format!("{}\n", swapped.join("\n")));
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    let removed = [log[0].clone(), log[2].clone()];
    let out = opens_with(&format!("{}\n", removed.join("\n")));
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
    // A removed first record leaves a chain that starts in the middle.
    let out = opens_with(&format!("{}\n", log[1..].join("\n")));
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
}

#[test]
fn a_damaged_complete_last_record_is_an_error_not_a_torn_write() {
    let host = world_with_log(2);
    let log = lines(&host);
    let mut changed = log.clone();
    changed[1] = changed[1].replace("\"qty\":2", "\"qty\":3");
    let out = opens_with(&format!("{}\n", changed.join("\n")));
    assert!(out.starts_with("store: corrupt log: line 2:"), "{out}");
}

#[test]
fn a_writer_refuses_a_damaged_log_and_leaves_it_alone() {
    let host = world_with_log(2);
    let log = lines(&host);
    let bad = format!("{}\n{}\n", log[1], log[0]);
    let mut host = world().with_file("data/log", &bad);
    let out = stdout(&mut host, &writing(&put("k3", 3)));
    assert!(out.starts_with("store: corrupt log:"), "{out}");
    assert_eq!(host.file("data/log"), Some(bad.as_str()));
}

fn sha256_hex(text: &str) -> String {
    // The log's own hash, through the Oska program that computes it.
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

// ---- crash -----------------------------------------------------------------------------

/// What a store holds after the first `n` of the crash program's writes.
fn state_after(n: usize) -> String {
    // put k1=1, put k2=2, delete k1, put k3=3
    let states = [
        "k1=-\nk2=-\nk3=-\nk4=-\n",
        "k1=1\nk2=-\nk3=-\nk4=-\n",
        "k1=1\nk2=2\nk3=-\nk4=-\n",
        "k1=-\nk2=2\nk3=-\nk4=-\n",
        "k1=-\nk2=2\nk3=3\nk4=-\n",
    ];
    states[n].to_string()
}

/// Four writes, each acknowledged by a printed line once `put` or
/// `delete` returned.
fn crash_program() -> String {
    let ops = r#"        let _ = st.put("k1", Item(name: "a", qty: 1))
        let _ = io.println("ack 1")
        let _ = st.put("k2", Item(name: "b", qty: 2))
        let _ = io.println("ack 2")
        let _ = st.delete("k1")
        let _ = io.println("ack 3")
        let _ = st.put("k3", Item(name: "c", qty: 3))
        let _ = io.println("ack 4")"#;
    program(&format!(
        r#"    let r = store.open(read, dir, codec, |s: store.Store<Item>| {{
        var st = s
{ops}
        "done"
    }})
    match r {{
        Ok(text) => io.println(text)?
        Err(e) => io.println(e.message())?
    }}"#
    ))
}

/// Recover from `fs`: open for writing, then read. The state it shows.
fn recovered(fs: FakeHost) -> (String, FakeHost) {
    let mut host = fs;
    let open = run(&program(&writing("")), &mut host, &["prog.osk".to_string()]);
    assert_eq!(open.stdout, "done\n", "recovery open: {open:#?}");
    let shown = run(&program(SHOW_ALL), &mut host, &["prog.osk".to_string()]);
    assert_eq!(shown.exit_code, 0, "{shown:#?}");
    (shown.stdout, host)
}

fn acked(printed: &[String]) -> usize {
    printed.iter().filter(|l| l.starts_with("ack ")).count()
}

#[test]
fn a_crash_at_every_write_boundary_recovers_to_a_prefix_of_the_acknowledged_writes() {
    let mut host = CrashHost::new(world());
    let src = crash_program();
    let out = run(&src, &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.stdout.lines().count(), 5, "{out:#?}");
    let recording = host.into_recording();
    let mut crashes = 0;
    let mut tail_cuts = 0;
    recording.for_each_crash(Tears::Half, |crash| {
        crashes += 1;
        let n = acked(&crash.printed);
        let (state, _) = recovered(copy(&crash.fs));
        let allowed: Vec<String> = (n..=4).map(state_after).collect();
        assert!(
            allowed.contains(&state),
            "after {}: acknowledged {n}, recovered\n{state}",
            crash.describe()
        );
        // Not more than was ever written, in order: the state is one of
        // the five prefixes, which `allowed` is a suffix of.
        if crash
            .fs
            .file("data/log")
            .is_some_and(|t| !t.is_empty() && !t.ends_with('\n'))
        {
            tail_cuts += 1;
        }
    });
    assert!(crashes > 20, "every step has crashes: {crashes}");
    assert!(tail_cuts > 0, "some crash leaves a torn last line");
}

#[test]
fn a_crash_while_recovering_from_a_torn_log_still_recovers_to_the_prefix() {
    let mut host = CrashHost::new(world());
    let out = run(&crash_program(), &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.exit_code, 0, "{out:#?}");
    let recording = host.into_recording();
    let mut second_crashes = 0;
    recording.for_each_crash(Tears::Half, |crash| {
        let torn = crash
            .fs
            .file("data/log")
            .is_some_and(|t| !t.is_empty() && !t.ends_with('\n'));
        if !torn {
            return;
        }
        let n = acked(&crash.printed);
        // Recover on a crash host, and crash that recovery everywhere.
        let mut again = CrashHost::new(copy(&crash.fs));
        let out = run(
            &program(&writing("")),
            &mut again,
            &["prog.osk".to_string()],
        );
        assert_eq!(out.stdout, "done\n", "{out:#?}");
        again
            .into_recording()
            .for_each_crash(Tears::Half, |second| {
                second_crashes += 1;
                let (state, _) = recovered(copy(&second.fs));
                let allowed: Vec<String> = (n..=4).map(state_after).collect();
                assert!(
                    allowed.contains(&state),
                    "after {} then {}: acknowledged {n}, recovered\n{state}",
                    crash.describe(),
                    second.describe()
                );
            });
    });
    assert!(second_crashes > 0, "some crash left a torn log to recover");
}

#[test]
fn an_early_acknowledgement_would_be_caught_by_the_same_test() {
    // The same check against a program that prints before it syncs: a
    // write counts as acknowledged, and the crash loses it.
    let src = program(
        r#"    let log = dir.restrict_to_file(Path.new("log"))
    log.append("x\n")?
    let _ = io.println("ack 1")
    log.sync()?"#,
    );
    let mut host = CrashHost::new(world());
    let out = run(&src, &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.exit_code, 0, "{out:#?}");
    let mut lost = 0;
    host.into_recording().for_each_crash(Tears::Every, |crash| {
        if acked(&crash.printed) == 1 && crash.fs.file("data/log") != Some("x\n") {
            lost += 1;
        }
    });
    assert!(lost > 0);
}
