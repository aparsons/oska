//! [`Dir`] on Unix: `openat`, `mkdirat` and `fdopendir`, one component at
//! a time, never following a symlink.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::raw::{c_char, c_int, c_uint};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The C library's declarations this crate uses, with each platform's
/// values from its own headers. Only the targets Oska releases for are
/// covered, so a wrong value cannot slip in for an untested one.
mod c {
    use std::os::raw::{c_char, c_int, c_void};

    pub use values::*;

    /// A `DIR *`, which only the C library looks inside.
    pub type DirStream = c_void;

    extern "C" {
        /// `int openat(int dirfd, const char *path, int flags, ...)`.
        /// Variadic, as in C: on Apple's arm64 ABI a variadic argument is
        /// passed differently from a fixed one. The mode, read only with
        /// `O_CREAT`, is passed as the `unsigned int` C promotes it to.
        pub fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;

        /// `int mkdirat(int dirfd, const char *path, mode_t mode)`.
        pub fn mkdirat(dirfd: c_int, path: *const c_char, mode: Mode) -> c_int;

        /// `DIR *fdopendir(int fd)`: on success the stream owns `fd`.
        /// Intel macOS keeps a 32-bit-inode `dirent` under the plain name,
        /// so it takes the `$INODE64` one, as its own headers do.
        #[cfg_attr(
            all(target_os = "macos", target_arch = "x86_64"),
            link_name = "fdopendir$INODE64"
        )]
        pub fn fdopendir(fd: c_int) -> *mut DirStream;

        /// `struct dirent *readdir(DIR *stream)`: `NULL` at the end, or
        /// on an error, which sets `errno`.
        #[cfg_attr(
            all(target_os = "macos", target_arch = "x86_64"),
            link_name = "readdir$INODE64"
        )]
        pub fn readdir(stream: *mut DirStream) -> *const c_void;

        /// `int closedir(DIR *stream)`, which closes its descriptor.
        pub fn closedir(stream: *mut DirStream) -> c_int;

        /// The calling thread's `errno`, which `readdir` leaves alone at
        /// the end of a directory, so it is cleared before each call.
        #[cfg_attr(target_os = "linux", link_name = "__errno_location")]
        #[cfg_attr(target_os = "macos", link_name = "__error")]
        pub fn errno_location() -> *mut c_int;
    }

    pub const O_RDONLY: c_int = 0;
    pub const O_WRONLY: c_int = 1;

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    mod values {
        use std::os::raw::c_int;
        pub type Mode = u32;
        pub const O_CREAT: c_int = 0o100;
        pub const D_NAME_OFFSET: usize = 19;
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
        pub type Mode = u32;
        pub const O_CREAT: c_int = 0o100;
        pub const D_NAME_OFFSET: usize = 19;
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
        pub type Mode = u16;
        pub const O_CREAT: c_int = 0x200;
        /// `struct dirent` with 64-bit inodes, the layout of the
        /// `$INODE64` functions on Intel and the only one on Apple silicon.
        pub const D_NAME_OFFSET: usize = 21;
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

    /// Flags for opening a directory to walk beneath: never following a
    /// symlink, and closed in a child process (`Process` spawns them).
    pub const DIRECTORY: c_int = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC;

    /// Flags for opening a trusted directory by path, following symlinks.
    pub const DIRECTORY_FOLLOWING: c_int = O_RDONLY | O_DIRECTORY | O_CLOEXEC;

    /// Flags for opening a file to read. `O_NONBLOCK` keeps a FIFO from
    /// blocking the open; [`super::Dir::open_file`] then refuses it.
    pub const FILE_READ: c_int = O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK;

    /// Flags for opening or creating a file to write. Not `O_TRUNC`:
    /// [`super::Dir::create_file`] empties the file only once it is known
    /// to be a regular one. `O_NONBLOCK` makes opening a FIFO with no
    /// reader fail rather than wait.
    pub const FILE_WRITE: c_int = O_WRONLY | O_CREAT | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK;

    /// The mode of a new file and a new directory, before the umask.
    pub const FILE_MODE: Mode = 0o666;
    pub const DIR_MODE: Mode = 0o777;
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
        let fd = c_string(path.as_os_str())
            .and_then(|path| open_at(c::AT_FDCWD, &path, c::DIRECTORY_FOLLOWING, 0))
            .map_err(|error| {
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
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let parent = self.walk(parents)?;
        let fd = open_name(parent.as_ref().unwrap_or(self), last.as_ref(), c::FILE_READ)
            .map_err(|error| refusal(error, &joined(names), "file"))?;
        let file = File::from(fd);
        if !file.metadata()?.is_file() {
            return Err(not_a_file(names));
        }
        Ok(file)
    }

    /// The regular file reached by walking `names` beneath this one, open
    /// for writing and emptied: created, with mode `0o666` less the
    /// umask, when nothing is there. Every name before the last is a
    /// directory, and none is a symlink, the last included, so a write
    /// never goes through one. Names are as for [`Dir::open_dir`]; there
    /// must be one.
    pub fn create_file<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<File> {
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let parent = self.walk(parents)?;
        let fd = open_name(
            parent.as_ref().unwrap_or(self),
            last.as_ref(),
            c::FILE_WRITE,
        )
        .map_err(|error| refusal(error, &joined(names), "file"))?;
        let file = File::from(fd);
        // Emptied only once it is known to be a regular file, never a
        // device or a FIFO.
        if !file.metadata()?.is_file() {
            return Err(not_a_file(names));
        }
        file.set_len(0)?;
        Ok(file)
    }

    /// Create the directory reached by walking `names` beneath this one,
    /// with mode `0o777` less the umask. A directory already there is
    /// kept; anything else there, a symlink included, is an error. Every
    /// name before the last is a directory, and none is a symlink. Names
    /// are as for [`Dir::open_dir`]; there must be one.
    pub fn create_dir<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<()> {
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let walked = self.walk(parents)?;
        let parent = walked.as_ref().unwrap_or(self);
        let name = plain_name(last.as_ref())?;
        let made = retried(|| {
            // SAFETY: `name` is a NUL-terminated string that outlives the
            // call, and `parent.fd` the descriptor of a live `Dir`.
            unsafe { c::mkdirat(parent.fd.as_raw_fd(), name.as_ptr(), c::DIR_MODE) }
        });
        match made {
            Ok(_) => Ok(()),
            // `mkdirat` never follows a symlink in the last component: one
            // there, dangling or not, makes it fail with `EEXIST`. What is
            // there is kept if it opens as a directory, not following a
            // symlink.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                open_name(parent, last.as_ref(), c::DIRECTORY)
                    .map(drop)
                    .map_err(|error| refusal(error, &joined(names), "directory"))
            }
            Err(error) => Err(refusal(error, &joined(names), "directory")),
        }
    }

    /// The names of the entries of this directory, in the order the
    /// directory gives them, without `.` and `..`. No entry is followed:
    /// a symlink is listed by its own name.
    pub fn entries(&self) -> io::Result<Vec<OsString>> {
        // A new open file description of this directory, so the listing
        // starts at its beginning and moves no other handle's offset.
        let fd = open_at(self.fd.as_raw_fd(), c".", c::DIRECTORY, 0)?;
        let stream = Stream::new(fd)?;
        let mut names = Vec::new();
        while let Some(name) = stream.next_name()? {
            if name != "." && name != ".." {
                names.push(name);
            }
        }
        Ok(names)
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

/// A directory stream from `fdopendir`, closed on drop.
struct Stream {
    stream: *mut c::DirStream,
}

impl Stream {
    /// A stream over the open directory `fd`, which it then owns.
    fn new(fd: OwnedFd) -> io::Result<Stream> {
        // SAFETY: `fd` is an open directory descriptor, kept open for the
        // call.
        let stream = unsafe { c::fdopendir(fd.as_raw_fd()) };
        if stream.is_null() {
            // `fd` is still ours, and closed as it drops.
            return Err(io::Error::last_os_error());
        }
        // The stream owns the descriptor now, and closes it.
        let _ = fd.into_raw_fd();
        Ok(Stream { stream })
    }

    /// The next entry's name, or `None` at the end of the directory.
    fn next_name(&self) -> io::Result<Option<OsString>> {
        // SAFETY: `errno_location` takes no arguments and returns the
        // address of the calling thread's `errno`.
        let errno = unsafe { c::errno_location() };
        // SAFETY: `errno` is the address of this thread's `errno`, a live
        // `int` for as long as the thread runs.
        unsafe { errno.write(0) };
        // SAFETY: `self.stream` came from a successful `fdopendir`, is
        // closed only when `self` drops, and only this thread uses it.
        let entry = unsafe { c::readdir(self.stream) };
        if entry.is_null() {
            // The end of the directory leaves `errno` at 0; an error sets it.
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(0) => Ok(None),
                _ => Err(error),
            };
        }
        let name: *const c_char = entry.cast::<c_char>().wrapping_add(c::D_NAME_OFFSET);
        // SAFETY: `entry` is the `struct dirent` that `readdir` returned,
        // valid until the next call on this stream, and its `d_name`, at
        // `D_NAME_OFFSET` on this platform, is NUL-terminated. The bytes
        // are copied out before the next call.
        let name = unsafe { CStr::from_ptr(name) };
        Ok(Some(OsStr::from_bytes(name.to_bytes()).to_os_string()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: `self.stream` came from a successful `fdopendir`, and is
        // closed here only, once. Like `close`, `closedir` is not retried
        // on an error: the descriptor is released either way.
        unsafe { c::closedir(self.stream) };
    }
}

/// `name` as a C string, if it is one plain component: not empty, `.`
/// or `..`, and holding no `/` or NUL.
fn plain_name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{}` is not a single file name", name.to_string_lossy()),
        ));
    }
    c_string(name)
}

/// `path` as a C string, refused if it holds a NUL byte.
fn c_string(path: &OsStr) -> io::Result<CString> {
    CString::new(path.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path holds a NUL byte, which no file name may",
        )
    })
}

/// Open the single component `name` beneath `dir` with `flags`, creating
/// a file with mode [`c::FILE_MODE`] when they hold `O_CREAT`.
fn open_name(dir: &Dir, name: &OsStr, flags: c_int) -> io::Result<OwnedFd> {
    open_at(dir.fd.as_raw_fd(), &plain_name(name)?, flags, c::FILE_MODE)
}

/// `openat(dirfd, path, flags, mode)`, retried when a signal interrupts
/// it.
fn open_at(dirfd: c_int, path: &CStr, flags: c_int, mode: c::Mode) -> io::Result<OwnedFd> {
    let fd = retried(|| {
        // SAFETY: `path` is a NUL-terminated string that outlives the
        // call. `dirfd` is `AT_FDCWD` or the descriptor of a live `Dir`,
        // borrowed for the call. The mode is passed as the `unsigned int`
        // that `openat` reads when `flags` holds `O_CREAT`; without it,
        // `openat` reads no variadic argument, and C allows an extra one.
        unsafe { c::openat(dirfd, path.as_ptr(), flags, c_uint::from(mode)) }
    })?;
    // SAFETY: `openat` succeeded, so `fd` is a new descriptor that nothing
    // else owns or will close.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// What `call` returned, a descriptor or 0, or the error it left in
/// `errno` when it returned -1, calling it again when a signal
/// interrupted it.
fn retried(mut call: impl FnMut() -> c_int) -> io::Result<c_int> {
    loop {
        let result = call();
        if result >= 0 {
            return Ok(result);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// The error for a file operation given no name.
fn no_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no file name to open")
}

/// The error for the file `names` reach not being a regular file.
fn not_a_file<N: AsRef<OsStr>>(names: &[N]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is not a regular file", joined(names)),
    )
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
