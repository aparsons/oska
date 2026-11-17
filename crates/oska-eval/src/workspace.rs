//! Workspaces: a fresh directory outside the repo that holds a task's
//! package, so nothing run in it can read the hidden checks or the other
//! solution. It is removed when dropped.

use crate::task::CHECKS_MODULE;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Workspace {
    dir: PathBuf,
}

impl Workspace {
    /// A new, empty directory in the system's temporary directory.
    pub fn new() -> Result<Workspace, String> {
        let base = std::env::temp_dir();
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("oska-eval-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Workspace { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("cannot create {}: {e}", dir.display())),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Copies every file beneath `from` into the workspace, at the same
    /// relative path, replacing any file already there.
    pub fn copy_in(&self, from: &Path) -> Result<(), String> {
        for file in crate::task::files(from) {
            self.copy_file(&from.join(&file), &file)?;
        }
        Ok(())
    }

    /// Adds `checks` as the reserved hidden-checks module, or removes it
    /// with `None`.
    pub fn set_checks(&self, checks: Option<&Path>) -> Result<(), String> {
        let module = format!("src/{CHECKS_MODULE}.osk");
        match checks {
            Some(checks) => self.copy_file(checks, &module),
            None => match std::fs::remove_file(self.dir.join(&module)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("cannot remove {module}: {e}"))
                }
                _ => Ok(()),
            },
        }
    }

    fn copy_file(&self, from: &Path, to: &str) -> Result<(), String> {
        let target = self.dir.join(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(from, &target)
            .map(|_| ())
            .map_err(|e| format!("cannot copy {}: {e}", from.display()))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
