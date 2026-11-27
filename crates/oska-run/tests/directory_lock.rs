// SPDX-License-Identifier: Apache-2.0
//! `DirWrite.with_lock`, the exclusive lock on a directory (ticket #846,
//! spec #842, ADR 0008): one writer holds a store at a time. The lock is
//! scoped, because Oska has no destructors: it is taken, a closure runs
//! with the same directory capability, and it is released when the
//! closure returns or panics. A second lock on the directory, from this
//! program or another process, fails at once with an `IoError` that
//! begins `L0001` and names the lock.
//!
//! The first half runs on [`oska_run::FakeHost`] through the differential
//! helpers, so every program also runs as a native executable and the
//! two must agree on output and on the filesystem afterwards (the lock
//! file included). The second half runs on [`oska_run::RealHost`] against
//! a real directory, on Unix and Windows, and holds the lock from a second
//! process.

// The tests take a lock from outside the program with `File::try_lock`
// (Rust 1.89), as another process would; the pinned toolchain has it.
#![allow(clippy::incompatible_msrv)]

mod common;

use common::{printed, printed_with, run_on, test_fake};
use oska_run::{FakeHost, TestStatus};

/// What a lock already held says, with the root's path left out.
const HELD: &str = "REFUSED: L0001: the directory lock `.oska-lock` is already held";

#[test]
fn a_lock_runs_the_closure_with_the_directory_and_gives_its_result() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let locked = store.with_lock(|dir: DirWrite| dir.write(Path.new("a.txt"), "inside"))
    match locked {
        Ok(inner) => io.println(done(inner))?
        Err(e) => io.println(refused(e))?
    }
    io.println(shown(read.restrict_to("store").read_to_string(Path.new("a.txt"))))?"#,
    );
    assert_eq!(out, "done\ninside\n");
}

#[test]
fn a_second_lock_on_the_same_directory_fails_and_names_the_lock() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        match dir.with_lock(|again: DirWrite| 1) {
            Ok(_) => "second lock taken"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n"));
}

#[test]
fn a_second_capability_for_the_same_directory_conflicts_too() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let first = write.restrict_to("store")
    let second = write.restrict_to("store/../store")
    let outer = first.with_lock(|dir: DirWrite| {
        match second.with_lock(|again: DirWrite| 1) {
            Ok(_) => "second lock taken"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n"));
}

#[test]
fn the_lock_is_released_when_the_closure_returns() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    for round in 0..3 {
        match store.with_lock(|dir: DirWrite| round) {
            Ok(n) => io.println("round {n}")?
            Err(e) => io.println(refused(e))?
        }
    }"#,
    );
    assert_eq!(out, "round 0\nround 1\nround 2\n");
}

#[test]
fn the_failed_attempt_does_not_release_the_holders_lock() {
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        let first = match dir.with_lock(|a: DirWrite| 1) {
            Ok(_) => "taken"
            Err(e) => refused(e)
        }
        let second = match dir.with_lock(|b: DirWrite| 2) {
            Ok(_) => "taken"
            Err(e) => refused(e)
        }
        [first, second]
    })
    match outer {
        Ok(lines) => {
            for line in lines {
                io.println(line)?
            }
        }
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, format!("{HELD}\n{HELD}\n"));
}

