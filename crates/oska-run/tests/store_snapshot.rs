// SPDX-License-Identifier: Apache-2.0
//! `std.store`'s snapshot and fast open (#851, spec #842, ADR 0040): a
//! sealed snapshot of the live records lets the log's front be dropped, and
//! opening reads the snapshot and only the tail of the log. The programs
//! are Oska programs; the ordinary tests also run natively on Linux and
//! must print the same (`common/mod.rs`), and the crash tests run on the
//! interpreter over `oska_run::CrashHost` and walk every crash of the
//! sequence in `formal/tla/store-log.md`: write the temporary snapshot, sync
//! it, rename it, sync the directory, then drop the log's front.

mod common;

use common::run_differential;
use oska_run::{run, CrashHost, FakeHost, Tears};

const KEY: &str = "Secret.new(\"snapshot key\".to_bytes())";

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
            if label.is_trusted() {{ "trusted" }} else {{ "untrusted" }}
        }}
        None => "none"
    }}
}}

fn show(io: Stdout, rd: store.Reader<Item>, key: String) -> Result<(), IoError> {{
    let l = label_text(rd.label(key))
    match rd.get(key) {{
        Some(item) => {{
            let n = item.qty
            io.println("{{key}}={{n}} {{l}}")?
        }}
        None => io.println("{{key}}=-")?
    }}
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

/// Print how the store opened, then every key `k1` to `k4`, or the error.
const SHOW_ALL: &str = r#"    match store.read(read, key, codec) {
        Ok(rd) => {
            let how = rd.opened()
            io.println("opened: {how}")?
            show(io, rd, "k1")?
            show(io, rd, "k2")?
            show(io, rd, "k3")?
            show(io, rd, "k4")?
        }
        Err(e) => io.println(e.message())?
    }"#;

/// Open for writing under `codec` and run `ops`, then print "done" or the
/// error.
fn writing_with(codec: &str, ops: &str) -> String {
    format!(
        r#"    let r = store.open(read, dir, key, {codec}, |s: store.Store<Item>| {{
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

fn writing(ops: &str) -> String {
    writing_with("codec", ops)
}

fn put(key: &str, qty: i64) -> String {
    format!("        let _ = st.put(\"{key}\", Item(name: \"{key}\", qty: {qty}))\n")
}

const SNAPSHOT: &str = "        let _ = st.snapshot()\n";

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

fn lines_of(host: &FakeHost, path: &str) -> Vec<String> {
    host.file(path)
        .unwrap_or_else(|| panic!("{path} is missing"))
        .lines()
        .map(str::to_string)
        .collect()
}

fn log(host: &FakeHost) -> Vec<String> {
    lines_of(host, "data/log")
}

fn flip(text: &str) -> String {
    let (head, last) = text.split_at(text.len() - 1);
    format!("{head}{}", if last == "0" { "1" } else { "0" })
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

/// Three puts, then a snapshot, then a fourth put: the log is the record
/// the snapshot ends at, and the one after it.
fn snapshotted() -> FakeHost {
    let mut host = world();
    let ops = format!(
        "{}{}{}{SNAPSHOT}{}",
        put("k1", 1),
        put("k2", 2),
        put("k3", 3),
        put("k4", 4)
    );
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    host
}

/// A world that holds the snapshot of `snapshotted` and the whole log
/// that a crash before the log was trimmed leaves.
fn untrimmed() -> FakeHost {
    let mut host = world();
    let ops = format!("{}{}{}", put("k1", 1), put("k2", 2), put("k3", 3));
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    let full = host.file("data/log").expect("log").to_string();
    let ops = format!("{SNAPSHOT}{}", put("k4", 4));
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    let snap = host.file("data/snapshot").expect("snapshot").to_string();
    let tail = log(&host);
    let all = format!("{full}{}\n", tail.last().expect("record 4"));
    world()
        .with_file("data/log", &all)
        .with_file("data/snapshot", &snap)
}

const ALL_FOUR: &str =
    "opened: snapshot\nk1=1 untrusted\nk2=2 untrusted\nk3=3 untrusted\nk4=4 untrusted\n";

// ---- fast open -------------------------------------------------------------------------

#[test]
fn a_snapshot_drops_the_logs_front_and_open_reads_the_snapshot_and_the_tail() {
    let mut host = snapshotted();
    // The log keeps the record the snapshot ends at (so a deleted snapshot
    // shows) and the one after it.
    assert_eq!(log(&host).len(), 2, "{:#?}", log(&host));
    assert!(
        host.file("data/snapshot.tmp").is_none(),
        "the temporary file is renamed away"
    );
    assert_eq!(stdout(&mut host, SHOW_ALL), ALL_FOUR);
    // The writer opens from it too, and goes on from the chain.
    assert_eq!(stdout(&mut host, &writing(&put("k5", 5))), "done\n");
    assert_eq!(log(&host).len(), 3);
    assert!(stdout(&mut host, SHOW_ALL).starts_with("opened: snapshot\n"));
}

#[test]
fn a_store_without_a_snapshot_opens_from_the_log() {
    let mut host = world();
    assert_eq!(stdout(&mut host, &writing(&put("k1", 1))), "done\n");
    assert_eq!(
        stdout(&mut host, SHOW_ALL),
        "opened: log\nk1=1 untrusted\nk2=-\nk3=-\nk4=-\n"
    );
    // An empty store has nothing to snapshot.
    let mut host = world();
    assert_eq!(stdout(&mut host, &writing(SNAPSHOT)), "done\n");
    assert!(host.file("data/snapshot").is_none());
}

#[test]
fn open_does_not_read_the_log_before_the_snapshot() {
    // A crash between the rename and the trim leaves the whole log. Its
    // first record is damaged: open goes by the snapshot and never looks.
    let host = untrimmed();
    let mut log = log(&host);
    log[0] = flip(&log[0]);
    let mut host = copy(&host).with_file("data/log", &format!("{}\n", log.join("\n")));
    assert_eq!(stdout(&mut host, SHOW_ALL), ALL_FOUR);
    // Without the snapshot the same log is an error: the test proves the
    // record is damaged, not that the snapshot was ignored.
    let mut bare = world().with_file("data/log", &format!("{}\n", log.join("\n")));
    let out = stdout(&mut bare, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
}

#[test]
fn a_snapshot_the_log_was_not_trimmed_after_opens_and_the_writer_trims() {
    let mut host = untrimmed();
    assert_eq!(log(&host).len(), 4);
    assert_eq!(stdout(&mut host, SHOW_ALL), ALL_FOUR);
    assert_eq!(stdout(&mut host, &writing(SNAPSHOT)), "done\n");
    assert_eq!(log(&host).len(), 1, "{:#?}", log(&host));
    assert_eq!(stdout(&mut host, SHOW_ALL), ALL_FOUR);
}

#[test]
fn a_snapshot_is_sealed_like_a_record() {
    let host = snapshotted();
    let snap = host.file("data/snapshot").expect("snapshot").to_string();
    assert!(!snap.contains("k1"), "no key in the clear: {snap}");
    assert!(!snap.contains("qty"), "no value in the clear: {snap}");
    assert_eq!(snap.lines().count(), 1);
    let line = snap.lines().next().expect("line");
    let (hash, body) = line.split_once(' ').expect("hash and body");
    assert_eq!(hash, sha256_hex(body));
    // The header names the record it ends at and links to it.
    let log = log(&host);
    let (h3, _) = log[0].split_once(' ').expect("record 3");
    assert!(body.contains("\"seq\":3"), "{body}");
    assert!(body.contains(&format!("\"prev\":\"{h3}\"")), "{body}");
    // Taking another snapshot of the same records writes the same bytes.
    let mut again = world();
    let ops = format!(
        "{}{}{}{SNAPSHOT}{SNAPSHOT}",
        put("k1", 1),
        put("k2", 2),
        put("k3", 3)
    );
    assert_eq!(stdout(&mut again, &writing(&ops)), "done\n");
    assert_eq!(again.file("data/snapshot").expect("snapshot"), snap);
    let mut again = copy(&again);
    assert_eq!(stdout(&mut again, &writing(SNAPSHOT)), "done\n");
    assert_eq!(again.file("data/snapshot").expect("snapshot"), snap);
}

#[test]
fn deletes_and_overwrites_are_in_the_snapshot() {
    let mut host = world();
    let ops = format!(
        "{}{}{}        let _ = st.delete(\"k2\")\n{SNAPSHOT}",
        put("k1", 1),
        put("k2", 2),
        put("k1", 10)
    );
    assert_eq!(stdout(&mut host, &writing(&ops)), "done\n");
    assert_eq!(
        stdout(&mut host, SHOW_ALL),
        "opened: snapshot\nk1=10 untrusted\nk2=-\nk3=-\nk4=-\n"
    );
}

// ---- labels ----------------------------------------------------------------------------

#[test]
fn a_snapshot_keeps_each_records_label() {
    let mut host = world();
    // k1 is written untrusted, k2 through the validating codec.
    assert_eq!(stdout(&mut host, &writing(&put("k1", 1))), "done\n");
    let ops = format!("{}{SNAPSHOT}", put("k2", 2));
    assert_eq!(stdout(&mut host, &writing_with("vcodec", &ops)), "done\n");
    // Read through the validating codec: the untrusted record stays so.
    let shown = stdout(
        &mut host,
        &SHOW_ALL.replace(
            "store.read(read, key, codec)",
            "store.read(read, key, vcodec)",
        ),
    );
    assert_eq!(
        shown,
        "opened: snapshot\nk1=1 untrusted\nk2=2 trusted\nk3=-\nk4=-\n"
    );
    // A snapshot taken by a writer that writes trusted records does not lift
    // the label of the old ones.
    let ops = format!("{}{SNAPSHOT}", put("k3", 3));
    assert_eq!(stdout(&mut host, &writing_with("vcodec", &ops)), "done\n");
    let shown = stdout(&mut host, SHOW_ALL);
    assert!(shown.contains("k1=1 untrusted"), "{shown}");
    assert!(shown.contains("k2=2 trusted"), "{shown}");
    assert!(shown.contains("k3=3 trusted"), "{shown}");
}

// ---- tamper ----------------------------------------------------------------------------

fn snapshot_line(host: &FakeHost) -> String {
    lines_of(host, "data/snapshot").remove(0)
}

fn with_snapshot(host: &FakeHost, line: &str) -> FakeHost {
    copy(host).with_file("data/snapshot", &format!("{line}\n"))
}

#[test]
fn a_damaged_snapshot_falls_back_to_a_complete_log_and_says_so() {
    let host = untrimmed();
    let line = snapshot_line(&host);
    // A changed byte, hash left alone.
    let mut bad = with_snapshot(&host, &flip(&line));
    let out = stdout(&mut bad, SHOW_ALL);
    assert!(
        out.starts_with("opened: log (the snapshot is damaged:"),
        "{out}"
    );
    assert!(out.contains("the hash does not match"), "{out}");
    assert!(
        out.ends_with("k1=1 untrusted\nk2=2 untrusted\nk3=3 untrusted\nk4=4 untrusted\n"),
        "{out}"
    );
    // Changed and re-hashed: the seal no longer opens.
    let forged = rehash(&flip(body_of(&line)));
    let mut bad = with_snapshot(&host, &forged);
    let out = stdout(&mut bad, SHOW_ALL);
    assert!(
        out.starts_with("opened: log (the snapshot is damaged:"),
        "{out}"
    );
    assert!(out.contains("does not open"), "{out}");
    assert!(out.ends_with("k4=4 untrusted\n"), "{out}");
    // Not JSON at all, and empty.
    for junk in ["junk", ""] {
        let mut bad = with_snapshot(&host, junk);
        let out = stdout(&mut bad, SHOW_ALL);
        assert!(
            out.starts_with("opened: log (the snapshot is damaged:"),
            "{junk}: {out}"
        );
    }
    // A writer that falls back writes a new, good snapshot on request.
    let mut bad = with_snapshot(&host, &flip(&line));
    assert_eq!(stdout(&mut bad, &writing(SNAPSHOT)), "done\n");
    assert_eq!(stdout(&mut bad, SHOW_ALL), ALL_FOUR);
}

#[test]
fn a_damaged_snapshot_with_a_trimmed_log_is_an_error_with_no_data() {
    let host = snapshotted();
    let line = snapshot_line(&host);
    for bad_line in [flip(&line), rehash(&flip(body_of(&line)))] {
        let mut bad = with_snapshot(&host, &bad_line);
        let out = stdout(&mut bad, SHOW_ALL);
        assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
        assert!(out.contains("snapshot"), "{out}");
        assert!(!out.contains("k1="), "no partial data: {out}");
        // A writer refuses it too, and leaves the files alone.
        let before = (
            bad.file("data/log").map(str::to_string),
            bad.file("data/snapshot").map(str::to_string),
        );
        let out = stdout(&mut bad, &writing(&put("k5", 5)));
        assert!(out.starts_with("store: corrupt log:"), "{out}");
        assert_eq!(
            before,
            (
                bad.file("data/log").map(str::to_string),
                bad.file("data/snapshot").map(str::to_string)
            )
        );
    }
}

#[test]
fn a_missing_snapshot_with_a_trimmed_log_is_an_error_not_an_empty_store() {
    let host = snapshotted();
    let mut bare = world().with_file("data/log", host.file("data/log").expect("log"));
    let out = stdout(&mut bare, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log: line 1:"), "{out}");
    assert!(out.contains("snapshot is missing"), "{out}");
    assert!(!out.contains("k4="), "{out}");
}

#[test]
fn a_snapshot_that_the_log_does_not_reach_is_an_error_not_a_fallback() {
    // The log's tail cut off at a line break: the snapshot shows records
    // the log no longer has, which a log alone could not.
    let host = untrimmed();
    let log = log(&host);
    let cut = format!("{}\n", log[..2].join("\n"));
    let mut cut = copy(&host).with_file("data/log", &cut);
    let out = stdout(&mut cut, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
    assert!(out.contains("snapshot"), "{out}");
    assert!(!out.contains("k1="), "{out}");
    // An empty log likewise.
    let mut empty = copy(&host).with_file("data/log", "");
    let out = stdout(&mut empty, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
}

#[test]
fn a_snapshot_that_does_not_match_the_log_is_an_error() {
    // The record the snapshot ends at was changed in the log, with its hash
    // made to match: the snapshot's link to it fails.
    let host = untrimmed();
    let mut log = log(&host);
    log[2] = rehash(&flip(body_of(&log[2])));
    let mut bad = copy(&host).with_file("data/log", &format!("{}\n", log.join("\n")));
    let out = stdout(&mut bad, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
    assert!(out.contains("snapshot"), "{out}");
}

#[test]
fn an_older_snapshot_put_back_is_an_error_when_the_log_no_longer_has_its_end() {
    // Snapshot at 3, trim, put k4, snapshot at 4, trim: the log starts at
    // 4. The first snapshot, put back, ends before the log's start.
    let mut host = snapshotted();
    let old = snapshot_line(&host);
    assert_eq!(stdout(&mut host, &writing(SNAPSHOT)), "done\n");
    assert_eq!(log(&host).len(), 1);
    let mut replaced = with_snapshot(&host, &old);
    let out = stdout(&mut replaced, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
    assert!(!out.contains("k1="), "{out}");
}

#[test]
fn the_wrong_key_shows_nothing_through_a_snapshot() {
    let mut host = snapshotted();
    let wrong = SHOW_ALL.replace(
        "store.read(read, key, codec)",
        "store.read(read, Secret.new(\"nope\".to_bytes()), codec)",
    );
    let out = stdout(&mut host, &wrong);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
    assert!(!out.contains("k1="), "{out}");
}

#[test]
fn a_records_seal_cannot_stand_in_for_the_snapshot() {
    // The sealed body of a log record put where the snapshot goes does not
    // open as a snapshot: the snapshot has its own keys.
    let host = snapshotted();
    let record = log(&host)[0].clone();
    let mut bad = with_snapshot(&host, &record);
    let out = stdout(&mut bad, SHOW_ALL);
    assert!(out.starts_with("store: corrupt log:"), "{out}");
}

// ---- crash -----------------------------------------------------------------------------

/// What the store holds after the first `n` of the crash program's puts.
fn state_after(n: usize) -> String {
    let states = [
        "k1=-\nk2=-\nk3=-\nk4=-\n",
        "k1=1\nk2=-\nk3=-\nk4=-\n",
        "k1=1\nk2=2\nk3=-\nk4=-\n",
        "k1=1\nk2=2\nk3=3\nk4=-\n",
        "k1=1\nk2=2\nk3=3\nk4=4\n",
    ];
    states[n].to_string()
}

/// Four puts with two snapshots between them, each put acknowledged by a
/// printed line once `put` returned.
fn crash_program() -> String {
    let ops = r#"        let _ = st.put("k1", Item(name: "a", qty: 1))
        let _ = io.println("ack 1")
        let _ = st.put("k2", Item(name: "b", qty: 2))
        let _ = io.println("ack 2")
        let _ = st.snapshot()
        let _ = io.println("snapshot")
        let _ = st.put("k3", Item(name: "c", qty: 3))
        let _ = io.println("ack 3")
        let _ = st.snapshot()
        let _ = st.put("k4", Item(name: "d", qty: 4))
        let _ = io.println("ack 4")"#;
    program(&writing(ops))
}

fn acked(printed: &[String]) -> usize {
    printed.iter().filter(|l| l.starts_with("ack ")).count()
}

/// Recover from `fs`: open for writing, then read. What the read printed,
/// without the `opened` line, and that line.
fn recovered(fs: FakeHost) -> (String, String) {
    let mut host = fs;
    let open = run(&program(&writing("")), &mut host, &["prog.osk".to_string()]);
    assert_eq!(open.stdout, "done\n", "recovery open: {open:#?}");
    let shown = run(&program(SHOW_ALL), &mut host, &["prog.osk".to_string()]);
    assert_eq!(shown.exit_code, 0, "{shown:#?}");
    let (opened, rest) = shown.stdout.split_once('\n').expect("opened line");
    // The text before the colon, and a store never shows its labels here.
    let rest = rest
        .lines()
        .map(|l| l.trim_end_matches(" untrusted"))
        .map(|l| format!("{l}\n"))
        .collect();
    (rest, opened.to_string())
}

#[test]
fn a_crash_at_every_write_boundary_of_a_snapshot_recovers_to_a_prefix_of_the_acknowledged_writes() {
    let mut host = CrashHost::new(world());
    let src = crash_program();
    let out = run(&src, &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.stdout.lines().count(), 6, "{out:#?}");
    let recording = host.into_recording();
    let mut crashes = 0;
    let mut from_snapshot = 0;
    let mut from_log = 0;
    let mut trimmed = 0;
    recording.for_each_crash(Tears::Half, |crash| {
        crashes += 1;
        let n = acked(&crash.printed);
        let (state, opened) = recovered(copy(&crash.fs));
        let allowed: Vec<String> = (n..=4).map(state_after).collect();
        assert!(
            allowed.contains(&state),
            "after {}: acknowledged {n}, recovered\n{state}",
            crash.describe()
        );
        // A crash never leaves a bad snapshot: a rename waits for the sync
        // of its data, so the fallback never has to run.
        assert!(
            !opened.contains("damaged"),
            "after {}: {opened}",
            crash.describe()
        );
        if opened == "opened: snapshot" {
            from_snapshot += 1;
        } else {
            from_log += 1;
        }
        // The log's front is dropped only once a snapshot is there to take
        // over: a log that starts after record 1 has its snapshot.
        let log = crash.fs.file("data/log").unwrap_or("");
        let first_line = log.split_once('\n').map(|(line, _)| line);
        if first_line.is_some_and(|line| !line.contains("\"seq\":1,")) {
            trimmed += 1;
            assert!(
                crash.fs.file("data/snapshot").is_some(),
                "after {}: the log's front is gone and no snapshot is",
                crash.describe()
            );
        }
    });
    assert!(crashes > 40, "every step has crashes: {crashes}");
    assert!(
        from_snapshot > 0 && from_log > 0,
        "{from_snapshot} {from_log}"
    );
    assert!(trimmed > 0, "some crash falls after the log was trimmed");
}

#[test]
fn a_crash_while_recovering_after_a_snapshot_still_recovers_to_the_prefix() {
    let mut host = CrashHost::new(world());
    let out = run(&crash_program(), &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.exit_code, 0, "{out:#?}");
    let recording = host.into_recording();
    let mut second_crashes = 0;
    recording.for_each_crash(Tears::None, |crash| {
        // The crashes after the first snapshot's first step are the ones
        // whose recovery has a snapshot to read or to replace.
        if crash.fs.file("data/snapshot").is_none() && crash.fs.file("data/snapshot.tmp").is_none()
        {
            return;
        }
        let n = acked(&crash.printed);
        // Recover on a crash host that also takes a snapshot, and crash
        // that recovery everywhere.
        let mut again = CrashHost::new(copy(&crash.fs));
        let out = run(
            &program(&writing(SNAPSHOT)),
            &mut again,
            &["prog.osk".to_string()],
        );
        assert_eq!(out.stdout, "done\n", "{out:#?}");
        again
            .into_recording()
            .for_each_crash(Tears::None, |second| {
                second_crashes += 1;
                let (state, opened) = recovered(copy(&second.fs));
                let allowed: Vec<String> = (n..=4).map(state_after).collect();
                assert!(
                    allowed.contains(&state) && !opened.contains("damaged"),
                    "after {} then {}: acknowledged {n}, recovered\n{opened}\n{state}",
                    crash.describe(),
                    second.describe()
                );
            });
    });
    assert!(second_crashes > 20, "{second_crashes}");
}

#[test]
fn renaming_a_snapshot_before_syncing_its_data_would_be_caught_by_the_same_test() {
    // The same walk over a program that skips the sync of the temporary
    // file (the broken model `StoreLogRenameFirst`): some crash leaves a
    // snapshot name with torn data.
    let src = program(
        r#"    let tmp = Path.new("snapshot.tmp")
    dir.write(tmp, "0123456789")?
    dir.rename(tmp, Path.new("snapshot"))?
    dir.sync()?"#,
    );
    let mut host = CrashHost::new(world());
    let out = run(&src, &mut host, &["prog.osk".to_string()]);
    assert_eq!(out.exit_code, 0, "{out:#?}");
    let mut torn = 0;
    host.into_recording().for_each_crash(Tears::Every, |crash| {
        if crash
            .fs
            .file("data/snapshot")
            .is_some_and(|t| t != "0123456789")
        {
            torn += 1;
        }
    });
    assert!(torn > 0);
}
