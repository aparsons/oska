// SPDX-License-Identifier: Apache-2.0
//! Native confined file capabilities (ticket #137), continued from
//! `native_files.rs`: hard links, the whole filesystem, files swapped while
//! the program runs, adversarial cases, closures and memory limits. Every
//! case starts from the same tree as `native_files.rs` (see `tree` in
//! `common/native_files.rs`), runs on both backends through the
//! differential harness, and runs where `native_files.rs` does.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

#[path = "common/native_files.rs"]
mod support;

use differential::{assert_same, Case};
use std::path::Path;
use support::*;

// ---- Hard links (`hard_links.rs`) --------------------------------------------------

#[test]
fn a_file_with_more_than_one_hard_link_is_never_read_or_emptied() {
    let case = case(
        "hard_links",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("leak.txt"))))?
    io.println(done(root.write_only().write(Path.new("leak.txt"), "overwritten")))?
    io.println(shown(read.restrict_to_file("root/leak.txt").read_to_string()))?
    io.println(done(write.restrict_to_file("root/leak.txt").write("overwritten")))?
    io.println(shown(root.read_only().restrict_to_file(Path.new("leak.txt")).read_to_string()))?
    io.println(done(root.write_only().restrict_to_file(Path.new("leak.txt")).write("overwritten")))?
    io.println(shown(root.read_only().read_to_string(Path.new("inside.txt"))))?
    io.println(done(root.write_only().write(Path.new("inside.txt"), "rewritten")))?
    io.println(done(root.write_only().write(Path.new("new.txt"), "new text")))?
    io.println(shown(root.read_only().read_to_string(Path.new("new.txt"))))?"#,
    )
    .with_setup(|dir| {
        std::fs::hard_link(dir.join("outside/secret.txt"), dir.join("root/leak.txt"))
            .expect("create a hard link");
    });
    let outcome = assert_same(&case);
    let refusal =
        "REFUSED: `leak.txt` has more than one hard link, and confined access never opens one\n";
    assert_eq!(
        outcome.stdout,
        format!("{}inside text\ndone\ndone\nnew text\n", refusal.repeat(6))
    );
    assert_eq!(
        outcome.tree["outside/secret.txt"].as_deref(),
        Some("outside text")
    );
    assert_eq!(
        outcome.tree["root/inside.txt"].as_deref(),
        Some("rewritten")
    );
}

// ---- The whole filesystem (`whole_filesystem.rs`) ------------------------------

#[cfg(unix)]
#[test]
fn the_whole_filesystem_is_still_confined_by_the_path_rules() {
    let case = case(
        "whole_paths",
        r#"    let everything = fs.restrict_to("/")
    io.println(shown(everything.read_only().read_to_string(Path.new("../up.txt"))))?
    io.println(shown(everything.read_only().read_to_string(Path.new("/etc/hostname"))))?
    io.println(shown(everything.read_only().read_to_string(Path.new("dev"))))?
    io.println(shown(fs.restrict_to("/dev/..").read_only().read_to_string(Path.new("."))))?"#,
    );
    assert_eq!(
        printed(&case),
        "REFUSED: `../up.txt` leads outside the capability's root\n\
         REFUSED: `/etc/hostname` is absolute; a directory capability takes paths relative to its root\n\
         REFUSED: `dev` is not a regular file\n\
         REFUSED: `.` names the directory capability's root itself, not a file\n"
    );
}