#[test]
fn locks_on_different_directories_do_not_conflict() {
    let host = FakeHost::new().with_dir("one").with_dir("two");
    let out = printed(
        host,
        r#"    let one = write.restrict_to("one")
    let two = write.restrict_to("two")
    let outer = one.with_lock(|dir: DirWrite| {
        match two.with_lock(|other: DirWrite| 7) {
            Ok(n) => "both {n}"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, "both 7\n");
}

#[test]
fn a_subdirectory_has_its_own_lock() {
    let host = FakeHost::new().with_dir("store/sub");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let outer = store.with_lock(|dir: DirWrite| {
        match dir.restrict_to(Path.new("sub")).with_lock(|inner: DirWrite| 3) {
            Ok(n) => "inner {n}"
            Err(e) => refused(e)
        }
    })
    match outer {
        Ok(text) => io.println(text)?
        Err(e) => io.println(refused(e))?
    }"#,
    );
    assert_eq!(out, "inner 3\n");
}

#[test]
fn the_lock_file_is_made_once_and_left_in_place() {
    let mut host = FakeHost::new().with_dir("store");
    let out = printed_with(
        &mut host,
        &[],
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| 0)
    let _ = store.with_lock(|dir: DirWrite| 0)
    io.println(listed(read.restrict_to("store").list_dir(Path.new("."))))?
    io.println(shown(read.restrict_to("store").read_to_string(Path.new(".oska-lock"))))?"#,
    );
    assert_eq!(out, ".oska-lock\n\n");
    assert_eq!(host.file("store/.oska-lock"), Some(""));
}

#[test]
fn a_lock_on_a_missing_directory_is_an_error_and_runs_nothing() {
    let host = FakeHost::new();
    let out = printed(
        host,
        r#"    let store = write.restrict_to("absent")
    match store.with_lock(|dir: DirWrite| 1) {
        Ok(_) => io.println("ran")?
        Err(e) => io.println("refused: {e.message}")?
    }"#,
    );
    assert_eq!(out, "refused\n");
}

#[test]
fn a_lock_needs_no_more_authority_than_the_directory_capability() {
    // A closure may capture other capabilities, as a `cap fn`.
    let host = FakeHost::new().with_dir("store");
    let out = printed(
        host,
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| io.println("holding"))
    io.println("released")?"#,
    );
    assert_eq!(out, "holding\nreleased\n");
}

#[test]
fn a_panic_in_the_closure_ends_the_program_with_its_panic_code() {
    let host = FakeHost::new().with_dir("store");
    let src = common::fs_program(
        r#"    let store = write.restrict_to("store")
    let _ = store.with_lock(|dir: DirWrite| [1, 2].at(5))"#,
    );
    let outcome = run_on(host, &src, &[]);
    assert_eq!(outcome.panic.map(|p| p.code), Some("P0002"), "{src}");
    assert_eq!(outcome.exit_code, 101);
}

/// A `test` block runs on the test host, which fakes the filesystem, and
/// natively on `std.rt`'s own fake: both must lock as `FakeHost` does.
#[test]
fn a_test_block_locks_a_directory_once_at_a_time() {
    let src = "test \"lock\" {\n    \
               let dir = t.fs.restrict_to(\"/\").write_only()\n    \
               t.assert(dir.create_dir(Path.new(\"store\")).is_ok())\n    \
               let store = dir.restrict_to(Path.new(\"store\"))\n    \
               match store.with_lock(|d: DirWrite| d.with_lock(|e: DirWrite| 1)) {\n        \
               Ok(inner) => match inner {\n            \
               Ok(_) => t.fail(\"second lock taken\")\n            \
               Err(e) => t.assert_eq(e.message, \"L0001: the directory lock `.oska-lock` is already held\")\n        }\n        \
               Err(_) => t.fail(\"first lock refused\")\n    }\n    \
               t.assert(store.with_lock(|d: DirWrite| 2).is_ok())\n    \
               let read = t.fs.restrict_to(\"/\").read_only()\n    \
               match read.list_dir(Path.new(\"store\")) {\n        \
               Ok(names) => t.assert_eq(\",\".join(names), \".oska-lock\")\n        \
               Err(_) => t.fail(\"list failed\")\n    }\n}\n";
    let report = test_fake(src);
    assert!(
        !report.tests.is_empty() && report.tests.iter().all(|t| t.status == TestStatus::Passed),
        "{src}\n{report:#?}"
    );
}

// ---- A real directory ----------------------------------------------------------------

#[cfg(any(unix, windows))]
mod real {
    use super::common::{self, TempTree};

