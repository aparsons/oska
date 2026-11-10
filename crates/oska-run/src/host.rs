//! The [`Host`]: the interpreter's only way to reach the OS.
//!
//! Every intrinsic that touches the outside world goes through it, so a
//! program behaves the same on [`RealHost`] and [`FakeHost`] except for
//! what the world gives back.
//!
//! The host offers primitives only. Policy, such as confining a restricted
//! filesystem to its root, lives in the runtime (`confine.rs`), built on
//! [`Host::canonicalize`] and [`Host::is_symlink`], so it is the same on
//! every host.
//!
//! To add an operation: add a method to [`Host`], implement it on both
//! hosts (`FakeHost` deterministically, in memory), and call it from the
//! intrinsic that needs it (`intrinsics.rs`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
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

    /// `path` made absolute, with `.` and `..` resolved and every symlink
    /// followed, as `std::fs::canonicalize` does. Fails when `path` does
    /// not exist.
    fn canonicalize(&mut self, path: &Path) -> io::Result<PathBuf>;

    /// Whether `path` itself is a symlink, without following it. A
    /// dangling symlink is one.
    fn is_symlink(&mut self, path: &Path) -> bool;

    /// The text of the file at `path`.
    ///
    /// With `beneath`, the root of a restricted filesystem, `path` is one
    /// `confine.rs` resolved: the host opens its last component without
    /// following a symlink, and refuses the file unless the file it
    /// opened still lies under `beneath`.
    fn read_file(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<String>;

    /// Create the file at `path`, or replace its contents, with
    /// `contents`. `beneath` is as for [`Host::read_file`]: with it,
    /// nothing is written until the opened file is known to lie under it.
    fn write_file(&mut self, path: &Path, contents: &str, beneath: Option<&Path>)
        -> io::Result<()>;

    /// The names of the entries of the directory at `path`, sorted by
    /// their bytes, so the order is the same on every host. A name is
    /// the entry's own, not a path; a subdirectory and a symlink are
    /// listed by name, and nothing is followed. Fails when `path` is not
    /// a directory, or an entry's name is not valid UTF-8.
    ///
    /// `beneath` is as for [`Host::read_file`]: with it, `path` is one
    /// `confine.rs` resolved, and nothing is listed unless the directory
    /// still lies under it.
    fn list_dir(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<Vec<String>>;

    /// Every line written to stdout so far, without its line break: one
    /// per `println`, as `t.printed()` returns them. A host that writes
    /// straight to the real stream records none.
    fn printed(&self) -> Vec<String>;

    /// Hand over what the host has recorded of the program's output since
    /// the last call. A host that writes straight to the real streams
    /// records nothing.
    fn take_output(&mut self) -> Output;
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

    fn canonicalize(&mut self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn is_symlink(&mut self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }

    fn read_file(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<String> {
        let Some(root) = beneath else {
            return std::fs::read_to_string(path);
        };
        let mut file = open_beneath(path, root, OpenOptions::new().read(true))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    fn write_file(
        &mut self,
        path: &Path,
        contents: &str,
        beneath: Option<&Path>,
    ) -> io::Result<()> {
        let Some(root) = beneath else {
            return std::fs::write(path, contents);
        };
        // Not truncated on open: the file is emptied only once it is
        // known to lie under the root.
        let mut file = open_beneath(path, root, OpenOptions::new().write(true).create(true))?;
        file.set_len(0)?;
        file.write_all(contents.as_bytes())
    }

    fn list_dir(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<Vec<String>> {
        // The directory is checked before it is read and again after: a
        // symlink swapped in for it in between sends the listing
        // somewhere else, and that listing is discarded.
        let under = |path: &Path| match beneath {
            Some(root) => {
                let now = std::fs::canonicalize(path)?;
                if now.starts_with(root) {
                    Ok(())
                } else {
                    Err(moved(path))
                }
            }
            None => Ok(()),
        };
        under(path)?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let name = entry?.file_name();
            let name = name.into_string().map_err(|name| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{name:?} in `{}` is not valid UTF-8", path.display()),
                )
            })?;
            names.push(name);
        }
        under(path)?;
        names.sort();
        Ok(names)
    }

    fn printed(&self) -> Vec<String> {
        Vec::new()
    }

    fn take_output(&mut self) -> Output {
        Output::default()
    }
}

/// `O_NOFOLLOW`, from the platform's `fcntl.h`: `open` fails on a symlink
/// in the last component instead of following it. `None` where its value
/// is not known here; there the checks in `confine.rs` and the re-check
/// after opening still apply.
#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "riscv64",
        target_arch = "loongarch64",
        target_arch = "s390x"
    )
))]
const O_NOFOLLOW: Option<i32> = Some(0o400000);
#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    )
))]
const O_NOFOLLOW: Option<i32> = Some(0o100000);
#[cfg(any(target_os = "macos", target_os = "ios"))]
const O_NOFOLLOW: Option<i32> = Some(0x0100);
#[cfg(not(any(
    all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "riscv64",
            target_arch = "loongarch64",
            target_arch = "s390x",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "powerpc",
            target_arch = "powerpc64"
        )
    ),
    target_os = "macos",
    target_os = "ios"
)))]
const O_NOFOLLOW: Option<i32> = None;

