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
#[cfg(not(unix))]
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::prng::SplitMix64;

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

    /// The environment variable `name`, if it is set to valid UTF-8.
    fn env_var(&mut self, name: &str) -> Option<String>;

    /// The current time, in milliseconds since the Unix epoch.
    fn now_millis(&mut self) -> i128;

    /// The next pseudo-random 64 bits.
    fn next_random(&mut self) -> u64;

    /// Open the directory or regular file at `path` as the root of a
    /// restricted filesystem. Its last component must not be a symlink;
    /// earlier ones are followed, since the root is trusted text.
    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle>;

    /// The directory or regular file reached by walking `names` beneath
    /// `root`, as a root of its own: each name a plain component, which
    /// `confine.rs` produced, and none a symlink. No names gives `root`.
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle>;

    /// The text of the regular file reached by walking `names` beneath
    /// `root`, none of them a symlink. When `root` is itself a file,
    /// `names` is empty.
    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<String>;

    /// The text of the file at `path`, read through an unrestricted
    /// filesystem: symlinks are followed.
    fn read_file(&mut self, path: &Path) -> io::Result<String>;

    /// Create the regular file reached by walking `names` beneath `root`,
    /// or replace its contents, with `contents`. No name is a symlink,
    /// the last included, so nothing is created through one. When `root`
    /// is itself a file, `names` is empty.
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &str,
    ) -> io::Result<()>;

    /// Create the file at `path`, or replace its contents, with
    /// `contents`, through an unrestricted filesystem: symlinks are
    /// followed.
    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()>;

    /// Create the directory reached by walking `names` beneath `root`,
    /// none of them a symlink: one name at least, and every name before
    /// the last a directory that exists. A directory already there is
    /// kept; anything else there, a symlink included, is an error.
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()>;

    /// Create the directory at `path`, whose parent must exist, through
    /// an unrestricted filesystem. A directory already there is kept;
    /// anything else there is an error.
    fn create_dir(&mut self, path: &Path) -> io::Result<()>;

    /// The names of the entries of the directory reached by walking
    /// `names` beneath `root`, none of them a symlink, listed as
    /// [`Host::list_dir`] lists them. No names lists `root`.
    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>>;

    /// The names of the entries of the directory at `path`, sorted by
    /// their bytes, so the order is the same on every host. A name is
    /// the entry's own, not a path; a subdirectory and a symlink are
    /// listed by name, and no entry is followed. Fails when `path` is not
    /// a directory, or an entry's name is not valid UTF-8.
    fn list_dir(&mut self, path: &Path) -> io::Result<Vec<String>>;

    /// Every line written to stdout so far, without its line break: one
    /// per `println`, as `t.printed()` returns them. A host that writes
    /// straight to the real stream records none.
    fn printed(&self) -> Vec<String>;

    /// Hand over what the host has recorded of the program's output since
    /// the last call. A host that writes straight to the real streams
    /// records nothing.
    fn take_output(&mut self) -> Output;
}

/// The root of a restricted filesystem, as a host opened it: a directory,
/// or a single regular file. Copies share it, and the last one dropped
/// releases what the host holds for it.
///
/// On a Unix [`RealHost`] it holds an OS handle for the directory, or for
/// a file's parent directory together with the file's name, and every
/// read beneath it walks from that handle (`oska-sys`). Elsewhere, and on
/// [`FakeHost`], it holds only the root's path.
#[derive(Debug, Clone)]
pub struct RootHandle {
    /// The root's canonical path, which the path-based operations
    /// resolve under.
    path: Rc<Path>,
    /// The file's own name, when the root is a file.
    file: Option<Rc<OsStr>>,
    /// The directory's handle, or the file's parent's. Always `None` off
    /// Unix, where `OsDir` has no values.
    #[cfg_attr(not(unix), allow(dead_code))]
    os: Option<Rc<OsDir>>,
}

#[cfg(unix)]
type OsDir = oska_sys::Dir;
/// No OS handle: a root holds only its path.
#[cfg(not(unix))]
type OsDir = std::convert::Infallible;

impl RootHandle {
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
        format!(
            "`{}` is a single file, not a directory",
            root.path.display()
        ),
    )
}

/// The error for writing to, or creating, a root that is a directory.
#[cfg(not(unix))]
fn a_dir_root(root: &RootHandle) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "`{}` is the restricted directory itself",
            root.path.display()
        ),
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
#[cfg(unix)]
fn joined(root: &RootHandle, names: &[String]) -> PathBuf {
    names
        .iter()
        .fold(root.path.to_path_buf(), |path, name| path.join(name))
}

