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
        for file in tree.files {
            if !self.copy_file(&from.join(&file), &file)? {
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
            && !self.copy_file(&source, file)?
        {
            return Err(format!("{} is not a regular file", source.display()));
        }
        Ok(())
    }

    /// Adds `checks` as the reserved hidden-checks module, or removes it
    /// (whatever is at its path) with `None`.
    pub fn set_checks(&self, checks: Option<&Path>) -> Result<(), String> {
        let module = checks_file();
        self.clear(&self.dir.join(&module))?;
        match checks {
            Some(checks) if self.copy_file(checks, &module)? => Ok(()),
            Some(checks) => Err(format!("{} is not a regular file", checks.display())),
            None => Ok(()),
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

    /// Copies `from` to `to`, relative to the workspace, if `from` is a
    /// regular file, and says whether it was. A link is never followed,
    /// even one put in a file's place between looking and opening it.
    fn copy_file(&self, from: &Path, to: &str) -> Result<bool, String> {
        let cannot = |e: std::io::Error| format!("cannot copy {}: {e}", from.display());
        let Ok(before) = std::fs::symlink_metadata(from) else {
            return Ok(false);
        };
        if !before.is_file() {
            return Ok(false);
        }
        let mut source = std::fs::File::open(from).map_err(cannot)?;
        let opened = source.metadata().map_err(cannot)?;
        if !opened.is_file() || !same_file(&before, &opened) {
            return Ok(false);
        }
        let target = self.dir.join(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let mut copy = std::fs::File::create(&target)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        std::io::copy(&mut source, &mut copy).map_err(cannot)?;
        Ok(true)
    }
}

/// Whether two descriptions are of one file: the one looked at without
/// following a link, and the one then opened.
#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Windows gives no stable file identity to compare; there the check made
/// before opening stands alone.
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
            .copy_file(&agent.path().join("l.osk"), "l.osk")
            .expect("no error"));
        assert!(!copy.path().join("l.osk").exists());
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
