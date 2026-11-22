// SPDX-License-Identifier: Apache-2.0
//! Workspaces: a fresh directory outside the repo that holds a task's
//! package, so nothing run in it can read the hidden checks or the other
//! solution. It is removed when dropped.

use crate::task::CHECKS_MODULE;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Where the hidden checks module goes in a package, relative to it.
pub fn checks_file() -> String {
    format!("src/{CHECKS_MODULE}.osk")
}

pub struct Workspace {
    dir: PathBuf,
    /// Leave the directory when dropped: it is a run's, not a scratch one.
    keep: bool,
}

impl Workspace {
    /// A workspace in `dir`, which is created, and is left in place when
    /// the workspace is dropped.
    pub fn kept(dir: &Path) -> Result<Workspace, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        Ok(Workspace {
            dir: dir.to_path_buf(),
            keep: true,
        })
    }

    /// A new, empty directory in the system's temporary directory.
    pub fn new() -> Result<Workspace, String> {
        let base = std::env::temp_dir();
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("oska-eval-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Workspace { dir, keep: false }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("cannot create {}: {e}", dir.display())),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Copies every regular file beneath `from` into the workspace, at the
    /// same relative path, replacing any file already there. Nothing else is
    /// copied or followed: the paths of what was left out (a link, a pipe, a
    /// device, or a file that turned into one while it was copied) are
    /// returned, sorted.
    pub fn copy_in(&self, from: &Path) -> Result<Vec<String>, String> {
        let tree = crate::task::tree(from);
        let mut refused = tree.others;
        for dir in &tree.dirs {
            let target = self.dir.join(dir);
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
        }
        for file in tree.files {
            if !self.copy_file(from, &file, &file)? {
                refused.push(file);
            }
        }
        refused.sort();
        Ok(refused)
    }

    /// Makes `file` in the workspace what it is beneath `from`: a copy of
    /// it, or nothing when `from` has no such regular file. Whatever is in
    /// the way, a directory at its path or a file at a parent's, goes.
    pub fn restore(&self, from: &Path, file: &str) -> Result<(), String> {
        let mut parent = self.dir.clone();
        let parts: Vec<&str> = file.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            parent.push(part);
            if std::fs::symlink_metadata(&parent).is_ok_and(|m| !m.is_dir()) {
                self.clear(&parent)?;
            }
        }
        self.clear(&self.dir.join(file))?;
        let source = from.join(file);
        if std::fs::symlink_metadata(&source).is_ok_and(|m| m.is_file())
            && !self.copy_file(from, file, file)?
        {
            return Err(format!("{} is not a regular file", source.display()));
        }
        Ok(())
    }

    /// Adds `checks` as the reserved hidden-checks module, or removes it
    /// (whatever is at its path) with `None`.
    pub fn set_checks(&self, checks: Option<&Path>) -> Result<(), String> {
        let module = checks_file();
        self.remove(&module)?;
        let Some(checks) = checks else {
            return Ok(());
        };
        let copied = match (checks.parent(), checks.file_name()) {
            (Some(dir), Some(name)) => self.copy_file(dir, &name.to_string_lossy(), &module)?,
            _ => false,
        };
        if copied {
            Ok(())
        } else {
            Err(format!("{} is not a regular file", checks.display()))
        }
    }

    /// Writes `text` to the file `to`, relative to the workspace.
    pub fn write(&self, to: &str, text: &str) -> Result<(), String> {
        let target = self.dir.join(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, text).map_err(|e| format!("cannot write {}: {e}", target.display()))
    }

    /// Removes whatever is at `file`, relative to the workspace.
    pub fn remove(&self, file: &str) -> Result<(), String> {
        self.clear(&self.dir.join(file))
    }

    /// Removes whatever is at `path`, a directory with all it holds; a link
    /// itself, never what it names.
    fn clear(&self, path: &Path) -> Result<(), String> {
        let removed = match std::fs::symlink_metadata(path) {
            Err(_) => return Ok(()),
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
        };
        removed.map_err(|e| format!("cannot remove {}: {e}", path.display()))
    }

    /// Copies `file` beneath `root` to `to`, relative to the workspace, if
    /// it is a regular file, and says whether it was. Nothing is followed or
    /// waited on, even when what is there changes while it is copied: a
    /// link or a pipe put in the file's place, or a link put in place of a
    /// directory on the way to it.
    fn copy_file(&self, root: &Path, file: &str, to: &str) -> Result<bool, String> {
        let from = root.join(file);
        let Some(mut source) = open_regular(&from) else {
            return Ok(false);
        };
        // A directory on the way swapped for a link while the file was
        // opened could have led outside `root`. Each must still be a
        // directory, and the path must still name the file that was opened.
        if !directories_on_the_way(root, file) || !still_names(&from, &source) {
            return Ok(false);
        }
        let target = self.dir.join(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let mut copy = std::fs::File::create(&target)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        std::io::copy(&mut source, &mut copy)
            .map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
        Ok(true)
    }
}

/// `path` opened for reading, if it is a regular file: looked at without
/// following a link, opened without following one or waiting, and then
/// asked what it is.
fn open_regular(path: &Path) -> Option<std::fs::File> {
    let before = std::fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.is_file())?;
    let file = open_without_following(path).ok()?;
    let opened = file.metadata().ok()?;
    (opened.is_file() && same_file(&before, &opened)).then_some(file)
}

