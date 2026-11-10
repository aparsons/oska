//! [`Dir`] on Unix: `openat`, one component at a time, never following a
//! symlink.

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The C library's declarations this crate uses, with each platform's
/// values from its own headers. Only the targets Oska releases for are
/// covered, so a wrong value cannot slip in for an untested one.
mod c {
    use std::os::raw::{c_char, c_int};

    extern "C" {
        /// `int openat(int dirfd, const char *path, int flags, ...)`.
        /// Variadic, as in C: on Apple's arm64 ABI a variadic argument is
        /// passed differently from a fixed one. No caller here passes
        /// `O_CREAT`, so none passes the mode.
        pub fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    }

    pub const O_RDONLY: c_int = 0;

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    mod values {
        use std::os::raw::c_int;
        pub const O_NONBLOCK: c_int = 0o4000;
        pub const O_DIRECTORY: c_int = 0o200000;
        pub const O_NOFOLLOW: c_int = 0o400000;
        pub const O_CLOEXEC: c_int = 0o2000000;
        pub const AT_FDCWD: c_int = -100;
        pub const ENOTDIR: i32 = 20;
        pub const ELOOP: i32 = 40;
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    mod values {
        use std::os::raw::c_int;
        pub const O_NONBLOCK: c_int = 0o4000;
        pub const O_DIRECTORY: c_int = 0o40000;
        pub const O_NOFOLLOW: c_int = 0o100000;
        pub const O_CLOEXEC: c_int = 0o2000000;
        pub const AT_FDCWD: c_int = -100;
        pub const ENOTDIR: i32 = 20;
        pub const ELOOP: i32 = 40;
    }

    #[cfg(all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    mod values {
        use std::os::raw::c_int;
        pub const O_NONBLOCK: c_int = 0x4;
        pub const O_DIRECTORY: c_int = 0x100000;
        pub const O_NOFOLLOW: c_int = 0x100;
        pub const O_CLOEXEC: c_int = 0x1000000;
        pub const AT_FDCWD: c_int = -2;
        pub const ENOTDIR: i32 = 20;
        pub const ELOOP: i32 = 62;
    }

    #[cfg(not(any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(
            target_os = "macos",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    compile_error!(
        "oska-sys knows the C library's values only for Linux and macOS on x86_64 and aarch64"
    );

    pub use values::*;

    /// Flags for opening a directory to walk beneath: never following a
    /// symlink, and closed in a child process (`Process` spawns them).
    pub const DIRECTORY: c_int = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC;

    /// Flags for opening a trusted directory by path, following symlinks.
    pub const DIRECTORY_FOLLOWING: c_int = O_RDONLY | O_DIRECTORY | O_CLOEXEC;

    /// Flags for opening a file to read. `O_NONBLOCK` keeps a FIFO from
    /// blocking the open; [`super::Dir::open_file`] then refuses it.
    pub const FILE_READ: c_int = O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK;
}

/// An open directory: the root of a confined capability, or a directory
/// beneath one. Dropping it closes the handle.
#[derive(Debug)]
pub struct Dir {
    fd: OwnedFd,
}