#[cfg(unix)]
#[test]
fn restrict_to_slash_reads_and_writes_a_real_file() {
    // Natively only: the path from `/` names the case's own directory,
    // which differs between the two runs.
    let case = case(
        "whole_real",
        r#"    let at = args.get(1).unwrap_or("")
    let everything = fs.restrict_to("/")
    let text = everything.read_only().read_to_string(Path.from("{at}/root/inside.txt"))?
    everything.write_only().write(Path.from("{at}/root/out.txt"), text)?
    io.println(listed(everything.read_only().list_dir(Path.from("{at}/root"))))?"#,
    );
    let (exe, dir) = differential::build_native(&case);
    let relative = dir
        .strip_prefix("/")
        .expect("an absolute path")
        .display()
        .to_string();
    let output = std::process::Command::new(exe)
        .arg(&relative)
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "inside.txt,out.txt,sub\n",
        "{output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("root/out.txt")).expect("written"),
        "inside text"
    );
}

// ---- Swapped while the program runs (`file_capabilities.rs`) -------------------

/// Run `case`, whose program prints `ready` and then reads a line, on
/// both backends: once it is ready, `swap` changes its tree, and then it
/// goes on. Requires the same output from both, and returns it with the
/// native run's directory.
fn swapping(case: impl Fn(&str) -> Case, swap: fn(&Path)) -> (String, std::path::PathBuf) {
    let (exe, native_dir) = differential::build_native(&case("native"));
    let native = interact(std::process::Command::new(exe), &native_dir, swap);
    // A second tree for the interpreter, set up the same way.
    let (_, interpreted_dir) = differential::build_native(&case("interpreted"));
    let mut oskac = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"));
    oskac.args(["run", "--interpret", "main.osk"]);
    let interpreted = interact(oskac, &interpreted_dir, swap);
    assert_eq!(interpreted, native, "the backends differ");
    (native, native_dir)
}

/// Run `command` in `dir` until it prints `ready`, call `swap`, then let
/// it finish: everything it printed after `ready`.
fn interact(mut command: std::process::Command, dir: &Path, swap: fn(&Path)) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let mut child = command
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("a line");
    assert_eq!(line, "ready\n");
    swap(dir);
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"go\n").expect("write stdin");
    drop(stdin);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).expect("the rest");
    let status = child.wait().expect("the program should finish");
    assert!(status.success(), "{rest}");
    rest
}

/// A program that makes its capabilities with `before`, prints `ready`,
/// waits for a line on stdin, then runs `after`.
fn swap_case(name: &str, before: &str, after: &str) -> Case {
    let src = program(&format!(
        "{before}\n    io.println(\"ready\")?\n    let _ = stdin.read_line()\n{after}"
    ))
    .replace("args: Args)", "args: Args, stdin: Stdin)");
    tree(Case::file(&format!("files_{name}"), &src))
}

#[test]
fn a_file_swapped_for_a_symlink_after_the_capability_was_made_is_refused() {
    let case = |side: &str| {
        swap_case(
            &format!("swap_file_{side}"),
            r#"    let inside = read.restrict_to_file("root/inside.txt")
    let out = write.restrict_to_file("root/inside.txt")
    let dir = fs.restrict_to("root")"#,
            r#"    io.println(shown(inside.read_to_string()))?
    io.println(done(out.write("pwned")))?
    io.println(shown(dir.read_only().read_to_string(Path.new("inside.txt"))))?
    io.println(done(dir.write_only().write(Path.new("inside.txt"), "pwned")))?
    io.println(shown(dir.read_only().read_to_string(Path.new("sub/nested.txt"))))?
    io.println(listed(dir.read_only().list_dir(Path.new("sub"))))?
    io.println(done(dir.write_only().create_dir(Path.new("sub/new"))))?"#,
        )
    };
    let (seen, dir) = swapping(case, |dir| {
        std::fs::remove_file(dir.join("root/inside.txt")).expect("remove the file");
        link(dir, "root/inside.txt", "../outside/secret.txt");
        std::fs::rename(dir.join("root/sub"), dir.join("root/sub-moved")).expect("move sub");
        link(dir, "root/sub", "../outside");
    });
    assert_all_refused(&seen, 7, "symlink");
    assert_eq!(
        std::fs::read_to_string(dir.join("outside/secret.txt")).expect("read"),
        "outside text"
    );
    assert!(!dir.join("outside/new").exists());
}