/// Whether `path`, looked at again without following a link, is `file`.
fn still_names(path: &Path, file: &std::fs::File) -> bool {
    match (std::fs::symlink_metadata(path), file.metadata()) {
        (Ok(now), Ok(opened)) => now.is_file() && same_file(&now, &opened),
        _ => false,
    }
}

/// Whether `root` and every directory between it and `file` is a directory,
/// not a link.
fn directories_on_the_way(root: &Path, file: &str) -> bool {
    let is_dir = |dir: &Path| std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir());
    let mut dir = root.to_path_buf();
    if !is_dir(&dir) {
        return false;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    parts.into_iter().all(|part| {
        dir.push(part);
        is_dir(&dir)
    })
}

/// Opens `path` for reading. A link at its end is not followed (the open
/// fails), and a pipe does not block waiting for a writer.
#[cfg(unix)]
fn open_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(open_flags::O_NOFOLLOW | open_flags::O_NONBLOCK)
        .open(path)
}

/// On Windows a link is a reparse point: opened as itself, it is never a
/// regular file, so it is refused. Windows has no pipes in the file system.
#[cfg(windows)]
fn open_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Anywhere else nothing is opened: the harness fails closed.
#[cfg(not(any(unix, windows)))]
fn open_without_following(_: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// `open(2)` flags std does not name, from each target's `<fcntl.h>`, so no
/// outside crate is needed. A Unix target not listed here does not build:
/// the harness fails closed rather than guess them.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
mod open_flags {
    pub const O_NONBLOCK: i32 = 0x0004;
    pub const O_NOFOLLOW: i32 = 0x0100;
}

/// Linux's own values on Arm and Power; the generic ones elsewhere.
#[cfg(all(
    any(target_os = "linux", target_os = "android"),
    any(
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    )
))]
mod open_flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o100_000;
}

#[cfg(all(
    any(target_os = "linux", target_os = "android"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "riscv32",
        target_arch = "riscv64",
        target_arch = "s390x",
        target_arch = "loongarch64"
    )
))]
mod open_flags {
    pub const O_NONBLOCK: i32 = 0o4000;
    pub const O_NOFOLLOW: i32 = 0o400_000;
}