/// The real OS: backs `oskac run` and integration tests.
#[derive(Debug)]
pub struct RealHost {
    random: SplitMix64,
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
        }
    }
}

impl Host for RealHost {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        io::stdout().write_all(text.as_bytes())
    }

    fn write_stderr(&mut self, text: &str) -> io::Result<()> {
        io::stderr().write_all(text.as_bytes())
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(without_line_break(line)))
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

    #[cfg(unix)]
    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        // Without a trailing `/` or `.`, which would follow a symlink in
        // the last component.
        let path: PathBuf = path.components().collect();
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            // `/`, or a path ending in `..`: no symlink to refuse.
            let dir = oska_sys::Dir::open(&path)?;
            return Ok(real_root(std::fs::canonicalize(&path)?, None, dir));
        };
        let parent = match parent.as_os_str().is_empty() {
            true => Path::new("."),
            false => parent,
        };
        let parent = oska_sys::Dir::open(parent)?;
        let (file, dir) = match parent.open_dir(&[name]) {
            Ok(dir) => (None, dir),
            Err(_) => {
                parent.open_file(&[name])?;
                (Some(name), parent)
            }
        };
        Ok(real_root(std::fs::canonicalize(&path)?, file, dir))
    }

    #[cfg(unix)]
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        if names.is_empty() {
            return Ok(root.clone());
        }
        if root.is_file() {
            return Err(beneath_a_file(names));
        }
        let dir = root_dir(root)?;
        // Canonical as it stands: the walk met no symlink. Not
        // canonicalized again, which would follow one swapped in since.
        let path = joined(root, names);
        match dir.open_dir(names) {
            Ok(dir) => Ok(real_root(path, None, dir)),
            Err(error) => {
                let (name, parents) = names.split_last().ok_or(error)?;
                let parent = dir.open_dir(parents)?;
                parent.open_file(&[name])?;
                Ok(real_root(path, Some(OsStr::new(name)), parent))
            }
        }
    }

    #[cfg(unix)]
    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<String> {
        let dir = root_dir(root)?;
        let mut file = match &root.file {
            Some(_) if !names.is_empty() => return Err(beneath_a_file(names)),
            Some(name) => dir.open_file(&[name])?,
            None => dir.open_file(names)?,
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    #[cfg(unix)]
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &str,
    ) -> io::Result<()> {
        let dir = root_dir(root)?;
        let mut file = match &root.file {
            Some(_) if !names.is_empty() => return Err(beneath_a_file(names)),
            Some(name) => dir.create_file(&[name])?,
            None => dir.create_file(names)?,
        };
        file.write_all(contents.as_bytes())
    }

    #[cfg(unix)]
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        root_dir(root)?.create_dir(names)
    }

    #[cfg(unix)]
    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        let entries = root_dir(root)?.open_dir(names)?.entries()?;
        sorted_names(entries, &joined(root, names))
    }

    #[cfg(not(unix))]
    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        let path: PathBuf = path.components().collect();
        let there = std::fs::symlink_metadata(&path)?;
        if there.file_type().is_symlink() {
            return Err(moved(&path));
        }
        let file = match there.is_file() {
            true => path.file_name().map(Rc::from),
            false => None,
        };
        Ok(RootHandle {
            path: Rc::from(std::fs::canonicalize(&path)?.as_path()),
            file,
            os: None,
        })
    }

    #[cfg(not(unix))]
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        if names.is_empty() {
            return Ok(root.clone());
        }
        if root.is_file() {
            return Err(beneath_a_file(names));
        }
        let path = checked_beneath(root, names)?;
        let file = match std::fs::metadata(&path)?.is_file() {
            true => path.file_name().map(Rc::from),
            false => None,
        };
        Ok(RootHandle {
            path: Rc::from(path.as_path()),
            file,
            os: None,
        })
    }

    #[cfg(not(unix))]
    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<String> {
        if root.is_file() && !names.is_empty() {
            return Err(beneath_a_file(names));
        }
        let path = checked_beneath(root, names)?;
        let mut file = open_beneath(&path, &root.path, OpenOptions::new().read(true))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    #[cfg(not(unix))]
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &str,
    ) -> io::Result<()> {
        let path = match (root.is_file(), names.split_last()) {
            (true, None) => root.path.to_path_buf(),
            (true, Some(_)) => return Err(beneath_a_file(names)),
            (false, Some((name, parents))) => checked_beneath(root, parents)?.join(name),
            (false, None) => return Err(a_dir_root(root)),
        };
        // Not truncated on open: the file is emptied only once it is
        // known to lie under the root.
        let mut file = open_beneath(
            &path,
            &root.path,
            OpenOptions::new().write(true).create(true),
        )?;
        file.set_len(0)?;
        file.write_all(contents.as_bytes())
    }

    #[cfg(not(unix))]
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        let (name, parents) = names.split_last().ok_or_else(|| a_dir_root(root))?;
        let path = checked_beneath(root, parents)?.join(name);
        // `mkdir` never follows a symlink in the last component: one
        // there, dangling or not, makes it fail with `AlreadyExists`.
        if let Err(error) = std::fs::create_dir(&path) {
            if error.kind() != io::ErrorKind::AlreadyExists
                || !std::fs::symlink_metadata(&path)?.is_dir()
            {
                return Err(error);
            }
        }
        // A symlink swapped in for a directory on the way may have sent
        // the `mkdir` elsewhere: refuse to report success for it.
        resolved_beneath(&path, &root.path).map(drop)
    }

    #[cfg(not(unix))]
    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        // Checked before it is read and again after: a symlink swapped in
        // for it in between sends the listing elsewhere, and that listing
        // is discarded.
        let path = checked_beneath(root, names)?;
        let names = self.list_dir(&path)?;
        resolved_beneath(&path, &root.path)?;
        Ok(names)
    }

    fn read_file(&mut self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        match std::fs::create_dir(path) {
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && std::fs::metadata(path)?.is_dir() =>
            {
                Ok(())
            }
            made => made,
        }
    }

    fn list_dir(&mut self, path: &Path) -> io::Result<Vec<String>> {
        let entries = std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<_>>()?;
        sorted_names(entries, path)
    }

    fn printed(&self) -> Vec<String> {
        Vec::new()
    }

    fn take_output(&mut self) -> Output {
        Output::default()
    }
}

