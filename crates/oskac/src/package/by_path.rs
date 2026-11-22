// SPDX-License-Identifier: Apache-2.0
//! Reading and replacing a program's files by path, where `oska-sys` has
//! no directory handle to open them beneath: every platform but Unix, so
//! Windows (ADR 0014). It uses only `std`.
//!
//! On Unix, `package::Files` opens each component beneath its parent's
//! handle with `O_NOFOLLOW`, so nothing is checked separately and there is
//! nothing to race. By path, the best `std` offers is to look: every
//! component from the root down is looked at, refused when it is a symlink
//! or, on Windows, any reparse point (a junction or a mount point), and
//! looked at again later. A file is read between two looks, and a new
//! file is written beside the old one, then everything is looked at again
//! just before the rename that replaces the old one; any difference, such
//! as a directory swapped for a junction or the file for a link, refuses.
//!
//! What it cannot do with `std` alone: tell two files apart by their file
//! ID (`MetadataExt::file_index` is unstable), refuse a file with another
//! hard link (`number_of_links` is unstable too), or close the moment
//! between the last look and the rename. A component is told apart by its
//! kind, its creation time and, for a file, its length and its last write.
//!
//! On Unix this module is compiled for its tests only, which run the same
//! checks against Unix symlinks; on Windows they run where a symlink can
//! be made.

use std::fs::Metadata;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// What one component of a path was when it was looked at: enough to see
/// that it was replaced between two looks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    dir: bool,
    created: Option<SystemTime>,
    /// For a file, its last write; for a directory, nothing: writing the
    /// new file beside the old one changes its directory's own time.
    modified: Option<SystemTime>,
    /// For a file, its length; 0 for a directory.
    len: u64,
}

impl Seen {
    fn of(meta: &Metadata) -> Seen {
        let dir = meta.is_dir();
        Seen {
            dir,
            created: meta.created().ok(),
            modified: if dir { None } else { meta.modified().ok() },
            len: if dir { 0 } else { meta.len() },
        }
    }
}

/// Whether `meta`, from `symlink_metadata`, is of a symlink or, on
/// Windows, of any reparse point: a junction or a mount point too. None
/// is ever followed.
fn is_link(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn link_refused(names: &[&str]) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{}` is a symlink or a reparse point, and confined access never follows one",
            names.join("/")
        ),
    )
}

fn changed(names: &[&str]) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{}`, or a directory on its path, changed while `oskac` used it",
            names.join("/")
        ),
    )
}

/// The root and each component `names` reach beneath it, as they are now.
/// The root is the caller's trusted choice and is followed, as Unix's
/// `Dir::open` follows it. Beneath it, no component is a symlink or a
/// reparse point, each but the last is a directory, and the last is a
/// regular file; or, when `missing` allows, not there, which is `None`.
fn look(root: &Path, names: &[&str], missing: bool) -> io::Result<Vec<Option<Seen>>> {
    let meta = std::fs::metadata(root)
        .map_err(|e| io::Error::new(e.kind(), format!("`{}`: {e}", root.display())))?;
    let mut seen = vec![Some(Seen::of(&meta))];
    let mut path = root.to_path_buf();
    for (at, name) in names.iter().enumerate() {
        let reached = &names[..=at];
        let mut components = Path::new(name).components();
        if !matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(_)), None)
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` is not one plain name", reached.join("/")),
            ));
        }
        path.push(name);
        let last = at + 1 == names.len();
        let meta = match std::fs::symlink_metadata(&path) {
            Err(e) if last && missing && e.kind() == io::ErrorKind::NotFound => {
                seen.push(None);
                continue;
            }
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("`{}`: {e}", reached.join("/")),
                ));
            }
            Ok(meta) => meta,
        };
        if is_link(&meta) {
            return Err(link_refused(reached));
        }
        let what = if last && !meta.is_file() {
            Some("a regular file")
        } else if !last && !meta.is_dir() {
            Some("a directory")
        } else {
            None
        };
        if let Some(what) = what {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` is not {what}", reached.join("/")),
            ));
        }
        seen.push(Some(Seen::of(&meta)));
    }
    Ok(seen)
}

/// A directory whose files are read and replaced by path, with every
/// component looked at before and after.
pub struct ByPath {
    root: PathBuf,
}