/// Whether two descriptions are of one file: the one looked at without
/// following a link, and the one then opened.
#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Windows gives no stable file identity to compare; there the file opened
/// as itself, never through a link, stands alone.
#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    true
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;

    /// What an agent may leave in a workspace, and where the harness copies it.
    fn agent_and_copy() -> (Workspace, Workspace) {
        let agent = Workspace::new().expect("a directory");
        agent
            .write("src/main.osk", "fn main() {}\n")
            .expect("a file");
        (agent, Workspace::new().expect("a directory"))
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_left_out_and_reported_never_followed() {
        let (agent, copy) = agent_and_copy();
        let outside = Workspace::new().expect("a directory");
        outside.write("secret.osk", "outside\n").expect("a file");
        std::os::unix::fs::symlink(
            outside.path().join("secret.osk"),
            agent.path().join("src/a.osk"),
        )
        .expect("a link to a file");
        std::os::unix::fs::symlink(outside.path(), agent.path().join("src/dir"))
            .expect("a link to a directory");
        std::os::unix::fs::symlink("..", agent.path().join("src/up")).expect("a cycle");
        let refused = copy.copy_in(agent.path()).expect("the copy");
        assert_eq!(refused, ["src/a.osk", "src/dir", "src/up"]);
        assert_eq!(crate::task::files(copy.path()), ["src/main.osk"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_from_a_link_is_refused() {
        let (agent, copy) = agent_and_copy();
        std::os::unix::fs::symlink(
            agent.path().join("src/main.osk"),
            agent.path().join("l.osk"),
        )
        .expect("a link");
        assert!(!copy
            .copy_file(agent.path(), "l.osk", "l.osk")
            .expect("no error"));
        assert!(!copy.path().join("l.osk").exists());
    }

    /// The open itself, past the look before it: it must not follow a
    /// link, and must not wait on a pipe for a writer that never comes.
    #[cfg(unix)]
    #[test]
    fn the_open_neither_follows_a_link_nor_waits_on_a_pipe() {
        let (agent, _) = agent_and_copy();
        let link = agent.path().join("link.osk");
        std::os::unix::fs::symlink(agent.path().join("src/main.osk"), &link).expect("a link");
        assert!(
            super::open_without_following(&link).is_err(),
            "followed a link"
        );
        let pipe = agent.path().join("pipe.osk");
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .expect("mkfifo should run");
        assert!(made.success());
        let opened = super::open_without_following(&pipe).expect("opened without waiting");
        assert!(!opened.metadata().expect("its metadata").is_file());
        assert!(super::open_regular(&pipe).is_none());
    }

    /// A directory on the way to a file, now a link, leads nowhere.
    #[cfg(unix)]
    #[test]
    fn a_directory_on_the_way_that_is_a_link_is_refused() {
        let (agent, copy) = agent_and_copy();
        let outside = Workspace::new().expect("a directory");
        outside.write("secret.osk", "outside\n").expect("a file");
        std::os::unix::fs::symlink(outside.path(), agent.path().join("lib")).expect("a link");
        assert!(!copy
            .copy_file(agent.path(), "lib/secret.osk", "lib/secret.osk")
            .expect("no error"));
    }

    #[test]
    fn restoring_clears_what_is_in_the_way() {
        let (task, copy) = agent_and_copy();
        task.write("oska.toml", "the task's\n").expect("a file");
        // A directory where the file was, and a file where a parent was.
        copy.write("oska.toml/x", "agent's\n").expect("a file");
        copy.write("src", "agent's\n").expect("a file");
        copy.restore(task.path(), "oska.toml").expect("restored");
        copy.restore(task.path(), "src/main.osk").expect("restored");
        let read = |f: &str| std::fs::read_to_string(copy.path().join(f)).expect("a file");
        assert_eq!(read("oska.toml"), "the task's\n");
        assert_eq!(read("src/main.osk"), "fn main() {}\n");
        // A read-only path the task does not have is removed.
        copy.write("extra.toml", "agent's\n").expect("a file");
        copy.restore(task.path(), "extra.toml").expect("restored");
        assert!(!copy.path().join("extra.toml").exists());
    }
}
