// SPDX-License-Identifier: Apache-2.0
//! The [`Host`]: the interpreter's only way to reach the OS.
//!
//! Every intrinsic that touches the outside world goes through it, so a
//! program behaves the same on [`RealHost`] and [`FakeHost`] except for
//! what the world gives back.
//!
//! The host offers primitives only. Policy lives in the runtime
//! (`confine.rs`), so it is the same on every host: it turns a program's
//! path into the plain names to walk beneath a root, and the host walks
//! them from the [`RootHandle`] it opened, never following a symlink, to
//! read, write, create a directory or list one.
//!
//! To add an operation: add a method to [`Host`], implement it on both
//! hosts (`FakeHost` deterministically, in memory), and call it from the
//! intrinsic that needs it (`intrinsics.rs`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::prng::SplitMix64;
use oska_syntax::SnapshotRecord;

pub use oska_sys::{Kind, Meta};

/// What a program wrote to its standard streams, as a host recorded it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// The operating system, as the interpreter sees it.
///
/// A host is `Send` because the interpreter runs on a thread of its own,
/// with a stack sized for deep recursion (`crate::STACK_SIZE`).
pub trait Host: Send {
    /// Write `text` to standard output, as is.
    fn write_stdout(&mut self, text: &str) -> io::Result<()>;

    /// Write `text` to standard error, as is.
    fn write_stderr(&mut self, text: &str) -> io::Result<()>;

    /// The next line of standard input, without its line break, or `None`
    /// at end of input.
    fn read_line(&mut self) -> io::Result<Option<String>>;

    /// [`Host::read_line`], but waiting no later than `deadline`
    /// (`--timeout`): `None` when the deadline passes first. A host whose
    /// reads never wait, such as a fake one, reads at once.
    fn read_line_before(&mut self, deadline: Instant) -> Option<io::Result<Option<String>>> {
        let _ = deadline;
        Some(self.read_line())
    }

    /// The environment variable `name`, if it is set to valid UTF-8.
    fn env_var(&mut self, name: &str) -> Option<String>;

    /// The current time, in milliseconds since the Unix epoch.
    fn now_millis(&mut self) -> i128;

    /// The next pseudo-random 64 bits.
    fn next_random(&mut self) -> u64;

    /// `len` bytes for keys and nonces. The real host takes them from the
    /// operating system's secure random source and returns its error if
    /// that fails. A test host gives [`generator_bytes`] explicitly. Every
    /// host says which it is: there is no default, so none can serve
    /// generator bytes as secure ones by accident.
    fn secure_bytes(&mut self, len: usize) -> io::Result<Vec<u8>>;

    /// Open the directory at `path` as the root of a directory
    /// capability. Its last component must not be a symlink; earlier ones
    /// are followed, since the root is trusted text. A file there is an
    /// error: a directory capability needs a directory.
    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle>;

    /// The directory reached by walking `names` beneath `root`, as a root
    /// of its own: each name a plain component, which `confine.rs`
    /// produced, and none a symlink. No names gives `root`. A file there
    /// is an error.
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle>;

    /// The root of a single file at `path`: its parent directory, opened
    /// as [`Host::open_root`] opens a root, together with the file's name,
    /// which is the last component and must be plain. The file itself is
    /// not opened, so it need not exist: a use opens the name beneath the
    /// parent, without following a symlink.
    fn open_file_root(&mut self, path: &Path) -> io::Result<RootHandle>;

    /// The root of the single file reached by walking `names` beneath
    /// `root`, a directory: the directory holding it, opened as
    /// [`Host::open_beneath`] opens one, together with the last name.
    /// `names` is at least one name. As for [`Host::open_file_root`], the
    /// file itself is not opened.
    fn open_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle>;

    /// The bytes of the regular file reached by walking `names` beneath
    /// `root`, none of them a symlink. When `root` is a single file,
    /// `names` is empty.
    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<u8>>;

    /// Create the regular file reached by walking `names` beneath `root`,
    /// or replace its contents, with `contents`. No name is a symlink,
    /// the last included, so nothing is created through one. When `root`
    /// is itself a file, `names` is empty.
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()>;

    /// Create the directory reached by walking `names` beneath `root`,
    /// none of them a symlink: one name at least, and every name before
    /// the last a directory that exists. A directory already there is
    /// kept; anything else there, a symlink included, is an error.
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()>;

