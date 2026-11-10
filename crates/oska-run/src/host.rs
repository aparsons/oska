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

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead, Write};
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
pub trait Host {
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
    fn read_file(&mut self, path: &Path) -> io::Result<String>;

    /// Create the file at `path`, or replace its contents, with
    /// `contents`.
    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()>;

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

    fn read_file(&mut self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn printed(&self) -> Vec<String> {
        Vec::new()
    }

    fn take_output(&mut self) -> Output {
        Output::default()
    }
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
///   ancestor of a file a directory. It has no symlinks, and its working
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

    /// The text of the file at `path`, relative to `/`, if there is one.
    pub fn file(&self, path: &str) -> Option<&str> {
        self.files
            .get(&normalize(Path::new(path)))
            .map(String::as_str)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path == Path::new("/") || self.files.keys().any(|f| f.starts_with(path) && f != path)
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

    fn read_file(&mut self, path: &Path) -> io::Result<String> {
        self.files
            .get(&normalize(path))
            .cloned()
            .ok_or_else(|| not_found(path))
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        let path = normalize(path);
        match path.parent() {
            Some(parent) if self.is_dir(parent) && !self.is_dir(&path) => {
                self.files.insert(path, contents.to_string());
                Ok(())
            }
            _ => Err(not_found(&path)),
        }
    }

    fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }
}
