// SPDX-License-Identifier: Apache-2.0
//! The filesystem that crashes (#847, spec #842): `CrashHost` records a
//! program's run and `CrashRecording::for_each_crash` hands recovery the
//! filesystem as a crash after each step may have left it, under the crash
//! rules of `formal/tla/store-log.md`.
//!
//! The programs are Oska programs run on the interpreter. They mirror the
//! models: `StoreLog` syncs before it acknowledges, `StoreLogAckEarly`
//! does not, `StoreLogRenameFirst` renames before it syncs the data.

use oska_run::{explore, run, CrashHost, CrashRecording, FakeHost, Tears};

/// Run `body` as the body of `main`, which has `io`, `fs` and `dir`, a
/// `DirWrite` on `/data`, on a crash host over `world`.
fn record(world: FakeHost, body: &str) -> CrashRecording {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n\
         \x20   let dir = fs.write_only().restrict_to(\"/data\")\n\
         {body}\n\
         \x20   Ok(())\n\
         }}\n"
    );
    let mut host = CrashHost::new(world);
    let outcome = run(&src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    host.into_recording()
}

fn world() -> FakeHost {
    FakeHost::new().with_dir("data")
}

/// The crashes after which `acked` had been printed and `lost` holds.
fn violations(
    recording: &CrashRecording,
    tears: Tears,
    acked: &str,
    lost: impl Fn(&FakeHost) -> bool,
) -> Vec<String> {
    let mut found = Vec::new();
    recording.for_each_crash(tears, |crash| {
        if crash.printed.iter().any(|l| l == acked) && lost(&crash.fs) {
            found.push(crash.describe());
        }
    });
    found
}

const SYNCED_ACK: &str = "    let log = dir.restrict_to_file(Path.new(\"log\"))\n\
    \x20   log.append(\"one\")?\n\
    \x20   log.sync()?\n\
    \x20   io.println(\"acked\")?\n";

const EARLY_ACK: &str = "    let log = dir.restrict_to_file(Path.new(\"log\"))\n\
    \x20   log.append(\"one\")?\n\
    \x20   io.println(\"acked\")?\n\
    \x20   log.sync()?\n";

fn log_lost(fs: &FakeHost) -> bool {
    fs.file("data/log") != Some("one")
}

#[test]
fn a_write_acknowledged_after_its_sync_survives_every_crash() {
    let recording = record(world(), SYNCED_ACK);
    let mut seen_ack = false;
    let crashes = recording.for_each_crash(Tears::Every, |crash| {
        seen_ack |= crash.printed.iter().any(|l| l == "acked");
    });
    assert!(crashes > 4, "every step has crashes: {crashes}");
    assert!(seen_ack, "some crash comes after the acknowledgement");
    let lost = violations(&recording, Tears::Every, "acked", log_lost);
    assert!(lost.is_empty(), "an acknowledged write was lost: {lost:#?}");
}

#[test]
fn a_write_acknowledged_before_its_sync_is_lost_at_some_crash() {
    let recording = record(world(), EARLY_ACK);
    let lost = violations(&recording, Tears::Every, "acked", log_lost);
    assert!(
        !lost.is_empty(),
        "the broken order must lose an acknowledged write at a crash point"
    );
    // The loss is at the print, before the sync: not after the sync.
    assert!(lost.iter().all(|d| !d.contains("sync file")), "{lost:#?}");
}

#[test]
fn a_rename_before_the_directory_sync_may_revert_and_after_it_may_not() {
    let steps = "    let tmp = dir.restrict_to_file(Path.new(\"snap.tmp\"))\n\
        \x20   tmp.write(\"snapshot\")?\n\
        \x20   tmp.sync()?\n\
        \x20   dir.rename(Path.new(\"snap.tmp\"), Path.new(\"snap\"))?\n\
        \x20   io.println(\"renamed\")?\n\
        \x20   dir.sync()?\n\
        \x20   io.println(\"durable\")?\n";
    let recording = record(world(), steps);
    let reverted = |fs: &FakeHost| fs.file("data/snap").is_none();
    let before = violations(&recording, Tears::Every, "renamed", reverted);
    assert!(!before.is_empty(), "an unsynced rename can revert");
    // When it reverts, the data it moved is under the old name, whole.
    recording.for_each_crash(Tears::Every, |crash| {
        if crash.printed.iter().any(|l| l == "renamed") && reverted(&crash.fs) {
            assert_eq!(crash.fs.file("data/snap.tmp"), Some("snapshot"));
        }
    });
    let after = violations(&recording, Tears::Every, "durable", reverted);
    assert!(after.is_empty(), "a synced rename stays: {after:#?}");
}

#[test]
fn a_rename_may_also_survive_before_the_directory_sync() {
    let steps = "    let tmp = dir.restrict_to_file(Path.new(\"snap.tmp\"))\n\
        \x20   tmp.write(\"snapshot\")?\n\
        \x20   tmp.sync()?\n\
        \x20   dir.rename(Path.new(\"snap.tmp\"), Path.new(\"snap\"))?\n";
    let recording = record(world(), steps);
    let (mut renamed, mut reverted) = (false, false);
    recording.for_each_crash(Tears::None, |crash| {
        if crash.after == recording.events().len() {
            renamed |= crash.fs.file("data/snap") == Some("snapshot");
            reverted |= crash.fs.file("data/snap.tmp") == Some("snapshot");
        }
    });
    assert!(renamed && reverted, "both outcomes: {renamed} {reverted}");
}

#[test]
fn a_rename_before_the_data_sync_can_leave_a_torn_snapshot() {
    // StoreLogRenameFirst: rename and directory sync, but no file sync.
    let steps = "    let tmp = dir.restrict_to_file(Path.new(\"snap.tmp\"))\n\
        \x20   tmp.write(\"snapshot\")?\n\
        \x20   dir.rename(Path.new(\"snap.tmp\"), Path.new(\"snap\"))?\n\
        \x20   dir.sync()?\n\
        \x20   io.println(\"durable\")?\n";
    let recording = record(world(), steps);
    let torn = |fs: &FakeHost| fs.file("data/snap").is_some_and(|t| t != "snapshot");
    let found = violations(&recording, Tears::Half, "durable", torn);
    assert!(
        !found.is_empty(),
        "the snapshot name is durable, its data not"
    );
    // With the file sync first, no crash after the directory sync tears it.
    let fixed = "    let tmp = dir.restrict_to_file(Path.new(\"snap.tmp\"))\n\
        \x20   tmp.write(\"snapshot\")?\n\
        \x20   tmp.sync()?\n\
        \x20   dir.rename(Path.new(\"snap.tmp\"), Path.new(\"snap\"))?\n\
        \x20   dir.sync()?\n\
        \x20   io.println(\"durable\")?\n";
    let recording = record(world(), fixed);
    let found = violations(&recording, Tears::Every, "durable", |fs| {
        fs.file("data/snap") != Some("snapshot")
    });
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_remove_is_durable_only_after_the_directory_sync() {
    let world = world().with_file("data/old", "x");
    let steps = "    dir.remove_file(Path.new(\"old\"))?\n\
        \x20   io.println(\"removed\")?\n\
        \x20   dir.sync()?\n\
        \x20   io.println(\"durable\")?\n";
    let recording = record(world, steps);
    let back = |fs: &FakeHost| fs.file("data/old") == Some("x");
    assert!(!violations(&recording, Tears::None, "removed", back).is_empty());
    assert!(violations(&recording, Tears::None, "durable", back).is_empty());
}

#[test]
fn a_torn_last_append_leaves_a_prefix_and_the_synced_part_stays() {
    let world = world().with_file("data/log", "base|");
    let steps = "    let log = dir.restrict_to_file(Path.new(\"log\"))\n\
        \x20   log.append(\"first|\")?\n\
        \x20   log.sync()?\n\
        \x20   log.append(\"abcd\")?\n";
    let recording = record(world, steps);
    let mut texts = std::collections::BTreeSet::new();
    recording.for_each_crash(Tears::Every, |crash| {
        texts.insert(
            crash
                .fs
                .file("data/log")
                .expect("the log is there")
                .to_string(),
        );
        let text = crash.fs.file("data/log").expect("the log is there");
        if crash.after >= 3 {
            assert!(text.starts_with("base|first|"), "synced data stays: {text}");
        }
    });
    for torn in [
        "base|first|",
        "base|first|a",
        "base|first|ab",
        "base|first|abc",
        "base|first|abcd",
    ] {
        assert!(texts.contains(torn), "{torn:?} missing from {texts:?}");
    }
    // Without tears a write is whole or absent.
    let mut whole = std::collections::BTreeSet::new();
    recording.for_each_crash(Tears::None, |crash| {
        whole.insert(
            crash
                .fs
                .file("data/log")
                .expect("the log is there")
                .to_string(),
        );
    });
    assert!(!whole.contains("base|first|ab"), "{whole:?}");
    assert!(whole.contains("base|first|abcd") && whole.contains("base|first|"));
}

#[test]
fn unsynced_data_is_dropped_and_a_synced_write_is_never_torn() {
    let steps = "    let log = dir.restrict_to_file(Path.new(\"log\"))\n\
        \x20   log.write(\"hello\")?\n\
        \x20   log.sync()?\n";
    let recording = record(world(), steps);
    recording.for_each_crash(Tears::Every, |crash| {
        let text = crash.fs.file("data/log");
        // Step 1 is the write (the file is created by it), 2 its sync.
        if crash.after >= 2 {
            assert_eq!(text, Some("hello"), "{}", crash.describe());
        } else if crash.after == 1 {
            assert!(text.is_some_and(|t| "hello".starts_with(t)), "{text:?}");
        }
    });
}

#[test]
fn explore_runs_the_program_once_and_calls_recovery_at_every_crash() {
    let mut calls = 0;
    let total = explore(
        world(),
        Tears::None,
        |host| {
            let src = "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {\n\
                       \x20   let log = fs.write_only().restrict_to_file(\"/data/log\")\n\
                       \x20   log.append(\"a\")?\n\
                       \x20   log.sync()?\n\
                       \x20   Ok(())\n}\n";
            let out = run(src, host, &["prog.osk".to_string()]);
            assert_eq!(out.exit_code, 0, "{out:#?}");
        },
        |_| calls += 1,
    );
    assert_eq!(calls, total);
    assert!(total >= 3);
}

#[test]
fn a_name_that_a_crash_reverts_is_a_file_still_and_never_a_directory() {
    // A rename that reverts leaves the old name; the new name must not
    // appear as a directory, or a program cannot write there again.
    let steps = "    let tmp = dir.restrict_to_file(Path.new(\"snap.tmp\"))\n\
        \x20   tmp.write(\"snapshot\")?\n\
        \x20   tmp.sync()?\n\
        \x20   dir.rename(Path.new(\"snap.tmp\"), Path.new(\"snap\"))?\n";
    let recording = record(world(), steps);
    recording.for_each_crash(Tears::Every, |crash| {
        let dirs = crash.fs.world().dirs;
        assert!(
            !dirs.contains(std::path::Path::new("/data/snap"))
                && !dirs.contains(std::path::Path::new("/data/snap.tmp")),
            "{}: {dirs:?}",
            crash.describe()
        );
    });
}