    /// Take an exclusive lock on the directory `root`, for
    /// `DirWrite.with_lock`: the lock file [`oska_sys::LOCK_NAME`]
    /// beneath it, which is created if it is missing and kept. A lock
    /// that is already held, by this program or another process, fails at
    /// once with [`lock_held`]. The lock is released when the returned
    /// [`DirLock`] is dropped; a real host's is the operating system's,
    /// so it is also released when the process ends, however it ends.
    fn lock_beneath(&mut self, root: &RootHandle) -> io::Result<DirLock> {
        let _ = root;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this host cannot lock a directory",
        ))
    }

    /// The names of the entries of the directory reached by walking
    /// `names` beneath `root`, none of them a symlink, sorted by their
    /// bytes so the order is the same on every host. A name is the
    /// entry's own, not a path; a subdirectory and a symlink are listed by
    /// name, and no entry is followed. Fails when an entry's name is not
    /// valid UTF-8. No names lists `root`.
    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>>;

    /// Whether anything is at the path `names` reach beneath `root`, none
    /// of them a symlink: `false` when a name, or a directory before it,
    /// is not there. No names ask about `root` itself, which is there.
    fn exists_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<bool> {
        let _ = (root, names);
        Err(no_file_operation("exists"))
    }

    /// The kind and size of what `names` reach beneath `root`, none of
    /// them a symlink, and a file with more than one hard link refused as
    /// a read refuses it. No names describe `root`: a directory, or the
    /// one file a file root reaches.
    fn metadata_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Meta> {
        let _ = (root, names);
        Err(no_file_operation("metadata"))
    }

    /// Add `contents` to the end of the regular file reached by walking
    /// `names` beneath `root`, creating it when nothing is there. Nothing
    /// is followed or emptied; a file with more than one hard link is
    /// refused as a write refuses it. When `root` is itself a file,
    /// `names` is empty.
    fn append_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        let _ = (root, names, contents);
        Err(no_file_operation("append"))
    }

    /// Remove the regular file reached by walking `names` beneath `root`
    /// (one name at least). A symlink, a directory and a file with more
    /// than one hard link are refused, as a read refuses them.
    fn remove_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        let _ = (root, names);
        Err(no_file_operation("remove_file"))
    }

    /// Remove the empty directory reached by walking `names` beneath
    /// `root` (one name at least). A symlink is refused, and so is a
    /// directory with anything in it: nothing is removed recursively.
    fn remove_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        let _ = (root, names);
        Err(no_file_operation("remove_dir"))
    }

    /// Move the regular file reached by walking `from` beneath `root` to
    /// the path `to` reaches beneath it (each one name at least), replacing
    /// a regular file there. Both stay beneath `root`: there is no way to
    /// name anything else. A symlink, a directory or a file with more than
    /// one hard link at either name is refused.
    fn rename_beneath(
        &mut self,
        root: &RootHandle,
        from: &[String],
        to: &[String],
    ) -> io::Result<()> {
        let _ = (root, from, to);
        Err(no_file_operation("rename"))
    }

    /// Flush the regular file reached by walking `names` beneath `root`,
    /// none of them a symlink, to stable storage, data and metadata, as
    /// `FileWrite.sync` asks. It must exist: nothing is created. A file
    /// with more than one hard link is refused as a write refuses it.
    /// When `root` is itself a file, `names` is empty.
    fn sync_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        let _ = (root, names);
        Err(no_file_operation("sync"))
    }

    /// Flush the directory `root` to stable storage, so a name made,
    /// removed or moved in it is durable, as `DirWrite.sync` asks.
    fn sync_dir_beneath(&mut self, root: &RootHandle) -> io::Result<()> {
        let _ = root;
        Err(no_file_operation("sync"))
    }

    /// Open a connection to `host` at `port`, as `HostConnect.connect`
    /// asks. A host with no network refuses with an error whose message is
    /// the same every time.
    fn net_connect(&mut self, host: &str, port: i128) -> io::Result<oska_sys::NetStream> {
        let _ = (host, port);
        Err(no_network())
    }

    /// Listen on the loopback address at `port`, as
    /// `NetListen.restrict_to_loopback` asks.
    fn net_listen_loopback(&mut self, port: i128) -> io::Result<oska_sys::NetListener> {
        let _ = port;
        Err(no_network())
    }

    /// Send one datagram to the address literal `host` at `port` and
    /// return the one that comes back, as `HostConnect.exchange_datagram`
    /// asks. A host with no network refuses, as `net_connect` does.
    fn net_exchange_datagram(
        &mut self,
        host: &str,
        port: i128,
        message: &[u8],
    ) -> io::Result<Vec<u8>> {
        let _ = (host, port, message);
        Err(no_network())
    }

    /// The DNS servers the operating system lists, as
    /// `NetConnect.system_name_servers` asks. A host with no network
    /// lists none.
    fn net_name_servers(&mut self) -> io::Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Where the program has tried to go, in order, as `t.net_attempts()`
    /// returns them: `host:port`, or `loopback:port` for a listener. A host
    /// that records none returns none.
    fn net_attempts(&self) -> Vec<String> {
        Vec::new()
    }

    /// The syncs a program has asked for, in order: `file ` or `dir `
    /// and the path, as a host that models a crash reads them. A host
    /// that records none returns none.
    fn syncs(&self) -> Vec<String> {
        Vec::new()
    }

    /// Every line written to stdout so far, without its line break: one
    /// per `println`, as `t.printed()` returns them. A host that writes
    /// straight to the real stream records none.
    fn printed(&self) -> Vec<String>;

    /// Record what a snapshot assertion was given ([`SnapshotRecord`]),
    /// for the test runner to compare with its file after the test
    /// (`crate::snapshots`). Only a test's host records it; any other
    /// drops it, as no other code can call it.
    fn record_snapshot(&mut self, _record: SnapshotRecord) {}

    /// Hand over what the host has recorded of the program's output since
    /// the last call. A host that writes straight to the real streams
    /// records nothing.
    fn take_output(&mut self) -> Output;
}

/// The error of a host that offers no file operation beyond the first
/// six: it refuses, with the same message every time.
fn no_file_operation(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("`{name}` is not available on this host"),
    )
}

/// The error of a host with no network, such as a plain test's: the same
/// message on every platform, so a test can assert it.
fn no_network() -> io::Error {
    io::Error::other("network access is not available here: a plain test is offline")
}

/// A directory lock a host holds (`Host::lock_beneath`): dropping it
/// releases the lock.
pub struct DirLock {
    _held: Box<dyn std::any::Any>,
}

impl DirLock {
    fn new(held: impl std::any::Any) -> DirLock {
        DirLock {
            _held: Box::new(held),
        }
    }
}

/// The error for a directory lock that is already held: its code and the
/// lock's name, worded as `oska_sys` words it and `std.rt` does.
pub fn lock_held() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "{}: the directory lock `{}` is already held",
            oska_sys::LOCK_CODE,
            oska_sys::LOCK_NAME
        ),
    )
}

/// The root of a directory or file capability, as a host opened it: a directory,
/// or the one file a file capability reaches. Copies share it, and the
/// last one dropped releases what the host holds for it.
///
/// On a [`RealHost`] it holds an OS handle for the directory, or for a
/// file's parent directory together with the file's name, and every
/// operation beneath it walks from that handle (`oska-sys`). On
/// [`FakeHost`] it holds only the root's path.
#[derive(Debug, Clone)]
pub struct RootHandle {
    /// The root's canonical path, which the path-based operations
    /// resolve under.
    path: Rc<Path>,
    /// The file's own name, when the root is a file.
    file: Option<Rc<OsStr>>,
    /// The directory's handle, or the file's parent's: `None` on a
    /// [`FakeHost`].
    os: Option<Rc<oska_sys::Dir>>,
}

impl RootHandle {
    /// A root at `path`: the directory itself, or with `file`, the file
    /// of that name, whose parent directory `os` holds when the host
    /// keeps a handle.
    fn new(path: &Path, file: Option<&OsStr>, os: Option<oska_sys::Dir>) -> RootHandle {
        RootHandle {
            path: Rc::from(path),
            file: file.map(Rc::from),
            os: os.map(Rc::new),
        }
    }