/// Not on Windows, which refuses to move a directory while a handle is
/// open on anything in it, as `sub` is here.
#[cfg(unix)]
#[test]
fn a_capability_whose_root_was_swapped_for_a_symlink_keeps_its_own_directory() {
    let case = |side: &str| {
        swap_case(
            &format!("swap_parent_{side}"),
            r#"    let inside = read.restrict_to_file("root/inside.txt")
    let out = write.restrict_to_file("root/inside.txt")
    let dir = fs.restrict_to("root")
    let sub = dir.restrict_to(Path.new("sub"))"#,
            r#"    io.println(shown(inside.read_to_string()))?
    io.println(done(out.write("rewritten")))?
    io.println(shown(inside.read_to_string()))?
    io.println(shown(dir.read_only().read_to_string(Path.new("sub/nested.txt"))))?
    io.println(listed(dir.read_only().list_dir(Path.new("."))))?
    io.println(done(sub.write_only().write(Path.new("new.txt"), "new")))?"#,
        )
        .with_file("outside/inside.txt", "decoy text")
    };
    let (seen, dir) = swapping(case, |dir| {
        std::fs::rename(dir.join("root"), dir.join("root-moved")).expect("move the root");
        link(dir, "root", "outside");
    });
    assert_eq!(
        seen,
        "inside text\ndone\nrewritten\nnested text\ninside.txt,sub\ndone\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("outside/inside.txt")).expect("read"),
        "decoy text"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("root-moved/sub/new.txt")).expect("read"),
        "new"
    );
}

// ---- Adversarial cases -----------------------------------------------------------

#[test]
fn a_nul_byte_anywhere_in_a_path_is_refused() {
    let case = case(
        "nul",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("in\0side.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("s\0ub/nested.txt"))))?
    io.println(done(root.write_only().write(Path.new("new\0.txt"), "x")))?
    io.println(done(root.write_only().create_dir(Path.new("new\0dir"))))?
    io.println(done(root.write_only().create_dir(Path.new("s\0ub/new"))))?
    io.println(listed(root.read_only().list_dir(Path.new("s\0ub"))))?
    io.println(shown(root.read_only().restrict_to(Path.new("s\0ub")).read_to_string(Path.new("nested.txt"))))?
    io.println(shown(read.restrict_to("ro\0ot").read_to_string(Path.new("inside.txt"))))?
    io.println(shown(read.restrict_to("root/s\0ub").read_to_string(Path.new("nested.txt"))))?
    io.println(shown(read.restrict_to_file("root/in\0side.txt").read_to_string()))?
    io.println(shown(read.restrict_to_file("ro\0ot/inside.txt").read_to_string()))?"#,
    );
    let outcome = assert_same(&case);
    if cfg!(windows) {
        // Windows refuses a name holding NUL as no single file name, and
        // a root's path holding one in Rust's words.
        assert_all_refused(&outcome.stdout, 11, "");
        let lines: Vec<&str> = outcome.stdout.lines().collect();
        assert_eq!(
            lines[0],
            "REFUSED: `in\0side.txt` is not a single file name"
        );
        assert_eq!(
            lines[10],
            "REFUSED: cannot restrict to `ro\0ot/inside.txt`: `ro\0ot`: \
             strings passed to WinAPI cannot contain NULs"
        );
        return;
    }
    assert_all_refused(
        &outcome.stdout,
        11,
        "a path holds a NUL byte, which no file name may",
    );
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[3],
        "REFUSED: a path holds a NUL byte, which no file name may"
    );
    assert_eq!(
        lines[0],
        "REFUSED: `in\0side.txt`: a path holds a NUL byte, which no file name may"
    );
}