    /// The message a lock held gives, as `TempTree::saw` shows it.
    const HELD: &str = "REFUSED: L0001: the directory lock `.oska-lock` is already held";

    #[test]
    fn a_real_directory_is_locked_once_at_a_time() {
        let tree = TempTree::new("lock-once");
        let seen = tree.saw(
            r#"    let locked = write.with_lock(|dir: DirWrite| {
        let again = match dir.with_lock(|inner: DirWrite| 1) {
            Ok(_) => "taken"
            Err(e) => refused(e)
        }
        let made = done(dir.write(Path.new("a.txt"), "inside"))
        "{again} / {made}"
    })
    match locked {
        Ok(text) => { say(text) }
        Err(e) => { say(refused(e)) }
    }
    match write.with_lock(|dir: DirWrite| 5) {
        Ok(n) => { say("relocked {n}") }
        Err(e) => { say(refused(e)) }
    }
    say(listed(read.list_dir(Path.new("."))))"#,
        );
        assert_eq!(seen, format!("{HELD} / done\nrelocked 5\n.oska-lock,a.txt"));
        assert_eq!(tree.file("root/a.txt"), "inside");
        assert_eq!(tree.file("root/.oska-lock"), "");
    }

    /// Hold the lock on `root` from outside the Oska program, as the
    /// operating system sees another holder: dropping the file releases
    /// it.
    #[cfg(unix)]
    fn hold(tree: &TempTree) -> std::fs::File {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(tree.path("root/.oska-lock"))
            .expect("open the lock file");
        file.try_lock().expect("take the lock");
        file
    }

    #[cfg(windows)]
    fn hold(tree: &TempTree) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(tree.path("root/.oska-lock"))
            .expect("open the lock file, sharing nothing")
    }

    #[test]
    fn a_lock_another_holder_has_fails_without_running_the_closure() {
        let tree = TempTree::new("lock-held");
        let holder = hold(&tree);
        let seen = tree.saw(
            r#"    match write.with_lock(|dir: DirWrite| dir.write(Path.new("ran.txt"), "x")) {
        Ok(_) => { say("ran") }
        Err(e) => { say(refused(e)) }
    }"#,
        );
        assert_eq!(seen, HELD);
        assert!(!tree.path("root/ran.txt").exists());
        drop(holder);
        let seen = tree.saw(
            r#"    match write.with_lock(|dir: DirWrite| 1) {
        Ok(n) => { say("ran {n}") }
        Err(e) => { say(refused(e)) }
    }"#,
        );
        assert_eq!(seen, "ran 1");
    }

    #[test]
    fn a_panic_in_the_closure_releases_the_lock() {
        let tree = TempTree::new("lock-panic");
        let root = common::literal(tree.path("root"));
        let program = |body: &str| {
            format!(
                "fn main(fs: Fs) -> Result<(), IoError> {{\n    \
                 let write = fs.write_only().restrict_to(\"{root}\")\n{body}\n    Ok(())\n}}\n"
            )
        };
        let mut host = oska_run::RealHost::new();
        let args = [common::PROGRAM_PATH.to_string()];
        let panicked = oska_run::run(
            &program("    let _ = write.with_lock(|dir: DirWrite| [1].at(3))"),
            &mut host,
            &args,
        );
        assert_eq!(panicked.panic.map(|p| p.code), Some("P0002"));
        // The same host, the same directory: the panic released the lock.
        let again = oska_run::run(
            &program(
                "    let _ = write.with_lock(|dir: DirWrite| dir.write(Path.new(\"b.txt\"), \"after\"))",
            ),
            &mut host,
            &args,
        );
        assert!(
            again.panic.is_none() && again.diagnostics.is_empty(),
            "{again:#?}"
        );
        assert_eq!(tree.file("root/b.txt"), "after");
    }
}

// ---- Attacks on the lock file --------------------------------------------------------