    /// The root's canonical path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the root is a single file rather than a directory.
    pub fn is_file(&self) -> bool {
        self.file.is_some()
    }
}

/// Two handles are equal when they name the same root.
impl PartialEq for RootHandle {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.file == other.file
    }
}

impl Eq for RootHandle {}

/// Whether `names` narrow `root` at all, for `open_beneath`: no names
/// leave it as it is, and a file root holds nothing to narrow to.
fn narrows(root: &RootHandle, names: &[String]) -> io::Result<bool> {
    if root.is_file() && !names.is_empty() {
        return Err(beneath_a_file(names));
    }
    Ok(!names.is_empty())
}

/// The parents and the file name of a file `names` reach beneath `root`,
/// for `open_file_beneath`: a file root holds nothing, and there must be
/// a name.
fn file_beneath<'n>(
    root: &RootHandle,
    names: &'n [String],
) -> io::Result<(&'n String, &'n [String])> {
    if root.is_file() {
        return Err(beneath_a_file(names));
    }
    names.split_last().ok_or_else(no_name)
}

/// A file root's trusted `path`, without a trailing `/` or `.`, split
/// into its parent (the current directory when it has none) and the
/// file's name, for `open_file_root`.
fn split_file_root(path: &Path) -> io::Result<(PathBuf, std::ffi::OsString)> {
    let path: PathBuf = path.components().collect();
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok((or_here(parent).to_path_buf(), name.to_os_string())),
        _ => Err(no_file_name(&path)),
    }
}

/// The error for `names` walked beneath a root that is a single file.
fn beneath_a_file(names: &[String]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "`{}` is beneath a single file, which holds no other file",
            names.join("/")
        ),
    )
}

/// The error for listing, or creating a directory in, a root that is a
/// single file.
fn a_file_root(root: &RootHandle) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is a single file, not a directory", shown(&root.path)),
    )
}

/// `entries` of the directory at `path` as text, sorted by their bytes,
/// or the error for the first that is not valid UTF-8.
fn sorted_names(entries: Vec<std::ffi::OsString>, path: &Path) -> io::Result<Vec<String>> {
    let mut names = entries
        .into_iter()
        .map(|name| {
            name.into_string().map_err(|name| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{name:?} in `{}` is not valid UTF-8", path.display()),
                )
            })
        })
        .collect::<io::Result<Vec<String>>>()?;
    names.sort();
    Ok(names)
}

/// `root`'s path joined with `names`.
pub(crate) fn joined(root: &RootHandle, names: &[String]) -> PathBuf {
    names
        .iter()
        .fold(root.path.to_path_buf(), |path, name| path.join(name))
}

/// The real OS: backs `oskac run` and integration tests.
#[derive(Debug)]
pub struct RealHost {
    random: SplitMix64,
    /// Whether the program's standard input is closed: every read is
    /// end of file at once (see [`RealHost::without_stdin`]).
    stdin_closed: bool,
    /// Under `--timeout`, once the program first reads: the lines a
    /// thread of its own reads from standard input, so that a read can
    /// stop waiting at the deadline ([`Host::read_line_before`]).
    lines: Option<mpsc::Receiver<io::Result<Option<String>>>>,
    /// The hosts file, `resolv.conf` and name server port `std.dns` uses
    /// in place of this machine's (see [`RealHost::with_name_files`]).
    name_files: Option<crate::resolve::Files>,
    /// Whether `std.dns` honors the `OSKA_DNS_*` test variables
    /// ([`RealHost::with_dns_test_hooks`]); off unless the embedder says.
    dns_test_hooks: bool,
}

impl Default for RealHost {
    fn default() -> Self {
        RealHost::new()
    }
}

impl RealHost {
    /// A real host, its generator seeded from the clock and the process.
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let seed = (nanos as u64) ^ ((nanos >> 64) as u64) ^ u64::from(std::process::id()) << 32;
        RealHost {
            random: SplitMix64::new(seed),
            stdin_closed: false,
            lines: None,
            name_files: None,
            dns_test_hooks: false,
        }
    }

    /// Whether name resolution honors the environment variables
    /// `OSKA_DNS_HOSTS`, `OSKA_DNS_RESOLV_CONF` and `OSKA_DNS_PORT` (ADR
    /// 0026), which a test uses to redirect lookups. Off by default: only
    /// the embedder decides, and `oskac` turns it on in debug builds
    /// alone, so that whoever starts a process of a release build cannot
    /// redirect its lookups. `connect`'s flag comes only from here.
    pub fn with_dns_test_hooks(mut self, on: bool) -> Self {
        self.dns_test_hooks = on;
        self
    }

    /// A real host whose name resolution reads `files` (the hosts file and
    /// `resolv.conf` as text, and the name servers' port) instead of this
    /// machine's, so a test can point `std.dns` at a name server of its
    /// own. The resolver is the same.
    pub fn with_name_files(mut self, files: crate::resolve::Files) -> Self {
        self.name_files = Some(files);
        self
    }

    /// A real host whose standard input is closed: every read is end of
    /// file at once, as for a program `oskac run -` read from the
    /// process's own standard input.
    pub fn without_stdin() -> Self {
        RealHost {
            stdin_closed: true,
            ..RealHost::new()
        }
    }
}

/// `len` bytes from `next`'s 64-bit words, each little-endian, the last
/// one cut short: what a test host gives for `Random.secure_bytes`, so a
/// test replays byte for byte. Never for a real host.
pub fn generator_bytes(mut next: impl FnMut() -> u64, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        out.extend_from_slice(&next().to_le_bytes());
    }
    out.truncate(len);
    out
}