impl Dir {
    /// Open the directory at `path`, relative to the working directory
    /// when it is not absolute, following symlinks in every component as
    /// any open by path does: `path` is the caller's trusted choice.
    ///
    /// A caller that must refuse a symlink as the last component opens
    /// the parent with this, then the last name with [`Dir::open_dir`] or
    /// [`Dir::open_file`].
    pub fn open(path: &Path) -> io::Result<Dir> {
        if path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an empty path names no directory",
            ));
        }
        let fd =
            open_at(c::AT_FDCWD, path.as_os_str(), c::DIRECTORY_FOLLOWING).map_err(|error| {
                io::Error::new(error.kind(), format!("`{}`: {error}", path.display()))
            })?;
        Ok(Dir { fd })
    }

    /// The directory reached by walking `names` beneath this one, each a
    /// directory and none a symlink. With no names, a new handle for this
    /// directory.
    ///
    /// Each name must be one plain component: not empty, `.` or `..`, and
    /// holding no `/` or NUL. The caller resolves `..` on the path as
    /// written, before the walk, so the walk never climbs.
    pub fn open_dir<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<Dir> {
        let Some((last, parents)) = names.split_last() else {
            return Ok(Dir {
                fd: self.fd.try_clone()?,
            });
        };
        let parent = self.walk(parents)?;
        let fd = open_name(parent.as_ref().unwrap_or(self), last.as_ref(), c::DIRECTORY)
            .map_err(|error| refusal(error, &joined(names), "directory"))?;
        Ok(Dir { fd })
    }

    /// The regular file reached by walking `names` beneath this one, open
    /// for reading: every name before the last a directory, and none a
    /// symlink. Names are as for [`Dir::open_dir`]; there must be one.
    pub fn open_file<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<File> {
        let Some((last, parents)) = names.split_last() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no file name to open",
            ));
        };
        let parent = self.walk(parents)?;
        let fd = open_name(parent.as_ref().unwrap_or(self), last.as_ref(), c::FILE_READ)
            .map_err(|error| refusal(error, &joined(names), "file"))?;
        let file = File::from(fd);
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` is not a regular file", joined(names)),
            ));
        }
        Ok(file)
    }

    /// The directory reached by walking `names`, or `None` for this one.
    fn walk<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<Option<Dir>> {
        let mut at: Option<Dir> = None;
        for (walked, name) in names.iter().enumerate() {
            let fd = open_name(at.as_ref().unwrap_or(self), name.as_ref(), c::DIRECTORY)
                .map_err(|error| refusal(error, &joined(&names[..=walked]), "directory"))?;
            at = Some(Dir { fd });
        }
        Ok(at)
    }
}

/// Open the single component `name` beneath `dir` with `flags`.
fn open_name(dir: &Dir, name: &OsStr, flags: c_int) -> io::Result<OwnedFd> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{}` is not a single file name", name.to_string_lossy()),
        ));
    }
    open_at(dir.fd.as_raw_fd(), name, flags)
}

/// `openat(dirfd, path, flags)`, retried when a signal interrupts it.
fn open_at(dirfd: c_int, path: &OsStr, flags: c_int) -> io::Result<OwnedFd> {
    let path = CString::new(path.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path holds a NUL byte, which no file name may",
        )
    })?;
    let path: *const c_char = path.as_ptr();
    loop {
        // SAFETY: `path` points to a NUL-terminated string that outlives
        // the call. `dirfd` is `AT_FDCWD` or the descriptor of a live
        // `Dir`, borrowed for the call. `flags` holds no `O_CREAT` or
        // `O_TMPFILE`, so `openat` reads no variadic mode argument.
        let fd = unsafe { c::openat(dirfd, path, flags) };
        if fd >= 0 {
            // SAFETY: `openat` succeeded, so `fd` is a new descriptor that
            // nothing else owns or will close.
            return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// `error` from opening `path` as a `kind`, with the path named, and a
/// symlink said plainly: `openat` reports one met with `O_NOFOLLOW` as
/// `ELOOP`, or as `ENOTDIR` when a directory was asked for.
fn refusal(error: io::Error, path: &str, kind: &str) -> io::Error {
    match error.raw_os_error() {
        Some(c::ELOOP) => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("`{path}` is a symlink, and confined access never follows one"),
        ),
        Some(c::ENOTDIR) if kind == "directory" => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("`{path}` is a symlink or not a directory, and confined access never follows a symlink"),
        ),
        _ => io::Error::new(error.kind(), format!("`{path}`: {error}")),
    }
}

/// `names` joined with `/`, for messages.
fn joined<N: AsRef<OsStr>>(names: &[N]) -> String {
    names
        .iter()
        .map(|name| name.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