#[test]
fn text_that_is_not_utf8_is_refused_and_large_files_round_trip() {
    let case = case(
        "contents",
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("latin1.txt"))))?
    io.println(shown(root.read_only().read_to_string(Path.new("empty.txt"))))?
    let big = root.read_only().read_to_string(Path.new("big.txt"))?
    io.println(done(root.write_only().write(Path.new("copy.txt"), big)))?
    let again = root.read_only().read_to_string(Path.new("copy.txt"))?
    let size = again.byte_len()
    if again == big {
        io.println("{size} same")?
    }
    var i = 0
    while i < 300 {
        root.write_only().write(Path.new("many/f{i}"), "")?
        i = i + 1
    }
    let names = root.read_only().list_dir(Path.new("many"))?
    let count = names.len()
    let first = names.at(0)
    let second = names.at(1)
    let last = names.at(299)
    io.println("{count} {first} {second} {last}")?"#,
    )
    .with_file("root/empty.txt", "")
    .with_file("root/many/f0", "")
    .with_setup(|dir| {
        std::fs::write(dir.join("root/latin1.txt"), b"caf\xe9").expect("write");
        let big: String = (0..100_000).map(|i| format!("line {i} é\n")).collect();
        std::fs::write(dir.join("root/big.txt"), big).expect("write");
    });
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        "REFUSED: stream did not contain valid UTF-8\n\n\
         done\n1388890 same\n300 f0 f1 f99\n"
    );
}

/// Linux only: APFS refuses a name that is not UTF-8 (`EILSEQ`), so
/// macOS cannot make the tree.
#[cfg(target_os = "linux")]
#[test]
fn an_entry_name_that_is_not_utf8_fails_the_listing_as_rust_shows_it() {
    let case = case(
        "bad_name",
        r#"    io.println(listed(read.restrict_to("root").list_dir(Path.new("sub"))))?
    io.println(listed(read.restrict_to("root/sub").list_dir(Path.new("."))))?"#,
    )
    .with_setup(|dir| {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"bad\xff\"\xe0\xa0A\xcc\x81'");
        std::fs::write(dir.join("root/sub").join(name), "x").expect("write");
    });
    // The message names the root's canonical path, which is each run's
    // own directory.
    let (interpreted, native) = differential::run(&case);
    assert_eq!(
        interpreted.stdout.replace("/interpreted/", "/native/"),
        native.stdout
    );
    let shown = "\"bad\\xFF\\\"\\xE0\\xA0A\\u{301}'\"";
    for line in native.stdout.lines() {
        assert!(
            line.starts_with(&format!("REFUSED: {shown} in `/"))
                && line.ends_with("/native/root/sub` is not valid UTF-8"),
            "{native:#?}"
        );
    }
}

#[test]
fn a_root_reached_through_chained_symlinks_and_dot_dot_is_its_real_directory() {
    // The same roots as the test below (absolute, relative, chained, with
    // `..`), to a directory with a UTF-8 name, so it runs on macOS too.
    let case = case(
        "chained_root",
        r#"    io.println(listed(read.restrict_to("link/sub/..").list_dir(Path.new("."))))?
    io.println(listed(read.restrict_to("chain/sub/..").list_dir(Path.new("."))))?
    io.println(listed(read.restrict_to("./up/real/sub/..").list_dir(Path.new("."))))?"#,
    )
    .with_setup(|dir| {
        std::fs::create_dir_all(dir.join("real/sub")).expect("mkdir");
        std::fs::write(dir.join("real/file.txt"), "x").expect("write");
        link(dir, "link", dir.join("real"));
        link(dir, "chain", "link");
        std::fs::create_dir(dir.join("up")).expect("mkdir");
        link(dir, "up/real", "../real");
    });
    // The absolute symlink differs between the two trees, so only what
    // the programs printed is compared.
    let (interpreted, native) = differential::run(&case);
    assert_eq!(
        interpreted.stdout, native.stdout,
        "{interpreted:#?}\n{native:#?}"
    );
    assert_eq!(native.stdout, "file.txt,sub\n".repeat(3), "{native:#?}");
}