impl Host for RealHost {
    /// A closed pipe ends the process quietly with 0, as `head` expects
    /// of what it reads, and as the native runtime does: the program
    /// writes nothing more and reports nothing. Any other error is the
    /// program's to handle.
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        match io::stdout().write_all(text.as_bytes()) {
            Err(err) if err.kind() == io::ErrorKind::BrokenPipe => std::process::exit(0),
            result => result,
        }
    }

    fn write_stderr(&mut self, text: &str) -> io::Result<()> {
        io::stderr().write_all(text.as_bytes())
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        if self.stdin_closed {
            return Ok(None);
        }
        if let Some(lines) = &self.lines {
            return lines.recv().unwrap_or(Ok(None));
        }
        read_stdin_line()
    }

    /// The read happens on a thread of its own, which nothing can
    /// interrupt portably; at the deadline the program stops, and the
    /// thread ends with the process.
    fn read_line_before(&mut self, deadline: Instant) -> Option<io::Result<Option<String>>> {
        if self.stdin_closed {
            return Some(Ok(None));
        }
        let lines = self.lines.get_or_insert_with(|| {
            let (send, lines) = mpsc::channel();
            std::thread::spawn(move || loop {
                let line = read_stdin_line();
                let more = matches!(line, Ok(Some(_)));
                if send.send(line).is_err() || !more {
                    break;
                }
            });
            lines
        });
        let wait = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(wait) {
            Ok(line) => Some(line),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            // The reader has ended: end of input, as before.
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Ok(None)),
        }
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn now_millis(&mut self) -> i128 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(since) => since.as_millis() as i128,
            Err(before) => -(before.duration().as_millis() as i128),
        }
    }

    fn next_random(&mut self) -> u64 {
        self.random.next()
    }

    fn secure_bytes(&mut self, len: usize) -> io::Result<Vec<u8>> {
        let mut out = vec![0; len];
        oska_sys::fill_random(&mut out)?;
        Ok(out)
    }

    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        // Without a trailing `/` or `.`, which would follow a symlink in
        // the last component.
        let path: PathBuf = with_backslashes(path).components().collect();
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            // `/`, or a path ending in `..`: no symlink to refuse.
            let dir = oska_sys::Dir::open(&path)?;
            return Ok(RootHandle::new(
                &std::fs::canonicalize(&path)?,
                None,
                Some(dir),
            ));
        };
        let dir = oska_sys::Dir::open(or_here(parent))?.open_dir(&[name])?;
        Ok(RootHandle::new(
            &std::fs::canonicalize(&path)?,
            None,
            Some(dir),
        ))
    }

    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        if !narrows(root, names)? {
            return Ok(root.clone());
        }
        let dir = checked_names(root, names)?.open_dir(names)?;
        // Canonical as it stands: the walk met no symlink. Not
        // canonicalized again, which would follow one swapped in since.
        Ok(RootHandle::new(&joined(root, names), None, Some(dir)))
    }

    fn open_file_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        let (parent, name) = split_file_root(&with_backslashes(path))?;
        let dir = oska_sys::Dir::open(&parent)?;
        let path = std::fs::canonicalize(&parent)?.join(&name);
        Ok(RootHandle::new(&path, Some(&name), Some(dir)))
    }

    fn open_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        let (name, parents) = file_beneath(root, names)?;
        let dir = checked_names(root, names)?.open_dir(parents)?;
        Ok(RootHandle::new(
            &joined(root, names),
            Some(OsStr::new(name)),
            Some(dir),
        ))
    }

    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<u8>> {
        let mut file = match &root.file {
            Some(_) if !names.is_empty() => return Err(beneath_a_file(names)),
            Some(name) => root_dir(root)?.open_file(&[name])?,
            None => checked_names(root, names)?.open_file(names)?,
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        let mut file = match &root.file {
            Some(_) if !names.is_empty() => return Err(beneath_a_file(names)),
            Some(name) => root_dir(root)?.create_file(&[name])?,
            None => checked_names(root, names)?.create_file(names)?,
        };
        file.write_all(contents)
    }

    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        checked_names(root, names)?.create_dir(names)
    }

    fn lock_beneath(&mut self, root: &RootHandle) -> io::Result<DirLock> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        Ok(DirLock::new(root_dir(root)?.lock()?))
    }

    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        let entries = checked_names(root, names)?.open_dir(names)?.entries()?;
        sorted_names(entries, &joined(root, names))
    }

    fn exists_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<bool> {
        let (dir, names) = target(root, names)?;
        dir.exists(&names)
    }

    fn metadata_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Meta> {
        let (dir, names) = target(root, names)?;
        dir.metadata(&names)
    }

    fn append_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        let (dir, names) = target(root, names)?;
        dir.append_file(&names)?.write_all(contents)
    }

    fn sync_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        let (dir, names) = target(root, names)?;
        dir.sync_file(&names)
    }

    fn sync_dir_beneath(&mut self, root: &RootHandle) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        root_dir(root)?.sync()
    }

    fn remove_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        checked_names(root, names)?.delete_file(names)
    }

    fn remove_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        checked_names(root, names)?.delete_dir(names)
    }

    fn rename_beneath(
        &mut self,
        root: &RootHandle,
        from: &[String],
        to: &[String],
    ) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        windows_names(from)?;
        checked_names(root, to)?.move_file(from, to)
    }

    fn printed(&self) -> Vec<String> {
        Vec::new()
    }

    fn net_connect(&mut self, host: &str, port: i128) -> io::Result<oska_sys::NetStream> {
        // A DNS name is resolved by `std.dns`, never by the system.
        if !oska_sys::NetStream::needs_resolving(host, port)? {
            return oska_sys::NetStream::connect(host, port);
        }
        let files = self.name_files.clone();
        let hooks = self.dns_test_hooks;
        match crate::resolve::resolve(self, files.as_ref(), hooks, host) {
            Ok(addresses) => oska_sys::NetStream::connect_resolved(host, &addresses, port),
            Err(err) => Err(io::Error::other(format!(
                "cannot connect to {host}:{port}: {err}"
            ))),
        }
    }

    fn net_listen_loopback(&mut self, port: i128) -> io::Result<oska_sys::NetListener> {
        oska_sys::NetListener::bind_loopback(port)
    }

    fn net_exchange_datagram(
        &mut self,
        host: &str,
        port: i128,
        message: &[u8],
    ) -> io::Result<Vec<u8>> {
        oska_sys::exchange_datagram(host, port, message)
    }

    fn net_name_servers(&mut self) -> io::Result<Vec<String>> {
        oska_sys::system_name_servers()
    }

    fn take_output(&mut self) -> Output {
        Output::default()
    }
}

