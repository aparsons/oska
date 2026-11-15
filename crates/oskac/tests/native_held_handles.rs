//! The handles a native program's file capabilities hold (spec #153,
//! user story 13): on aarch64 and x86_64 Linux alike, every one is
//! close-on-exec, a directory, and opened without following a symlink
//! beneath a root. Natively only: the flags are read from `/proc` while
//! the program waits on stdin.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::procfs::held_while_waiting;
use differential::{build_native, Case};

/// `openat`'s flags as the host's kernel shows them in
/// `/proc/<pid>/fdinfo`, copied from the UAPI headers rather than from
/// std.rt's table: `O_CLOEXEC` from asm-generic/fcntl.h on both;
/// x86_64's `O_DIRECTORY` and `O_NOFOLLOW` from asm-generic/fcntl.h too,
/// which arch/x86/include/uapi/asm/fcntl.h takes as it is; arm64's from
/// arch/arm64/include/uapi/asm/fcntl.h, which overrides them. (Each one's
/// `O_NOFOLLOW` is the other's `O_LARGEFILE`, which a 64-bit kernel sets
/// on every open, so a value copied from the wrong one would pass
/// unnoticed on one of them.)
const O_CLOEXEC: u32 = 0o2000000;
#[cfg(target_arch = "x86_64")]
const O_DIRECTORY: u32 = 0o200000;
#[cfg(target_arch = "x86_64")]
const O_NOFOLLOW: u32 = 0o400000;
#[cfg(target_arch = "aarch64")]
const O_DIRECTORY: u32 = 0o40000;
#[cfg(target_arch = "aarch64")]
const O_NOFOLLOW: u32 = 0o100000;

/// A program that makes a capability of each kind, says `ready`, waits
/// for a line on stdin, and then uses each one.
const HOLDER: &str = r#"fn main(io: Stdout, fs: Fs, stdin: Stdin) -> Result<(), IoError> {
    let read = fs.read_only()
    let root = fs.restrict_to("root")
    let sub = root.restrict_to(Path.new("sub"))
    let nested = root.read_only().restrict_to_file(Path.new("sub/nested.txt"))
    let same = root.read_only().restrict_to_file(Path.new("inside.txt"))
    let secret = read.restrict_to_file("outside/secret.txt")
    io.println("ready")?
    let _ = stdin.read_line()
    io.println(listed(root.read_only().list_dir(Path.new("."))))?
    io.println(listed(sub.read_only().list_dir(Path.new("."))))?
    io.println(shown(nested.read_to_string()))?
    io.println(shown(same.read_to_string()))?
    io.println(shown(secret.read_to_string()))?
    Ok(())
}

fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn listed(result: Result<List<Untrusted<String>>, IoError>) -> Untrusted<String> {
    match result {
        Ok(names) => ",".join(names)
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

#[test]
fn every_handle_a_capability_holds_is_close_on_exec_and_opened_without_following() {
    let case = Case::file("held_handles", HOLDER)
        .with_file("root/inside.txt", "inside text")
        .with_file("root/sub/nested.txt", "nested text")
        .with_file("outside/secret.txt", "outside text");
    let (exe, dir) = build_native(&case);
    let held = held_while_waiting(&exe, &dir);
    assert!(held.status.success(), "{held:?}");
    assert_eq!(
        held.stdout,
        "inside.txt,sub\nnested.txt\nnested text\ninside text\noutside text\n"
    );
    // One handle per capability: `same` shares the root's directory.
    let names: Vec<&str> = held.handles.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["outside", "root", "root", "root/sub", "root/sub"],
        "{held:?}"
    );
    for (name, flags) in &held.handles {
        assert_eq!(flags & O_CLOEXEC, O_CLOEXEC, "`{name}`: {flags:o}");
        assert_eq!(flags & O_DIRECTORY, O_DIRECTORY, "`{name}`: {flags:o}");
        // A root named by trusted text follows symlinks up to its last
        // component; everything beneath one was opened without following.
        let followed = *name == "outside";
        assert_eq!(flags & O_NOFOLLOW == 0, followed, "`{name}`: {flags:o}");
    }
}