impl ByPath {
    /// The directory at `root`, a path the user chose.
    pub fn open(root: &Path) -> io::Result<ByPath> {
        look(root, &[], false)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` is not a directory", root.display()),
            ));
        }
        Ok(ByPath {
            root: root.to_path_buf(),
        })
    }

    fn path(&self, names: &[&str]) -> PathBuf {
        names.iter().fold(self.root.clone(), |p, n| p.join(n))
    }

    /// The bytes of the regular file `names` reach, read between two
    /// looks at every component that agree.
    pub fn read(&self, names: &[&str]) -> io::Result<Vec<u8>> {
        let before = look(&self.root, names, false)?;
        let bytes = std::fs::read(self.path(names))?;
        if look(&self.root, names, false)? != before {
            return Err(changed(names));
        }
        Ok(bytes)
    }

    /// Write `bytes` to a new file beside the one `names` reach, with its
    /// permissions; the file may be missing, and is then created by
    /// [`Staged::commit`]. Every component is looked at before and after.
    pub fn stage(&self, names: &[&str], bytes: &[u8]) -> io::Result<Staged> {
        let Some((last, parents)) = names.split_last() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no file name to write",
            ));
        };
        let seen = look(&self.root, names, true)?;
        let dir = self.path(parents);
        let temp = super::temp_name(last);
        let temp_path = dir.join(&temp);
        // A new file a crash left behind is replaced; anything else there,
        // a link included, is refused.
        match std::fs::symlink_metadata(&temp_path) {
            Ok(meta) if is_link(&meta) || !meta.is_file() => {
                return Err(link_refused(&[parents, &[temp.as_str()]].concat()));
            }
            Ok(_) => std::fs::remove_file(&temp_path)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let permissions = match seen.last() {
            Some(Some(_)) => Some(std::fs::symlink_metadata(dir.join(last))?.permissions()),
            _ => None,
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let mut staged = Staged {
            root: self.root.clone(),
            names: names.iter().map(|n| n.to_string()).collect(),
            dir,
            temp,
            seen,
            written: None,
            committed: false,
        };
        file.write_all(bytes)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        drop(file);
        let written = std::fs::symlink_metadata(&temp_path)?;
        if is_link(&written) || !written.is_file() {
            return Err(changed(names));
        }
        staged.written = Some(Seen::of(&written));
        staged.check()?;
        Ok(staged)
    }
}

/// A file's new bytes, written beside it, waiting to replace it.
pub struct Staged {
    root: PathBuf,
    names: Vec<String>,
    /// The directory the file is in, and the new file's name in it.
    dir: PathBuf,
    temp: String,
    /// Every component when it was staged, and the new file as written.
    seen: Vec<Option<Seen>>,
    written: Option<Seen>,
    committed: bool,
}

impl Staged {
    /// Every component, and the new file, as they were when it was
    /// staged.
    fn check(&self) -> io::Result<()> {
        let names: Vec<&str> = self.names.iter().map(String::as_str).collect();
        if look(&self.root, &names, true)? != self.seen {
            return Err(changed(&names));
        }
        let temp = std::fs::symlink_metadata(self.dir.join(&self.temp))?;
        if is_link(&temp) || Some(Seen::of(&temp)) != self.written {
            return Err(changed(&names));
        }
        Ok(())
    }

    /// Look at everything once more, then replace the file with the new
    /// one, in one step.
    pub fn commit(mut self) -> io::Result<()> {
        self.check()?;
        let name = self.names.last().map_or("", String::as_str);
        std::fs::rename(self.dir.join(&self.temp), self.dir.join(name))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            // A link swapped in for the new file is removed itself, never
            // followed.
            let _ = std::fs::remove_file(self.dir.join(&self.temp));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ByPath;
    use std::path::{Path, PathBuf};

    /// A fresh directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("oskac-by-path-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("src")).expect("a scratch directory");
            std::fs::write(dir.join("src/app.osk"), "old").expect("the file");
            Scratch(dir)
        }

        fn text(&self, path: &str) -> String {
            std::fs::read_to_string(self.0.join(path)).expect("the file")
        }

        fn entries(&self, dir: &str) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(self.0.join(dir))
                .expect("the directory")
                .map(|e| {
                    e.expect("an entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A symlink at `at` to `target`, or `false`, saying why, where this
    /// OS or user may not make one: Windows without the privilege or
    /// developer mode.
    fn link(target: &Path, at: &Path) -> bool {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, at);
        #[cfg(windows)]
        let made = if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, at)
        } else {
            std::os::windows::fs::symlink_file(target, at)
        };
        #[cfg(not(any(unix, windows)))]
        let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
        match made {
            Ok(()) => true,
            Err(err) => {
                println!("skipped: cannot make a symlink here: {err}");
                false
            }
        }
    }

    #[test]
    fn a_file_is_read_and_replaced_and_nothing_else_is_left() {
        let dir = Scratch::new("replaced");
        let files = ByPath::open(&dir.0).expect("the directory");
        assert_eq!(files.read(&["src", "app.osk"]).expect("read"), b"old");
        let staged = files.stage(&["src", "app.osk"], b"new").expect("staged");
        assert_eq!(dir.entries("src"), [".app.osk.oskac-fix", "app.osk"]);
        staged.commit().expect("committed");
        assert_eq!(dir.text("src/app.osk"), "new");
        assert_eq!(dir.entries("src"), ["app.osk"]);
    }

    #[test]
    fn a_missing_file_is_created() {
        let dir = Scratch::new("created");
        let files = ByPath::open(&dir.0).expect("the directory");
        let staged = files.stage(&["new.json"], b"{}").expect("staged");
        staged.commit().expect("committed");
        assert_eq!(dir.text("new.json"), "{}");
    }

    #[test]
    fn a_symlinked_file_or_parent_is_never_read_or_written() {
        let dir = Scratch::new("links");
        if !(link(&dir.0.join("src/app.osk"), &dir.0.join("alias.osk"))
            && link(&dir.0.join("src"), &dir.0.join("lib")))
        {
            return;
        }
        let files = ByPath::open(&dir.0).expect("the directory");
        for names in [&["alias.osk"][..], &["lib", "app.osk"]] {
            let read = files.read(names).map(|_| ());
            assert!(
                read.as_ref()
                    .is_err_and(|e| e.to_string().contains("symlink")),
                "{names:?}: {read:?}"
            );
            let staged = files.stage(names, b"new").map(|_| ());
            assert!(staged.is_err(), "{names:?}");
        }
        assert_eq!(dir.text("src/app.osk"), "old");
        assert_eq!(dir.entries("src"), ["app.osk"]);
    }

    #[test]
    fn a_file_swapped_for_a_symlink_after_staging_is_not_replaced() {
        let dir = Scratch::new("swap-file");
        let files = ByPath::open(&dir.0).expect("the directory");
        let staged = files.stage(&["src", "app.osk"], b"new").expect("staged");
        std::fs::rename(dir.0.join("src/app.osk"), dir.0.join("real.osk")).expect("a rename");
        if !link(&dir.0.join("real.osk"), &dir.0.join("src/app.osk")) {
            return;
        }
        let committed = staged.commit();
        assert!(committed.is_err(), "{committed:?}");
        assert_eq!(dir.text("real.osk"), "old");
        assert_eq!(dir.entries("src"), ["app.osk"], "the new file is removed");
    }

    #[test]
    fn a_parent_swapped_for_a_symlink_after_staging_is_not_written_through() {
        let dir = Scratch::new("swap-parent");
        let files = ByPath::open(&dir.0).expect("the directory");
        let staged = files.stage(&["src", "app.osk"], b"new").expect("staged");
        std::fs::rename(dir.0.join("src"), dir.0.join("elsewhere")).expect("a rename");
        if !link(&dir.0.join("elsewhere"), &dir.0.join("src")) {
            return;
        }
        let committed = staged.commit();
        assert!(
            committed
                .as_ref()
                .is_err_and(|e| e.to_string().contains("symlink")),
            "{committed:?}"
        );
        assert_eq!(dir.text("elsewhere/app.osk"), "old");
    }

    #[test]
    fn a_parent_swapped_for_another_directory_after_staging_is_not_written_into() {
        let dir = Scratch::new("swap-dir");
        let files = ByPath::open(&dir.0).expect("the directory");
        let staged = files.stage(&["src", "app.osk"], b"new").expect("staged");
        std::fs::rename(dir.0.join("src"), dir.0.join("moved")).expect("a rename");
        // Created later, so it is told apart by its creation time where the
        // file system records one.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir(dir.0.join("src")).expect("a directory");
        std::fs::write(dir.0.join("src/app.osk"), "other").expect("a file");
        let committed = staged.commit();
        assert!(committed.is_err(), "{committed:?}");
        assert_eq!(dir.text("src/app.osk"), "other");
        assert_eq!(dir.text("moved/app.osk"), "old");
    }
}