/// The directory handle and the names to walk beneath it for an
/// operation on `names` beneath `root`: for a directory root, the root's
/// own handle and `names`; for a file root, which holds nothing beneath
/// it, the file's parent directory and the file's own name.
fn target<'r>(
    root: &'r RootHandle,
    names: &[String],
) -> io::Result<(&'r oska_sys::Dir, Vec<std::ffi::OsString>)> {
    match &root.file {
        Some(_) if !names.is_empty() => Err(beneath_a_file(names)),
        Some(name) => Ok((root_dir(root)?, vec![name.to_os_string()])),
        None => Ok((
            checked_names(root, names)?,
            names.iter().map(std::ffi::OsString::from).collect(),
        )),
    }
}

/// The OS handle of `root`, to walk `names` beneath, once each name is
/// known to be a file's own name on Windows (see [`windows_names`]). Every
/// `RealHost` method that walks names beneath a root gets its handle
/// here, so none can skip the check.
fn checked_names<'r>(root: &'r RootHandle, names: &[String]) -> io::Result<&'r oska_sys::Dir> {
    windows_names(names)?;
    root_dir(root)
}

/// The OS handle of `root`, which only a root [`FakeHost`] opened lacks.
/// Used alone only for a file root's own name, which came from trusted
/// text; names walked beneath a root go through [`checked_names`].
fn root_dir(root: &RootHandle) -> io::Result<&oska_sys::Dir> {
    root.os.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "this root was opened by another host",
        )
    })
}

/// A root's trusted `path` with each `/` a separator. On Windows `/` is
/// one in every path but a verbatim one (`\\?\C:\...`, as
/// `canonicalize` gives), where Windows would take it for part of a name,
/// so there it becomes `\`. Elsewhere `path` is as it is.
fn with_backslashes(path: &Path) -> PathBuf {
    match path.to_str() {
        Some(text) if cfg!(windows) => PathBuf::from(text.replace('/', "\\")),
        _ => path.to_path_buf(),
    }
}

/// `parent`, the leading part of a root's path, or the current directory
/// if it is empty.
fn or_here(parent: &Path) -> &Path {
    match parent.as_os_str().is_empty() {
        true => Path::new("."),
        false => parent,
    }
}

/// Refuse `names` unless each is a file's own name on Windows (see
/// [`windows_file_name`]). Elsewhere any plain name is one.
fn windows_names(names: &[String]) -> io::Result<()> {
    if cfg!(windows) {
        for name in names {
            windows_file_name(name)?;
        }
    }
    Ok(())
}

/// Refuse `name` unless it is a file's own name on Windows. A `:` opens
/// an alternate data stream, so it is not a file beneath the root. A
/// reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`,
/// `LPT1` to `LPT9`, and `CONIN$`, `CONOUT$`), with or without an
/// extension, is refused too. The walk's own open, relative to a
/// directory handle, would reach a plain file by that name, but Windows'
/// path functions, which other programs open files with, may take it for
/// the device, so such a file could be written and never read back or
/// removed by path. Refusing it keeps the names a program may use what
/// they were before the walk opened names beneath a handle (#175).
fn windows_file_name(name: &str) -> io::Result<()> {
    let refused = |what: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("`{name}` {what}, and confined access never opens one"),
        ))
    };
    if name.contains(':') {
        return refused("holds a `:`, which names an alternate data stream on Windows");
    }
    // Windows ignores the extension, and spaces before it.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let stem = stem.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            let mut chars = n.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        })
    };
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered("COM")
        || numbered("LPT");
    if device {
        return refused("is a Windows device name");
    }
    Ok(())
}

/// `line` without its trailing `\n` or `\r\n`.
/// The next line of the process's standard input, without its line
/// break, or `None` at end of input.
fn read_stdin_line() -> io::Result<Option<String>> {
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(without_line_break(line)))
}

fn without_line_break(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

/// The seed of every [`FakeHost`]'s generator.
pub const FAKE_SEED: u64 = 0x05EE_D0F0_5CA0;

/// Everything a [`FakeHost`] holds but its output and its generator: its
/// stdin, environment and filesystem, as [`FakeHost::world`] gives them.
/// A test sets up the same world on a real OS from it, as `oska-run`'s
/// differential test helpers do for the native backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeWorld {
    /// The lines waiting on stdin, without their line breaks.
    pub stdin: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Every file, by absolute path, with its bytes.
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    /// Directories that exist though no file lies under them.
    pub dirs: BTreeSet<PathBuf>,
    /// Symlinks, by their absolute path, to their target's.
    pub links: BTreeMap<PathBuf, PathBuf>,
}

/// A deterministic stand-in for the OS: it records output rather than
/// printing it. Backs this crate's tests and the language's test
/// stand-ins.
///
/// - Its filesystem is in memory: files by absolute path, with every
///   ancestor of a file a directory, the empty directories
///   [`FakeHost::with_dir`] adds, and the symlinks
///   [`FakeHost::with_symlink`] adds. Its working directory is `/`.
/// - It confines as a Unix [`RealHost`] does: a walk beneath a root
///   refuses every symlink, and a root's last component must not be one.
///   A root's own path follows them in every component but the last,
///   after resolving `.` and `..` on the path as written.
/// - Its clock is fixed at the Unix epoch.
/// - Its generator has a fixed seed, [`FAKE_SEED`].
/// - Its stdin and environment are what the builder methods give it.
#[derive(Debug)]
pub struct FakeHost {
    output: Output,
    /// Every write to stdout, without its line break.
    printed: Vec<String>,
    stdin: VecDeque<String>,
    env: BTreeMap<String, String>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    /// Directories that exist though no file lies under them.
    dirs: BTreeSet<PathBuf>,
    /// Symlinks, by their path, to their target's.
    links: BTreeMap<PathBuf, PathBuf>,
    random: SplitMix64,
    /// Where the program tried to connect or listen, which this host
    /// refused.
    net_attempts: Vec<String>,
    /// Every `sync` that succeeded, in order: `file ` or `dir ` and the
    /// path. This host loses nothing in a crash, so a sync changes
    /// nothing; the record is for a host that does (#847).
    syncs: Vec<String>,
    /// The directories whose lock is held, shared with each [`DirLock`]
    /// that releases its own on drop.
    locked: Arc<Mutex<BTreeSet<PathBuf>>>,
}

/// What a [`FakeHost`]'s lock holds: the directory, released on drop.
struct FakeLock {
    locked: Arc<Mutex<BTreeSet<PathBuf>>>,
    dir: PathBuf,
}