/// Open `path`, which `confine.rs` resolved under `root`, with `options`:
/// its last component without following a symlink, and the opened file
/// checked to still lie under `root`, so a symlink swapped in after the
/// check is refused rather than followed.
fn open_beneath(path: &Path, root: &Path, options: &mut OpenOptions) -> io::Result<File> {
    #[cfg(unix)]
    if let Some(flag) = O_NOFOLLOW {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(flag);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(moved(path));
    }
    let file = options.open(path)?;
    let now = std::fs::canonicalize(path)?;
    if !now.starts_with(root) || !same_file(&file, &now)? {
        return Err(moved(path));
    }
    Ok(file)
}

/// Whether `file`, already open, is the file at `path`.
#[cfg(unix)]
fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (opened, named) = (file.metadata()?, std::fs::symlink_metadata(path)?);
    Ok(opened.dev() == named.dev() && opened.ino() == named.ino())
}

/// Whether `file`, already open, is the file at `path`: without inode
/// numbers, as near as `std` can tell.
#[cfg(not(unix))]
fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    let (opened, named) = (file.metadata()?, std::fs::symlink_metadata(path)?);
    Ok(!named.file_type().is_symlink() && opened.len() == named.len())
}

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
///   ancestor of a file a directory, and the empty directories
///   [`FakeHost::with_dir`] adds. It has no symlinks, and its working
///   directory is `/`.
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

    fn is_dir(&self, path: &Path) -> bool {
        path == Path::new("/")
            || self.dirs.iter().any(|d| d.starts_with(path))
            || self.files.keys().any(|f| f.starts_with(path) && f != path)
    }
}

/// `path` against `/`, with `.` and `..` resolved by hand: the fake
/// filesystem has no symlinks, so that is all canonicalizing does.
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

/// `path` normalized, refused unless it lies under `beneath`: the fake
/// filesystem has no symlinks, so nothing can move it there.
fn fake_beneath(path: &Path, beneath: Option<&Path>) -> io::Result<PathBuf> {
    let path = normalize(path);
    match beneath {
        Some(root) if !path.starts_with(root) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("`{}` is outside the restricted directory", path.display()),
        )),
        _ => Ok(path),
    }
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

    fn canonicalize(&mut self, path: &Path) -> io::Result<PathBuf> {
        if path.as_os_str().is_empty() {
            return Err(not_found(path));
        }
        let normal = normalize(path);
        if self.files.contains_key(&normal) || self.is_dir(&normal) {
            Ok(normal)
        } else {
            Err(not_found(path))
        }
    }

    fn is_symlink(&mut self, _: &Path) -> bool {
        false
    }

    fn read_file(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<String> {
        let path = fake_beneath(path, beneath)?;
        self.files
            .get(&path)
            .cloned()
            .ok_or_else(|| not_found(&path))
    }

    fn write_file(
        &mut self,
        path: &Path,
        contents: &str,
        beneath: Option<&Path>,
    ) -> io::Result<()> {
        let path = fake_beneath(path, beneath)?;
        match path.parent() {
            Some(parent) if self.is_dir(parent) && !self.is_dir(&path) => {
                self.files.insert(path, contents.to_string());
                Ok(())
            }
            _ => Err(not_found(&path)),
        }
    }

    fn list_dir(&mut self, path: &Path, beneath: Option<&Path>) -> io::Result<Vec<String>> {
        let path = fake_beneath(path, beneath)?;
        if !self.is_dir(&path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("not a directory: {}", path.display()),
            ));
        }
        // A file's or empty directory's first component below `path` is
        // an entry of it: a file if it is the last, else a directory.
        // `BTreeSet` sorts by the bytes of the name, as `RealHost` does.
        let names: BTreeSet<String> = self
            .files
            .keys()
            .chain(&self.dirs)
            .filter_map(|file| file.strip_prefix(&path).ok())
            .filter_map(|rest| rest.components().next())
            .filter_map(|c| c.as_os_str().to_str().map(str::to_string))
            .collect();
        Ok(names.into_iter().collect())
    }

    fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }
}