/// Linux only: APFS refuses a name that is not UTF-8 (`EILSEQ`), so
/// macOS cannot make the tree.
#[cfg(target_os = "linux")]
#[test]
fn a_root_is_named_by_its_canonical_path_shown_lossily() {
    // The root is reached through symlinks (absolute, relative, chained,
    // with `..`) to a directory whose name is not UTF-8: the listing error
    // names it as Rust's `fs::canonicalize` resolves it and `display`
    // shows it, each ill-formed sequence one U+FFFD.
    let case = case(
        "canonical_root",
        r#"    io.println(listed(read.restrict_to("link/sub/..").list_dir(Path.new("."))))?
    io.println(listed(read.restrict_to("chain/sub/..").list_dir(Path.new("."))))?
    io.println(listed(read.restrict_to("./up/odd/sub/..").list_dir(Path.new("."))))?"#,
    )
    .with_setup(|dir| {
        use std::os::unix::ffi::OsStrExt;
        let odd = std::ffi::OsStr::from_bytes(b"odd\xff\xe0\xa0x\xf0\x9f");
        let bad = std::ffi::OsStr::from_bytes(b"bad\xfe");
        std::fs::create_dir_all(dir.join(odd).join("sub")).expect("mkdir");
        std::fs::write(dir.join(odd).join(bad), "x").expect("write");
        let target = dir.join(odd);
        std::os::unix::fs::symlink(&target, dir.join("link")).expect("symlink");
        std::os::unix::fs::symlink("link", dir.join("chain")).expect("symlink");
        std::fs::create_dir(dir.join("up")).expect("mkdir");
        std::os::unix::fs::symlink(
            std::path::Path::new("..").join(odd),
            dir.join("up").join("odd"),
        )
        .expect("symlink");
    });
    let (interpreted, native) = differential::run(&case);
    assert_eq!(
        interpreted.stdout.replace("/interpreted/", "/native/"),
        native.stdout,
        "{interpreted:#?}\n{native:#?}"
    );
    assert_eq!(native.stdout.lines().count(), 3, "{native:#?}");
    for line in native.stdout.lines() {
        assert!(
            line.starts_with("REFUSED: \"bad\\xFE\" in `/")
                && line.ends_with("/native/odd\u{fffd}\u{fffd}x\u{fffd}` is not valid UTF-8"),
            "{native:#?}"
        );
    }
}

/// The directory listing the process's own open descriptors.
#[cfg(unix)]
const FD_DIR: &str = if cfg!(target_os = "macos") {
    "/dev/fd"
} else {
    "/proc/self/fd"
};