/// A root at the canonical `path`, opened as `dir`: the directory
/// itself, or with `file`, the parent of the file of that name.
#[cfg(unix)]
fn real_root(path: PathBuf, file: Option<&OsStr>, dir: oska_sys::Dir) -> RootHandle {
    RootHandle {
        path: Rc::from(path.as_path()),
        file: file.map(Rc::from),
        os: Some(Rc::new(dir)),
    }
}

/// The OS handle of `root`, which only a root [`FakeHost`] opened lacks.
#[cfg(unix)]
fn root_dir(root: &RootHandle) -> io::Result<&oska_sys::Dir> {
    root.os.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "this root was opened by another host",
        )
    })
}

/// Where Windows has no handle-relative open: the path `names` reach
/// beneath `root`, refused when any component is a symlink or the path
/// canonicalizes outside the root. A narrowed check, not a race-free one
/// (`docs/runtime.md`).
#[cfg(not(unix))]
fn checked_beneath(root: &RootHandle, names: &[String]) -> io::Result<PathBuf> {
    let mut path = root.path.to_path_buf();
    for name in names {
        path.push(name);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(moved(&path));
        }
    }
    resolved_beneath(&path, &root.path)
}

/// Where Windows has no handle-relative open: open `path`, which
/// [`checked_beneath`] produced, with `options`. A symlink there is
/// refused, and the opened file is checked to still be the one at the
/// path, under `root`, which narrows the race but does not close it.
#[cfg(not(unix))]
fn open_beneath(path: &Path, root: &Path, options: &mut OpenOptions) -> io::Result<File> {
    if std::fs::symlink_metadata(path).is_ok_and(|there| there.file_type().is_symlink()) {
        return Err(moved(path));
    }
    let file = options.open(path)?;
    let now = resolved_beneath(path, root)?;
    let (opened, named) = (file.metadata()?, std::fs::symlink_metadata(&now)?);
    // Without inode numbers, as near as `std` can tell.
    if named.file_type().is_symlink() || opened.len() != named.len() {
        return Err(moved(path));
    }
    Ok(file)
}

/// `path` with every symlink resolved, refused unless it lies under
/// `root`: Windows' check before and after it touches the filesystem.
#[cfg(not(unix))]
fn resolved_beneath(path: &Path, root: &Path) -> io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)?;
    if resolved.starts_with(root) {
        Ok(resolved)
    } else {
        Err(moved(path))
    }
}