#[cfg(unix)]
mod attacks {
    use super::common::{self, TempTree};

    fn lock_attempt(tree: &TempTree) -> String {
        tree.saw(
            r#"    match write.with_lock(|dir: DirWrite| 1) {
        Ok(n) => { say("ran {n}") }
        Err(e) => { say(refused(e)) }
    }"#,
        )
    }

    #[test]
    fn a_symlink_at_the_lock_file_is_refused_and_not_followed() {
        let tree = TempTree::new("lock-symlink");
        tree.write("outside/target.txt", "outside");
        common::symlink(
            &tree.path("outside/target.txt"),
            &tree.path("root/.oska-lock"),
        );
        let seen = lock_attempt(&tree);
        assert_eq!(
            seen,
            "REFUSED: `.oska-lock` is a symlink, and confined access never follows one"
        );
        assert_eq!(tree.file("outside/target.txt"), "outside");
    }

    #[test]
    fn a_hard_link_at_the_lock_file_is_refused_and_leaves_the_target_alone() {
        let tree = TempTree::new("lock-hardlink");
        tree.write("outside/target.txt", "outside");
        std::fs::hard_link(
            tree.path("outside/target.txt"),
            tree.path("root/.oska-lock"),
        )
        .expect("create a hard link");
        let seen = lock_attempt(&tree);
        assert_eq!(
            seen,
            "REFUSED: `.oska-lock` has more than one hard link, and confined access never opens one"
        );
        assert_eq!(tree.file("outside/target.txt"), "outside");
    }

    #[test]
    fn a_directory_at_the_lock_file_is_refused() {
        let tree = TempTree::new("lock-dir");
        std::fs::create_dir_all(tree.path("root/.oska-lock")).expect("create a directory");
        let seen = lock_attempt(&tree);
        assert!(seen.starts_with("REFUSED: "), "{seen}");
        assert!(!seen.contains("ran"), "{seen}");
    }

    #[test]
    fn a_lock_file_that_has_content_is_kept_not_emptied() {
        let tree = TempTree::new("lock-content");
        tree.write("root/.oska-lock", "keep me");
        let seen = lock_attempt(&tree);
        assert_eq!(seen, "ran 1");
        assert_eq!(tree.file("root/.oska-lock"), "keep me");
    }

    #[test]
    fn a_capability_cannot_lock_beneath_a_symlinked_directory() {
        let tree = TempTree::new("lock-linked-dir");
        tree.write("outside/file.txt", "outside");
        common::symlink(&tree.path("outside"), &tree.path("root/link"));
        let seen = tree.saw(
            r#"    match write.restrict_to(Path.new("link")).with_lock(|dir: DirWrite| 1) {
        Ok(n) => { say("ran {n}") }
        Err(e) => { say(refused(e)) }
    }"#,
        );
        assert!(seen.starts_with("REFUSED: "), "{seen}");
        assert!(!tree.path("outside/.oska-lock").exists());
    }
}

// ---- Another process ------------------------------------------------------------------