impl Drop for FakeLock {
    fn drop(&mut self) {
        self.locked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.dir);
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        FakeHost {
            output: Output::default(),
            printed: Vec::new(),
            stdin: VecDeque::new(),
            env: BTreeMap::new(),
            files: BTreeMap::new(),
            dirs: BTreeSet::new(),
            links: BTreeMap::new(),
            random: SplitMix64::new(FAKE_SEED),
            net_attempts: Vec::new(),
            syncs: Vec::new(),
            locked: Arc::default(),
        }
    }
}

impl FakeHost {
    pub fn new() -> Self {
        FakeHost::default()
    }

    /// A host holding `world`: its stdin, environment and filesystem.
    pub fn from_world(world: FakeWorld) -> Self {
        FakeHost {
            stdin: world.stdin.into_iter().collect(),
            env: world.env,
            files: world.files,
            dirs: world.dirs,
            links: world.links,
            ..FakeHost::default()
        }
    }

    /// The host with environment variable `name` set to `value`.
    pub fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env.insert(name.to_string(), value.to_string());
        self
    }

    /// The host with `text` waiting on stdin, one line per `\n`.
    pub fn with_stdin(mut self, text: &str) -> Self {
        self.stdin.extend(text.lines().map(str::to_string));
        self
    }

    /// The host with a file at `path`, relative to `/`, holding
    /// `contents`.
    pub fn with_file(self, path: &str, contents: &str) -> Self {
        self.with_file_bytes(path, contents.as_bytes())
    }

    /// The host with a file at `path`, relative to `/`, holding the bytes
    /// `contents`, which need not be UTF-8.
    pub fn with_file_bytes(mut self, path: &str, contents: &[u8]) -> Self {
        self.files
            .insert(normalize(Path::new(path)), contents.to_vec());
        self
    }

    /// The host with an empty directory at `path`, relative to `/`.
    pub fn with_dir(mut self, path: &str) -> Self {
        self.dirs.insert(normalize(Path::new(path)));
        self
    }

    /// The host with a symlink at `path` pointing to `target`, both
    /// relative to `/`. The directory holding it exists; `target` need
    /// not.
    pub fn with_symlink(mut self, path: &str, target: &str) -> Self {
        let path = normalize(Path::new(path));
        if let Some(parent) = path.parent() {
            self.dirs.insert(parent.to_path_buf());
        }
        self.links.insert(path, normalize(Path::new(target)));
        self
    }

    /// Every file under the directory `dir`, relative to `/`, by its path
    /// relative to `dir`, with `/` between components, and its text, in
    /// which a byte that is not UTF-8 shows as U+FFFD.
    pub fn files_under(&self, dir: &str) -> BTreeMap<String, String> {
        let dir = normalize(Path::new(dir));
        self.files
            .iter()
            .filter_map(|(path, text)| {
                let relative = path.strip_prefix(&dir).ok()?;
                let name = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                Some((name, String::from_utf8_lossy(text).into_owned()))
            })
            .collect()
    }

    /// What the host holds now: its stdin, environment and filesystem.
    pub fn world(&self) -> FakeWorld {
        FakeWorld {
            stdin: self.stdin.iter().cloned().collect(),
            env: self.env.clone(),
            files: self.files.clone(),
            dirs: self.dirs.clone(),
            links: self.links.clone(),
        }
    }

    /// The text of the file at `path`, relative to `/`, if there is one.
    /// `None` means no file at all, so a test can assert that nothing was
    /// written there.
    ///
    /// # Panics
    ///
    /// When the file's bytes are not UTF-8: read those with
    /// [`FakeHost::file_bytes`].
    pub fn file(&self, path: &str) -> Option<&str> {
        let bytes = self.file_bytes(path)?;
        match std::str::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => panic!("the file at {path} is not UTF-8: read it with `file_bytes`"),
        }
    }

    /// The bytes of the file at `path`, relative to `/`, if there is one.
    pub fn file_bytes(&self, path: &str) -> Option<&[u8]> {
        self.files
            .get(&normalize(Path::new(path)))
            .map(Vec::as_slice)
    }

    /// Take the lock on the directory `dir`: its lock file is made
    /// empty if missing, and left, as a real host leaves it. A directory
    /// or a symlink there is refused as a real host refuses it.
    fn fake_lock(&mut self, dir: PathBuf) -> io::Result<DirLock> {
        let file = dir.join(oska_sys::LOCK_NAME);
        if self.links.contains_key(&file) {
            return Err(fake_symlink(oska_sys::LOCK_NAME));
        }
        if self.is_dir(&file) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` is not a regular file", oska_sys::LOCK_NAME),
            ));
        }
        if !self.files.contains_key(&file) {
            self.put_file(file, &[])?;
        }
        let fresh = self
            .locked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(dir.clone());
        if !fresh {
            return Err(lock_held());
        }
        Ok(DirLock::new(FakeLock {
            locked: Arc::clone(&self.locked),
            dir,
        }))
    }

    /// A directory root at `path`, already normalized.
    fn root_at(&self, path: PathBuf) -> io::Result<RootHandle> {
        if self.files.contains_key(&path) {
            return Err(not_a_directory(&path));
        }
        if !self.is_dir(&path) {
            return Err(not_found(&path));
        }
        Ok(RootHandle::new(&path, None, None))
    }

    /// A file root at `path`, already normalized, whose parent must be a
    /// directory. The file itself need not exist.
    fn file_root_at(&self, path: PathBuf) -> io::Result<RootHandle> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(no_file_name(&path));
        };
        self.root_at(parent.to_path_buf())?;
        Ok(RootHandle::new(&path, Some(name), None))
    }

    /// The path of the file `root` reaches with `names` beneath it: the
    /// walk beneath a directory root, or for a file root, its own path,
    /// refused if it is now a symlink.
    fn walk_file(&self, root: &RootHandle, names: &[String]) -> io::Result<PathBuf> {
        match (&root.file, names.is_empty()) {
            (Some(_), false) => Err(beneath_a_file(names)),
            (Some(name), true) if self.links.contains_key(&*root.path) => {
                Err(fake_symlink(&name.to_string_lossy()))
            }
            (Some(_), true) => Ok(root.path.to_path_buf()),
            (None, _) => self.walk(root, names),
        }
    }

    fn is_dir(&self, path: &Path) -> bool {
        path == Path::new("/")
            || self.dirs.iter().any(|d| d.starts_with(path))
            || self.files.keys().any(|f| f.starts_with(path) && f != path)
    }

    /// `path`, normalized, with every symlink in it followed, as a root's
    /// own path is (its ancestors; the caller refuses a link at the end).
    fn follow(&self, path: &Path) -> io::Result<PathBuf> {
        let mut path = normalize(path);
        // As many links as Linux follows on one path.
        for _ in 0..40 {
            let link = path
                .ancestors()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .find_map(|at| Some((at, self.links.get(at)?)));
            let Some((at, target)) = link else {
                return Ok(path);
            };
            let rest = path.strip_prefix(at).unwrap_or(Path::new(""));
            path = normalize(&target.join(rest));
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("too many symlinks in `{}`", shown(&path)),
        ))
    }

    /// The path `names` reach beneath `root`, refused at the first that
    /// is a symlink, or, before the last, not a directory: the walk a
    /// Unix [`RealHost`] makes beneath the root's handle.
    fn walk(&self, root: &RootHandle, names: &[String]) -> io::Result<PathBuf> {
        let mut path = root.path.to_path_buf();
        for (walked, name) in names.iter().enumerate() {
            path.push(name);
            if self.links.contains_key(&path) {
                return Err(fake_symlink(&names[..=walked].join("/")));
            }
            if walked + 1 < names.len() && !self.is_dir(&path) {
                return Err(not_found(&path));
            }
        }
        Ok(path)
    }

    /// Write `contents` to the file at `path`, created if need be in its
    /// parent directory, which must exist.
    fn put_file(&mut self, path: PathBuf, contents: &[u8]) -> io::Result<()> {
        match path.parent() {
            Some(parent) if self.is_dir(parent) && !self.is_dir(&path) => {
                self.files.insert(path, contents.to_vec());
                Ok(())
            }
            _ => Err(not_found(&path)),
        }
    }

    /// Create the directory at `path`, in its parent directory, which
    /// must exist. A directory already there is kept; a file or a symlink
    /// there is an error.
    fn put_dir(&mut self, path: PathBuf) -> io::Result<()> {
        if self.files.contains_key(&path) || self.links.contains_key(&path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("a file is already at {}", shown(&path)),
            ));
        }
        if self.is_dir(&path) {
            return Ok(());
        }
        match path.parent() {
            Some(parent) if self.is_dir(parent) => {
                self.dirs.insert(path);
                Ok(())
            }
            _ => Err(not_found(&path)),
        }
    }

    /// Make `path`'s parent a directory that stays, as it is when its
    /// last entry goes: a directory holding nothing is still there.
    fn keep_parent(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.dirs.insert(parent.to_path_buf());
        }
    }

    /// Remove the regular file at `path`, reached by `names` beneath
    /// `root`, and give its bytes; refused when it is a directory or is
    /// not there.
    fn take_file(
        &mut self,
        path: &Path,
        root: &RootHandle,
        names: &[String],
    ) -> io::Result<Vec<u8>> {
        if !self.files.contains_key(path) {
            return Err(match self.is_dir(path) {
                true => not_a_regular_file(root, names),
                false => missing(root, names),
            });
        }
        self.keep_parent(path);
        Ok(self.files.remove(path).unwrap_or_default())
    }

    /// The names of the entries of the directory at `path`, sorted.
    fn entries(&self, path: &Path) -> io::Result<Vec<String>> {
        if !self.is_dir(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("not a directory: {}", shown(path)),
            ));
        }
        // A file's, empty directory's or symlink's first component below
        // `path` is an entry of it. `BTreeSet` sorts by the bytes of the
        // name, as `RealHost` does.
        let names: BTreeSet<String> = self
            .files
            .keys()
            .chain(&self.dirs)
            .chain(self.links.keys())
            .filter_map(|entry| entry.strip_prefix(path).ok())
            .filter_map(|rest| rest.components().next())
            .filter_map(|c| c.as_os_str().to_str().map(str::to_string))
            .collect();
        Ok(names.into_iter().collect())
    }
}

/// `path` against `/`, with `.` and `..` resolved by hand, on the path
/// as written.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(name) => out.push(name),
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out
}

/// The error for meeting a symlink at `path` on a walk beneath a root,
/// worded as `oska-sys` words it.
fn fake_symlink(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("`{path}` is a symlink, and confined access never follows one"),
    )
}

/// The error for a write or `create_dir` beneath a root given no name.
fn no_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no file name to open")
}

/// `path` as a message shows it. A [`FakeHost`] path is rooted at `/` on
/// every OS, but `Path::join` adds `\` on Windows, so there its
/// separators are shown as `/`. A real Windows path, which starts with a
/// drive or a `\\?\` prefix, is shown as it is.
fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    match path.components().next() {
        Some(Component::Prefix(_)) => text,
        _ if cfg!(windows) => text.replace('\\', "/"),
        _ => text,
    }
}

/// The error for a file root given a path with no file name to take.
fn no_file_name(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` does not end in a file name", shown(path)),
    )
}