#[cfg(not(unix))]
fn moved(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{}` is a symlink or left the restricted directory while it was being opened",
            path.display()
        ),
    )
}

/// `line` without its trailing `\n` or `\r\n`.
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
///   An unrestricted filesystem follows them, after resolving `.` and
///   `..` on the path as written.
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
    files: BTreeMap<PathBuf, String>,
    /// Directories that exist though no file lies under them.
    dirs: BTreeSet<PathBuf>,
    /// Symlinks, by their path, to their target's.
    links: BTreeMap<PathBuf, PathBuf>,
    random: SplitMix64,
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
        }
    }
}

impl FakeHost {
    pub fn new() -> Self {
        FakeHost::default()
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
    pub fn with_file(mut self, path: &str, contents: &str) -> Self {
        self.files
            .insert(normalize(Path::new(path)), contents.to_string());
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
    /// relative to `dir`, with `/` between components.
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
                Some((name, text.clone()))
            })
            .collect()
    }

    /// The text of the file at `path`, relative to `/`, if there is one.
    pub fn file(&self, path: &str) -> Option<&str> {
        self.files
            .get(&normalize(Path::new(path)))
            .map(String::as_str)
    }

    /// A root at `path`, already normalized: a file or a directory.
    fn root_at(&self, path: PathBuf) -> io::Result<RootHandle> {
        let file = match self.files.contains_key(&path) {
            true => path.file_name().map(Rc::from),
            false if self.is_dir(&path) => None,
            false => return Err(not_found(&path)),
        };
        Ok(RootHandle {
            path: Rc::from(path.as_path()),
            file,
            os: None,
        })
    }

    fn is_dir(&self, path: &Path) -> bool {
        path == Path::new("/")
            || self.dirs.iter().any(|d| d.starts_with(path))
            || self.files.keys().any(|f| f.starts_with(path) && f != path)
    }

    /// `path`, normalized, with every symlink in it followed, as an open
    /// through an unrestricted filesystem follows them.
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
            format!("too many symlinks in `{}`", path.display()),
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
    fn put_file(&mut self, path: PathBuf, contents: &str) -> io::Result<()> {
        match path.parent() {
            Some(parent) if self.is_dir(parent) && !self.is_dir(&path) => {
                self.files.insert(path, contents.to_string());
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
                format!("a file is already at {}", path.display()),
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

    /// The names of the entries of the directory at `path`, sorted.
    fn entries(&self, path: &Path) -> io::Result<Vec<String>> {
        if !self.is_dir(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("not a directory: {}", path.display()),
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
fn no_fake_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no file name to open")
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("no such file or directory: {}", path.display()),
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
            return Err(fake_symlink(&path.display().to_string()));
        }
        self.root_at(path)
    }

    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        if names.is_empty() {
            return Ok(root.clone());
        }
        if root.is_file() {
            return Err(beneath_a_file(names));
        }
        self.root_at(self.walk(root, names)?)
    }

    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<String> {
        if root.is_file() && !names.is_empty() {
            return Err(beneath_a_file(names));
        }
        let path = self.walk(root, names)?;
        self.files
            .get(&path)
            .cloned()
            .ok_or_else(|| not_found(&path))
    }

    fn read_file(&mut self, path: &Path) -> io::Result<String> {
        let path = self.follow(path)?;
        self.files
            .get(&path)
            .cloned()
            .ok_or_else(|| not_found(&path))
    }

    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &str,
    ) -> io::Result<()> {
        match (root.is_file(), names.is_empty()) {
            (true, false) => return Err(beneath_a_file(names)),
            (false, true) => return Err(no_fake_name()),
            _ => {}
        }
        let path = self.walk(root, names)?;
        self.put_file(path, contents)
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        let path = self.follow(path)?;
        self.put_file(path, contents)
    }

    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        if names.is_empty() {
            return Err(no_fake_name());
        }
        let path = self.walk(root, names)?;
        self.put_dir(path)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        let path = self.follow(path)?;
        self.put_dir(path)
    }

    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        if root.is_file() {
            return Err(a_file_root(root));
        }
        let path = self.walk(root, names)?;
        self.entries(&path)
    }

    fn list_dir(&mut self, path: &Path) -> io::Result<Vec<String>> {
        let path = self.follow(path)?;
        self.entries(&path)
    }

    fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }
}
