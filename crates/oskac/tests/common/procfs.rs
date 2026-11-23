// SPDX-License-Identifier: Apache-2.0
//! A running native program, seen through Linux's `/proc`: the
//! descriptors it holds and the flags they were opened with, and the
//! processes another one started.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

/// What a program held open while it waited ([`held_while_waiting`]),
/// and how it went on.
#[derive(Debug)]
pub struct Held {
    /// Each descriptor open on a path under the directory it ran in: the
    /// path, relative to that directory, and the `open` flags the kernel
    /// shows for it in `/proc/<pid>/fdinfo`, sorted.
    pub handles: Vec<(String, u32)>,
    /// What it printed after `ready`.
    pub stdout: String,
    /// How it ended.
    pub status: ExitStatus,
}

/// Run the executable `exe` in `dir`, with no environment, until it
/// prints the line `ready` and waits on stdin; read each descriptor it
/// holds on a path under `dir`, then give it a line on stdin and let it
/// finish.
pub fn held_while_waiting(exe: &Path, dir: &Path) -> Held {
    let dir = std::fs::canonicalize(dir).expect("the program's directory");
    let mut child = Command::new(exe)
        .current_dir(&dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("a line");
    assert_eq!(line, "ready\n");
    let proc = Path::new("/proc").join(child.id().to_string());
    let mut handles = Vec::new();
    for entry in std::fs::read_dir(proc.join("fd")).expect("its descriptors") {
        let fd = entry.expect("a descriptor").file_name();
        let Ok(target) = std::fs::read_link(proc.join("fd").join(&fd)) else {
            continue;
        };
        let Ok(relative) = target.strip_prefix(&dir) else {
            continue;
        };
        let info = std::fs::read_to_string(proc.join("fdinfo").join(&fd)).expect("its fdinfo");
        let flags = info
            .lines()
            .find_map(|l| l.strip_prefix("flags:"))
            .map(|f| u32::from_str_radix(f.trim(), 8).expect("octal flags"))
            .expect("a flags line");
        handles.push((relative.to_string_lossy().into_owned(), flags));
    }
    handles.sort();
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"go\n").expect("write stdin");
    drop(stdin);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).expect("the rest");
    Held {
        handles,
        stdout: rest,
        status: child.wait().expect("it should end"),
    }
}

/// The processes whose parent is `pid`, from `/proc/<n>/stat`.
pub fn children(pid: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let child: u32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            // `pid (comm) state ppid ...`, where `comm` may hold spaces.
            let ppid = stat.rsplit_once(')')?.1.split_whitespace().nth(1)?;
            (ppid.parse::<u32>().ok()? == pid).then_some(child)
        })
        .collect()
}