#[cfg(unix)]
#[test]
fn no_handle_is_left_open_on_any_path() {
    // Natively only: the program counts its own open descriptors.
    let src = program(
        &r#"    let root = fs.restrict_to("root")
    io.println(listed(read.restrict_to("{FD_DIR}").list_dir(Path.new("."))))?
    var i = 0
    // Past the usual limit of 1024 open descriptors, so one leaked on
    // any path fails the run.
    while i < 1100 {
        let _ = root.read_only().read_to_string(Path.new("inside.txt"))
        let _ = root.read_only().read_to_string(Path.new("out/secret.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub/escape.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub/out/x"))
        let _ = root.read_only().read_to_string(Path.new("leak.txt"))
        let _ = root.read_only().read_to_string(Path.new("sub"))
        let _ = root.read_only().list_dir(Path.new("sub"))
        let _ = root.read_only().list_dir(Path.new("."))
        let _ = root.read_only().list_dir(Path.new("out"))
        let _ = root.write_only().write(Path.new("sub/new.txt"), "x")
        let _ = root.write_only().write(Path.new("leak.txt"), "x")
        let _ = root.write_only().write(Path.new("sub/escape.txt"), "x")
        let _ = root.write_only().create_dir(Path.new("sub/made"))
        let _ = root.write_only().create_dir(Path.new("sub/out/made"))
        let _ = root.write_only().create_dir(Path.new("sub/escape.txt"))
        let narrowed = root.restrict_to(Path.new("sub"))
        let _ = narrowed.restrict_to(Path.new("out"))
        let _ = narrowed.read_only().restrict_to_file(Path.new("nested.txt")).read_to_string()
        let _ = read.restrict_to_file("root/inside.txt").read_to_string()
        let _ = write.restrict_to_file("root/sub/new.txt").write("y")
        let reader = reader_of(read)
        let _ = reader("nested.txt")
        let _ = reader("out/x")
        let held = Held(dirs: [root.read_only(), read.restrict_to("root/sub")], file: read.restrict_to_file("root/inside.txt"))
        let _ = held.dirs.at(1).read_to_string(Path.new("nested.txt"))
        i = i + 1
    }
    io.println(listed(read.restrict_to("{FD_DIR}").list_dir(Path.new("."))))?"#
            .replace("{FD_DIR}", FD_DIR),
    );
    let src = format!("{src}{READER}\ncap struct Held {{ dirs: List<DirRead>, file: FileRead }}\n");
    let case = tree(Case::file("files_handles", &src)).with_setup(|dir| {
        link(dir, "root/out", "../outside");
        link(dir, "root/sub/out", "../../outside");
        link(dir, "root/sub/escape.txt", "../../outside/secret.txt");
        std::fs::hard_link(dir.join("outside/secret.txt"), dir.join("root/leak.txt"))
            .expect("create a hard link");
    });
    let outcome = differential::run_native(&case);
    // The same descriptors open after the loop as before it. Inherited
    // ones (a git hook passes some on, a CI runner others, numbered above
    // 100) are in both listings, which sort by bytes ("142" before "2").
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{outcome:#?}");
    assert_eq!(lines[0], lines[1], "{outcome:#?}");
    let open: Vec<&str> = lines[0].split(',').collect();
    for fd in ["0", "1", "2"] {
        assert!(open.contains(&fd), "{outcome:#?}");
    }
}

#[cfg(unix)]
#[test]
fn a_file_or_directory_without_permission_fails_with_the_systems_message() {
    let src = program(
        r#"    let root = fs.restrict_to("root")
    io.println(shown(root.read_only().read_to_string(Path.new("locked/x.txt"))))?
    io.println(listed(root.read_only().list_dir(Path.new("locked"))))?
    io.println(listed(read.restrict_to("root/locked").list_dir(Path.new("."))))?
    io.println(done(root.write_only().write(Path.new("locked/x.txt"), "x")))?
    io.println(shown(root.read_only().read_to_string(Path.new("secret.txt"))))?
    io.println(done(root.write_only().write(Path.new("secret.txt"), "x")))?"#,
    );
    let mut seen = Vec::new();
    for side in ["native", "interpreted"] {
        let case = tree(Case::file(&format!("files_permission_{side}"), &src));
        let (exe, dir) = differential::build_native(&case);
        lock(&dir, 0o000);
        let mut command = match side {
            "native" => std::process::Command::new(exe),
            _ => {
                let mut oskac = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"));
                oskac.args(["run", "--interpret", "main.osk"]);
                oskac
            }
        };
        let output = command
            .current_dir(&dir)
            .env_clear()
            .output()
            .expect("runs");
        // Unlocked again, so the tree can be read and removed.
        lock(&dir, 0o700);
        seen.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    assert_eq!(seen[0], seen[1], "the backends differ");
    if !seen[0].contains("permission denied") {
        // Running as root, which no permission stops: nothing to compare.
        return;
    }
    assert_eq!(
        seen[0],
        "REFUSED: `locked`: permission denied\n\
         REFUSED: `locked`: permission denied\n\
         REFUSED: cannot restrict to `root/locked`: `locked`: permission denied\n\
         REFUSED: `locked`: permission denied\n\
         REFUSED: `secret.txt`: permission denied\n\
         REFUSED: `secret.txt`: permission denied\n"
    );
}

#[cfg(unix)]
/// Set the mode of `root/locked` and `root/secret.txt` beneath `dir`,
/// making them first.
fn lock(dir: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let locked = dir.join("root/locked");
    let secret = dir.join("root/secret.txt");
    if !locked.exists() {
        std::fs::create_dir(&locked).expect("mkdir");
        std::fs::write(&secret, "s").expect("write");
    }
    for path in [locked, secret] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
}

// ---- Captured by closures ------------------------------------------------------

/// A function that makes a closure capturing a `DirRead`, which outlives
/// the binding it came from.
const READER: &str = r#"
fn reader_of(read: FsRead) -> cap fn(String) -> Result<Untrusted<String>, IoError> {
    let sub = read.restrict_to("root/sub")
    |name: String| sub.read_to_string(Path.new(name))
}
"#;

#[test]
fn a_closure_keeps_the_directory_capability_it_captured() {
    let src = program(
        r#"    let reader = reader_of(read)
    io.println(shown(reader("nested.txt")))?
    io.println(shown(reader("../inside.txt")))?
    io.println(shown(reader("out/secret.txt")))?
    let readers = [reader, reader_of(read), reader]
    for r in readers {
        io.println(shown(r("nested.txt")))?
    }
    io.println(listed(read.restrict_to("root/sub").list_dir(Path.new("."))))?"#,
    );
    let src = format!("{src}{READER}");
    let case = tree(Case::file("files_closures", &src))
        .with_setup(|dir| link(dir, "root/sub/out", "../../outside"));
    assert_eq!(
        printed(&case),
        worded(
            "nested text\n\
             REFUSED: `../inside.txt` leads outside the capability's root\n\
             REFUSED: `out` is a symlink or not a directory, and confined access never follows a symlink\n\
             nested text\nnested text\nnested text\n\
             nested.txt,out\n"
        )
    );
}

// ---- Running out of memory --------------------------------------------------------

/// Linux only: macOS overcommits memory without limit, as Linux does
/// under `vm.overcommit_memory = 1` (below), so a terabyte reserves and
/// the read goes on until the system kills the process.
#[cfg(target_os = "linux")]
#[test]
fn a_file_too_large_to_hold_is_an_io_error_not_the_end_of_the_process() {
    // A sparse file of 1 TiB: no memory holds it, so the interpreter's
    // `read_to_string` cannot reserve its buffer and gives an `IoError`.
    // Under `vm.overcommit_memory = 1` the reservation would succeed on
    // both backends and the read would go on for a terabyte of zeros.
    let policy = std::fs::read_to_string("/proc/sys/vm/overcommit_memory").unwrap_or_default();
    if policy.trim() == "1" {
        eprintln!("skipped: memory is overcommitted without limit here");
        return;
    }
    // Outside the trees the harness compares, which it reads whole.
    let case = case(
        "sparse",
        r#"    let big = read.restrict_to("../sparse")
    io.println(shown(big.read_to_string(Path.new("big"))))?
    io.println(shown(big.restrict_to_file(Path.new("big")).read_to_string()))?
    io.println(shown(read.restrict_to("root").read_to_string(Path.new("inside.txt"))))?"#,
    )
    .with_setup(|dir| {
        let sparse = dir.parent().expect("the case directory").join("sparse");
        std::fs::create_dir_all(&sparse).expect("create the sparse directory");
        let big = std::fs::File::create(sparse.join("big")).expect("create the sparse file");
        big.set_len(1 << 40).expect("size the sparse file");
    });
    assert_eq!(
        printed(&case),
        "REFUSED: out of memory\nREFUSED: out of memory\ninside text\n"
    );
}