/// The lock is the operating system's: a native executable holds it from
/// another process, the interpreter cannot take it meanwhile, and killing
/// that process (a crash) frees it.
#[cfg(all(
    unix,
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod another_process {
    use super::common::TempTree;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    const HOLDER: &str = r#"fn main(io: Stdout, input: Stdin, fs: Fs, args: Args) -> Result<(), IoError> {
    let dir = fs.write_only().restrict_to(args.get(1).unwrap_or(".").assume_trusted())
    let held = dir.with_lock(|d: DirWrite| {
        let _ = io.println("locked")
        input.read_line()
    })
    match held {
        Ok(_) => io.println("released")
        Err(e) => io.println("refused: {e.message}")
    }
}
"#;

    fn executable(tree: &TempTree) -> std::path::PathBuf {
        let report = oska_check::check_module(HOLDER);
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let program = report.program.expect("the holder checks");
        let bytes = oska_native::build(
            &program,
            oska_native::Target::host().expect("a native host"),
            &oska_native::Unplaced,
        )
        .expect("the holder builds");
        let exe = tree.path("holder");
        std::fs::write(&exe, bytes).expect("write the executable");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
        exe
    }

    /// Start the holder on `root`, once it says it holds the lock.
    fn start(tree: &TempTree) -> (std::process::Child, BufReader<std::process::ChildStdout>) {
        let mut child = Command::new(executable(tree))
            .arg(tree.path("root"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the holder");
        let mut out = BufReader::new(child.stdout.take().expect("a stdout pipe"));
        let mut line = String::new();
        out.read_line(&mut line).expect("read a line");
        assert_eq!(line.trim(), "locked");
        (child, out)
    }

    fn attempt(tree: &TempTree) -> String {
        tree.saw(
            r#"    match write.with_lock(|dir: DirWrite| 1) {
        Ok(n) => { say("ran {n}") }
        Err(e) => { say(refused(e)) }
    }"#,
        )
    }

    const HELD: &str = "REFUSED: L0001: the directory lock `.oska-lock` is already held";

    #[test]
    fn a_lock_a_native_process_holds_keeps_the_interpreter_out_until_it_ends() {
        let tree = TempTree::new("lock-process");
        let (mut child, mut out) = start(&tree);
        assert_eq!(attempt(&tree), HELD);
        // End its stdin: it returns from the closure and releases.
        drop(child.stdin.take());
        let mut line = String::new();
        out.read_line(&mut line).expect("read a line");
        assert_eq!(line.trim(), "released");
        child.wait().expect("wait for the holder");
        assert_eq!(attempt(&tree), "ran 1");
    }

    #[test]
    fn a_lock_is_freed_when_the_process_holding_it_is_killed() {
        let tree = TempTree::new("lock-crash");
        let (mut child, _out) = start(&tree);
        assert_eq!(attempt(&tree), HELD);
        child.kill().expect("kill the holder");
        child.wait().expect("wait for the holder");
        assert_eq!(attempt(&tree), "ran 1");
    }

    #[test]
    fn a_native_process_cannot_take_a_lock_the_interpreter_holds() {
        let tree = TempTree::new("lock-reverse");
        let exe = executable(&tree);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(tree.path("root/.oska-lock"))
            .expect("open the lock file");
        file.try_lock().expect("take the lock");
        let output = Command::new(exe)
            .arg(tree.path("root"))
            .stdin(Stdio::null())
            .output()
            .expect("run the holder");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "refused: L0001: the directory lock `.oska-lock` is already held"
        );
    }

    #[test]
    fn the_native_and_the_interpreter_refuse_a_planted_lock_file_in_the_same_words() {
        let tree = TempTree::new("lock-planted");
        let exe = executable(&tree);
        tree.write("outside/target.txt", "outside");
        super::common::symlink(
            &tree.path("outside/target.txt"),
            &tree.path("root/.oska-lock"),
        );
        let interpreted = attempt(&tree);
        let output = Command::new(&exe)
            .arg(tree.path("root"))
            .stdin(Stdio::null())
            .output()
            .expect("run the holder");
        let native = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            interpreted.replace("REFUSED: ", "refused: "),
            native,
            "a symlink"
        );
        std::fs::remove_file(tree.path("root/.oska-lock")).expect("remove the link");
        std::fs::hard_link(
            tree.path("outside/target.txt"),
            tree.path("root/.oska-lock"),
        )
        .expect("create a hard link");
        let interpreted = attempt(&tree);
        let output = Command::new(&exe)
            .arg(tree.path("root"))
            .stdin(Stdio::null())
            .output()
            .expect("run the holder");
        let native = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            interpreted.replace("REFUSED: ", "refused: "),
            native,
            "a hard link"
        );
        assert_eq!(tree.file("outside/target.txt"), "outside");
    }
}