/// The error for a directory root that is some other kind of file.
fn not_a_directory(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is not a directory", shown(path)),
    )
}

/// `names` beneath `root` as a message shows them: with `/` between, or a
/// file root's own name.
fn shown_names(root: &RootHandle, names: &[String]) -> String {
    match &root.file {
        Some(name) => name.to_string_lossy().into_owned(),
        None => names.join("/"),
    }
}

/// The error for a name that is not there, worded as `oska-sys` words it.
fn missing(root: &RootHandle, names: &[String]) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("`{}`: not found", shown_names(root, names)),
    )
}

/// The error for a name that is not a regular file, worded as `oska-sys`
/// words it.
fn not_a_regular_file(root: &RootHandle, names: &[String]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("`{}` is not a regular file", shown_names(root, names)),
    )
}

/// The error for a name that is not a directory, or is a symlink, worded
/// as `oska-sys` words it.
fn not_a_directory_or_symlink(root: &RootHandle, names: &[String]) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{}` is a symlink or not a directory, and confined access never follows a symlink",
            shown_names(root, names)
        ),
    )
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("no such file or directory: {}", shown(path)),
    )
}

impl Host for FakeHost {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        self.output.stdout.push_str(text);
        self.printed
            .push(text.strip_suffix('\n').unwrap_or(text).to_string());
        Ok(())
    }

    fn printed(&self) -> Vec<String> {
        self.printed.clone()
    }

    fn write_stderr(&mut self, text: &str) -> io::Result<()> {
        self.output.stderr.push_str(text);
        Ok(())
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        Ok(self.stdin.pop_front())
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn now_millis(&mut self) -> i128 {
        0
    }

    fn next_random(&mut self) -> u64 {
        self.random.next()
    }

    fn secure_bytes(&mut self, len: usize) -> io::Result<Vec<u8>> {
        Ok(generator_bytes(|| self.random.next(), len))
    }

    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        if path.as_os_str().is_empty() {
            return Err(not_found(path));
        }
        // Its ancestors are followed, and its last component must not be
        // a symlink, as on `RealHost`.
        let path = normalize(path);
        let path = match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => self.follow(parent)?.join(name),
            _ => path,
        };
        if self.links.contains_key(&path) {
            return Err(fake_symlink(&shown(&path)));
        }
        self.root_at(path)
    }

    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        if !narrows(root, names)? {
            return Ok(root.clone());
        }
        self.root_at(self.walk(root, names)?)
    }

    fn open_file_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        if path.as_os_str().is_empty() {
            return Err(not_found(path));
        }
        // Its ancestors are followed, as for a directory root. Its own
        // name is not checked: a use meets a symlink there.
        let path = normalize(path);
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(no_file_name(&path));
        };
        self.file_root_at(self.follow(parent)?.join(name))
    }

    fn open_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        let (name, parents) = file_beneath(root, names)?;
        // The parents are walked, so none is a symlink; the file's own
        // name is not checked.
        self.file_root_at(self.walk(root, parents)?.join(name))
    }

    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<u8>> {
        let path = self.walk_file(root, names)?;
        self.files
            .get(&path)
            .cloned()
            .ok_or_else(|| not_found(&path))
    }

    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        if !root.is_file() && names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk_file(root, names)?;
        self.put_file(path, contents)
    }

    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        if names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk(root, names)?;
        self.put_dir(path)
    }

    fn lock_beneath(&mut self, root: &RootHandle) -> io::Result<DirLock> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        self.fake_lock(root.path.to_path_buf())
    }

    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        let path = self.walk(root, names)?;
        self.entries(&path)
    }

    fn exists_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<bool> {
        let path = match self.walk_file(root, names) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(self.files.contains_key(&path) || self.is_dir(&path))
    }

    fn metadata_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Meta> {
        let path = self.walk_file(root, names)?;
        if let Some(bytes) = self.files.get(&path) {
            return Ok(Meta {
                kind: Kind::File,
                size: bytes.len() as u64,
            });
        }
        if self.is_dir(&path) && !root.is_file() {
            return Ok(Meta {
                kind: Kind::Directory,
                size: 0,
            });
        }
        Err(missing(root, names))
    }

    fn append_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        if !root.is_file() && names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk_file(root, names)?;
        if self.is_dir(&path) && !self.files.contains_key(&path) {
            return Err(not_a_regular_file(root, names));
        }
        match self.files.get_mut(&path) {
            Some(bytes) => {
                bytes.extend_from_slice(contents);
                Ok(())
            }
            None => self.put_file(path, contents),
        }
    }

    fn sync_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if !root.is_file() && names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk_file(root, names)?;
        if !self.files.contains_key(&path) {
            return Err(match self.is_dir(&path) {
                true => not_a_regular_file(root, names),
                false => missing(root, names),
            });
        }
        self.syncs.push(format!("file {}", path.display()));
        Ok(())
    }

    fn sync_dir_beneath(&mut self, root: &RootHandle) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        // The capability holds the directory open, so as on a real system
        // a sync still succeeds when the directory has since been removed.
        self.syncs.push(format!("dir {}", root.path.display()));
        Ok(())
    }

    fn remove_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        if names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk(root, names)?;
        self.take_file(&path, root, names).map(drop)
    }

    fn remove_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        if names.is_empty() {
            return Err(no_name());
        }
        let path = self.walk(root, names)?;
        if self.files.contains_key(&path) {
            return Err(not_a_directory_or_symlink(root, names));
        }
        if !self.is_dir(&path) {
            return Err(missing(root, names));
        }
        let inside = |entry: &PathBuf| entry != &path && entry.starts_with(&path);
        if self
            .files
            .keys()
            .chain(&self.dirs)
            .chain(self.links.keys())
            .any(inside)
        {
            return Err(io::Error::other(format!(
                "`{}`: directory not empty",
                names.join("/")
            )));
        }
        self.keep_parent(&path);
        self.dirs.remove(&path);
        Ok(())
    }

    fn rename_beneath(
        &mut self,
        root: &RootHandle,
        from: &[String],
        to: &[String],
    ) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        if from.is_empty() || to.is_empty() {
            return Err(no_name());
        }
        let from_path = self.walk(root, from)?;
        if !self.files.contains_key(&from_path) {
            return Err(match self.is_dir(&from_path) {
                true => not_a_regular_file(root, from),
                false => missing(root, from),
            });
        }
        let to_path = self.walk(root, to)?;
        if self.is_dir(&to_path) && !self.files.contains_key(&to_path) {
            return Err(not_a_regular_file(root, to));
        }
        if from_path == to_path {
            return Ok(());
        }
        let bytes = self.take_file(&from_path, root, from)?;
        self.put_file(to_path, &bytes)
    }

    fn net_connect(&mut self, host: &str, port: i128) -> io::Result<oska_sys::NetStream> {
        self.net_attempts.push(format!("{host}:{port}"));
        Err(no_network())
    }

    fn net_listen_loopback(&mut self, port: i128) -> io::Result<oska_sys::NetListener> {
        self.net_attempts.push(format!("loopback:{port}"));
        Err(no_network())
    }

    fn net_exchange_datagram(
        &mut self,
        host: &str,
        port: i128,
        _message: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.net_attempts.push(format!("{host}:{port}"));
        Err(no_network())
    }

    fn syncs(&self) -> Vec<String> {
        self.syncs.clone()
    }

    fn net_attempts(&self) -> Vec<String> {
        self.net_attempts.clone()
    }

    fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }
}
