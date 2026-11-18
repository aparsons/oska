//! The build cache: a per-user, content-addressed store of the results of
//! checking and compiling whole programs, so `oskac check`, `run`, `test`
//! and `build` reuse a result whose inputs are unchanged (spec #279).
//!
//! There are two kinds of entry. A **check entry** holds what `oskac
//! check` printed, on stdout and stderr, and its exit code. A **native
//! entry** holds an executable the native backend built for one target.
//! A hit prints exactly what a fresh run prints; cache activity shows only
//! in `oskac cache info`.
//!
//! **The key** is a SHA-256 over a length-prefixed encoding of: the kind
//! of result; every input of the program (each module's path and text,
//! the manifest, and a package's refused symlinks, or a single file's
//! path and text); the path the program was named by, which the output
//! prints; the target; every flag that changes the output (`--format`,
//! `--release`, run, test or build); and the SHA-256 of the running
//! `oskac` executable, so a development build never reuses another
//! build's entries. Hashing a large executable on every command would
//! cost more than a hit saves, so that digest is itself kept as an entry,
//! keyed by the executable's path, size and modification time and, on
//! Unix, its device, inode and change time, which a process cannot set.
//!
//! **Location.** `OSKA_CACHE_DIR`, else the platform's per-user cache
//! folder: `$XDG_CACHE_HOME/oska`, else `~/.cache/oska` on Linux;
//! `~/Library/Caches/oska` on macOS; `%LOCALAPPDATA%\oska\cache` on
//! Windows. Entries live in `v1/entries` beneath it.
//!
//! **Integrity.** An entry is written to a new temporary file in the
//! entries folder and renamed into place, so a reader never sees a partial
//! entry and no lock is needed. Each entry starts with the SHA-256 of its
//! payload, checked on every read: a mismatch is a miss, and the entry is
//! removed and rebuilt. Entries are opened through `oska_sys::Dir`, which
//! never follows a symbolic link (or on Windows a reparse point), and a
//! file with more than one link is refused. On Unix a cache folder not
//! owned by the current user, or writable by its group or by others, is
//! refused: `oskac` warns once and runs without the cache.
//!
//! **Eviction.** After each write the least recently used entries are
//! removed until the entries take at most the limit: 1 GiB, or
//! `OSKA_CACHE_LIMIT` bytes. A hit marks its entry used.
//!
//! **Off.** `--no-cache` on a command, or `OSKA_CACHE=off`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use oska_check::Package;
use oska_native::Target;
use oska_syntax::Diagnostic;

use crate::json::quote;
use crate::sha256::{self, Digest, Sha256};

/// The limit on the entries' total size when `OSKA_CACHE_LIMIT` sets none.
const DEFAULT_LIMIT: u64 = 1 << 30;

/// The first bytes of every entry: the format, then the payload's SHA-256.
const MAGIC: &[u8] = b"oska-cache 1\n";

/// The prefix of a temporary file being written.
const TEMPORARY: &str = ".tmp-";

/// A temporary file older than this was left by a process that died
/// before renaming it, and is removed.
const ABANDONED: Duration = Duration::from_secs(3600);

/// An open build cache.
pub struct Store {
    /// `v1/entries` beneath the cache folder.
    entries: PathBuf,
    /// A handle for `entries`, beneath which nothing is followed.
    dir: oska_sys::Dir,
    limit: u64,
}

/// Why there is no cache.
#[derive(Debug)]
pub enum Off {
    /// `--no-cache` or `OSKA_CACHE=off`.
    Disabled,
    /// No cache folder can be named: no home directory is set.
    Nowhere,
    /// The cache folder is not safe to use, or cannot be made: the reason,
    /// naming the folder.
    Refused(String),
}

/// The cache folder: `OSKA_CACHE_DIR`, else the platform's.
pub fn location() -> Option<PathBuf> {
    use crate::self_cmd::{clean, env_path, user_home};
    if let Some(dir) = env_path("OSKA_CACHE_DIR") {
        return Some(clean(dir));
    }
    let dir = if cfg!(windows) {
        env_path("LOCALAPPDATA").map(|d| d.join("oska").join("cache"))
    } else if cfg!(target_os = "macos") {
        user_home().map(|h| h.join("Library").join("Caches").join("oska"))
    } else {
        env_path("XDG_CACHE_HOME")
            .filter(|d| d.is_absolute())
            .or_else(|| user_home().map(|h| h.join(".cache")))
            .map(|d| d.join("oska"))
    };
    dir.map(clean)
}

/// Whether `OSKA_CACHE=off` turns the cache off.
fn disabled_by_environment() -> bool {
    std::env::var_os("OSKA_CACHE").is_some_and(|v| v == "off")
}

/// The cache for a command, or `None` when it is off: with `--no-cache`
/// (`no_cache`), `OSKA_CACHE=off`, no folder to put it in, or a folder
/// that is refused, after one warning on stderr that says why.
pub fn for_command(no_cache: bool) -> Option<Store> {
    if no_cache {
        return None;
    }
    match Store::open(true) {
        Ok(store) => Some(store),
        Err(Off::Refused(reason)) => {
            eprintln!("warning: running without the build cache: {reason}");
            None
        }
        Err(Off::Disabled | Off::Nowhere) => None,
    }
}

impl Store {
    /// Open the cache, making its folders when `create` says so, or
    /// say why there is none.
    fn open(create: bool) -> Result<Store, Off> {
        if disabled_by_environment() {
            return Err(Off::Disabled);
        }
        let root = location().ok_or(Off::Nowhere)?;
        Store::at(root, create).map_err(Off::Refused)
    }

    /// The cache in the folder `root`, which must be trusted. Its
    /// folders are made when `create` says so; otherwise a missing one
    /// is an error.
    fn at(root: PathBuf, create: bool) -> Result<Store, String> {
        let shown = root.display().to_string();
        if create {
            if let Some(parent) = root.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot make the folder `{}`: {e}", parent.display()))?;
            }
        }
        let v1 = root.join("v1");
        let entries = v1.join("entries");
        for folder in [&root, &v1, &entries] {
            if create {
                make_private_dir(folder)
                    .map_err(|e| format!("cannot make the folder `{}`: {e}", folder.display()))?;
            }
            trusted(folder)?;
        }
        let dir = oska_sys::Dir::open(&root)
            .and_then(|d| d.open_dir(&["v1", "entries"]))
            .map_err(|e| format!("cannot open the cache folder `{shown}`: {e}"))?;
        Ok(Store {
            entries,
            dir,
            limit: limit(),
        })
    }

    /// The payload of the entry `key`, if there is one and it is intact.
    /// A damaged entry is removed. A hit marks the entry used.
    fn get(&self, key: &Digest) -> Option<Vec<u8>> {
        let name = sha256::hex(key);
        let mut file = self.dir.open_file(&[&name]).ok()?;
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return None;
        }
        match intact(&bytes) {
            Some(payload) => {
                // For eviction; where the handle cannot set it, the entry
                // is evicted as if unused since it was written.
                let _ = file.set_modified(SystemTime::now());
                Some(payload.to_vec())
            }
            None => {
                drop(file);
                let _ = std::fs::remove_file(self.entries.join(&name));
                None
            }
        }
    }

    /// Store `payload` as the entry `key`, replacing any there, then
    /// evict. A failure only means the next run misses too.
    fn put(&self, key: &Digest, payload: &[u8]) {
        let name = sha256::hex(key);
        let temporary = format!("{TEMPORARY}{:016x}", crate::native::random());
        let written = self.dir.create_file(&[&temporary]).and_then(|mut file| {
            file.write_all(MAGIC)?;
            file.write_all(&sha256::digest(payload))?;
            file.write_all(payload)?;
            file.flush()
        });
        let renamed = written.and_then(|()| {
            std::fs::rename(self.entries.join(&temporary), self.entries.join(&name))
        });
        if renamed.is_err() {
            let _ = std::fs::remove_file(self.entries.join(&temporary));
            return;
        }
        self.evict();
    }

    /// Remove the least recently used entries until the entries take at
    /// most the limit, and any abandoned temporary file and anything that
    /// is not a regular file.
    fn evict(&self) {
        let now = SystemTime::now();
        let mut kept = Vec::new();
        let mut total = 0u64;
        for (name, meta) in self.listing() {
            let path = self.entries.join(&name);
            if !meta.file_type().is_file() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if name.to_string_lossy().starts_with(TEMPORARY) {
                if now.duration_since(used).unwrap_or_default() > ABANDONED {
                    let _ = std::fs::remove_file(&path);
                }
                continue;
            }
            total += meta.len();
            kept.push((used, meta.len(), path));
        }
        kept.sort();
        for (_, len, path) in kept {
            if total <= self.limit {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }

    /// Every entry of the entries folder and what it is, not followed.
    fn listing(&self) -> Vec<(std::ffi::OsString, std::fs::Metadata)> {
        self.dir
            .entries()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| {
                let meta = std::fs::symlink_metadata(self.entries.join(&name)).ok()?;
                Some((name, meta))
            })
            .collect()
    }

    /// Count a hit or a miss, for `oskac cache info`: one byte appended
    /// to `hits` or `misses`, which concurrent runs cannot tear. Anything
    /// there but a regular file is removed first, never followed.
    fn count(&self, hit: bool) {
        let path = self
            .entries
            .with_file_name(if hit { "hits" } else { "misses" });
        if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file())
            && std::fs::remove_file(&path).is_err()
        {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
        {
            let _ = file.write_all(b".");
        }
    }

    /// The number of hits or misses counted.
    fn counted(&self, hit: bool) -> u64 {
        let path = self
            .entries
            .with_file_name(if hit { "hits" } else { "misses" });
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|m| m.file_type().is_file())
            .map_or(0, |m| m.len())
    }

    /// A key for a result of `kind`, which every field added to it then
    /// narrows. `None` when the running `oskac` cannot be hashed.
    fn key(&self, kind: &str) -> Option<Key> {
        let mut key = Key::new("result");
        key.field(&self.executable_digest()?);
        key.field(kind.as_bytes());
        Some(key)
    }

    /// The SHA-256 of the running `oskac`, computed once per process, and
    /// kept in the cache for the next one.
    fn executable_digest(&self) -> Option<Digest> {
        static DIGEST: OnceLock<Option<Digest>> = OnceLock::new();
        *DIGEST.get_or_init(|| {
            let path = std::env::current_exe().ok()?.canonicalize().ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            let mut memo = Key::new("executable");
            memo.field(path.as_os_str().as_encoded_bytes());
            memo.field(&meta.len().to_le_bytes());
            memo.field(&time_bytes(meta.modified().ok()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                for value in [meta.dev(), meta.ino()] {
                    memo.field(&value.to_le_bytes());
                }
                memo.field(&meta.ctime().to_le_bytes());
                memo.field(&meta.ctime_nsec().to_le_bytes());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                memo.field(&meta.creation_time().to_le_bytes());
            }
            let memo = memo.finish();
            if let Some(digest) = self.get(&memo).and_then(|d| Digest::try_from(d).ok()) {
                return Some(digest);
            }
            let digest = file_digest(&path)?;
            self.put(&memo, &digest);
            Some(digest)
        })
    }
}

/// Make the folder `path` if it is missing: on Unix with mode 0700.
fn make_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        made => made,
    }
}

/// Whether the folder `path` may hold the cache: a folder, not a
/// symbolic link, and on Unix owned by this user and writable by no one
/// else. The error says why not.
fn trusted(path: &Path) -> Result<(), String> {
    let shown = path.display();
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot read the cache folder `{shown}`: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("the cache folder `{shown}` is a symbolic link"));
    }
    if !meta.is_dir() {
        return Err(format!("the cache folder `{shown}` is not a folder"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != oska_sys::effective_uid() {
            return Err(format!(
                "the cache folder `{shown}` is owned by another user"
            ));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(format!(
                "the cache folder `{shown}` is writable by its group or by others; make it \
                 private (chmod 700) or set OSKA_CACHE_DIR"
            ));
        }
    }
    Ok(())
}

/// The payload of the entry `bytes`, if its stored SHA-256 matches.
fn intact(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes.strip_prefix(MAGIC)?;
    if rest.len() < 32 {
        return None;
    }
    let (stored, payload) = rest.split_at(32);
    (sha256::digest(payload) == stored).then_some(payload)
}

/// The SHA-256 of the file at `path`.
fn file_digest(path: &Path) -> Option<Digest> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Some(sha.finish()),
            Ok(read) => sha.update(&buffer[..read]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// `time` as bytes for a key: seconds and nanoseconds since 1970.
fn time_bytes(time: Option<SystemTime>) -> [u8; 12] {
    let since = time
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .unwrap_or_default();
    let mut bytes = [0u8; 12];
    bytes[..8].copy_from_slice(&since.as_secs().to_le_bytes());
    bytes[8..].copy_from_slice(&since.subsec_nanos().to_le_bytes());
    bytes
}

/// The limit on the entries' total size: `OSKA_CACHE_LIMIT` bytes, or
/// 1 GiB when it is unset or not a whole number.
fn limit() -> u64 {
    std::env::var("OSKA_CACHE_LIMIT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_LIMIT)
}

/// A key being built: a SHA-256 over length-prefixed fields, so no two
/// sequences of fields encode alike.
struct Key(Sha256);

impl Key {
    fn new(purpose: &str) -> Key {
        let mut key = Key(Sha256::new());
        key.field(b"oska build cache 1");
        key.field(purpose.as_bytes());
        key
    }

    fn field(&mut self, bytes: &[u8]) {
        self.0.update(&(bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    fn finish(self) -> Digest {
        self.0.finish()
    }
}

/// The digest of a single file's inputs: the path it was named by, which
/// the output prints, and its text.
pub fn file_sources(path: &str, src: &str) -> Digest {
    let mut key = Key::new("file");
    key.field(path.as_bytes());
    key.field(src.as_bytes());
    key.finish()
}

/// The digest of a package's inputs: the directory it was named by, which
/// the output prints, its manifest, every module's name, path and text,
/// and the symlinks refused in it.
pub fn package_sources(dir: &str, package: &Package, refused: &[Diagnostic]) -> Digest {
    let mut key = Key::new("package");
    key.field(dir.as_bytes());
    match &package.manifest {
        Some(manifest) => {
            key.field(b"manifest");
            key.field(manifest.as_bytes());
        }
        None => key.field(b"no manifest"),
    }
    key.field(&(package.modules.len() as u64).to_le_bytes());
    for module in &package.modules {
        key.field(module.module.as_bytes());
        key.field(module.path.as_bytes());
        key.field(module.source.as_bytes());
    }
    key.field(&(refused.len() as u64).to_le_bytes());
    for diagnostic in refused {
        key.field(diagnostic.to_json().as_bytes());
    }
    key.finish()
}

/// What a command printed, and its exit code: a check entry's payload.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Printed {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

impl Printed {
    /// Print it, stderr first, as the command did, and end with its code.
    pub fn emit(self) -> ExitCode {
        eprint!("{}", self.stderr);
        crate::write_stdout(&self.stdout);
        ExitCode::from(self.code)
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![self.code];
        bytes.extend_from_slice(&(self.stdout.len() as u64).to_le_bytes());
        bytes.extend_from_slice(self.stdout.as_bytes());
        bytes.extend_from_slice(self.stderr.as_bytes());
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Printed> {
        let (&code, rest) = bytes.split_first()?;
        let len = u64::from_le_bytes(rest.get(..8)?.try_into().ok()?);
        let rest = &rest[8..];
        let len = usize::try_from(len).ok().filter(|&len| len <= rest.len())?;
        let (stdout, stderr) = rest.split_at(len);
        Some(Printed {
            stdout: String::from_utf8(stdout.to_vec()).ok()?,
            stderr: String::from_utf8(stderr.to_vec()).ok()?,
            code,
        })
    }
}

/// The build cache as one command on one program uses it: the store,
/// when the cache is on, and the digest of the program's inputs.
pub struct Cached<'s> {
    store: Option<&'s Store>,
    sources: Digest,
}

impl<'s> Cached<'s> {
    pub fn new(store: Option<&'s Store>, sources: Digest) -> Cached<'s> {
        Cached { store, sources }
    }

    /// The key of a result of `kind` with the output-affecting `flags`.
    fn key(&self, kind: &str, flags: &[&[u8]]) -> Option<(&'s Store, Digest)> {
        let store = self.store?;
        let mut key = store.key(kind)?;
        key.field(&self.sources);
        for flag in flags {
            key.field(flag);
        }
        Some((store, key.finish()))
    }

    /// What `oskac check` printed for this program with the
    /// output-affecting `flags`: from the cache, or from `check`, which
    /// is then stored.
    pub fn check(&self, flags: &[&[u8]], check: impl FnOnce() -> Printed) -> Printed {
        let Some((store, key)) = self.key("check", flags) else {
            return check();
        };
        if let Some(printed) = store.get(&key).and_then(|p| Printed::decode(&p)) {
            store.count(true);
            return printed;
        }
        store.count(false);
        let printed = check();
        store.put(&key, &printed.encode());
        printed
    }

    /// The executable the native backend builds for `target`, for `oskac
    /// <command>` (`run`, `test` or `build`), with `--format=json` or not:
    /// from the cache, or from `build`, which is then stored if it
    /// succeeds. A refusal is never stored.
    pub fn native(
        &self,
        command: &str,
        target: Target,
        json: bool,
        build: impl FnOnce() -> Result<Vec<u8>, Diagnostic>,
    ) -> Result<Vec<u8>, Diagnostic> {
        let format: &[u8] = if json { b"json" } else { b"text" };
        let flags = [command.as_bytes(), target.name().as_bytes(), format];
        let Some((store, key)) = self.key("native", &flags) else {
            return build();
        };
        if let Some(bytes) = store.get(&key) {
            store.count(true);
            return Ok(bytes);
        }
        store.count(false);
        let built = build()?;
        store.put(&key, &built);
        Ok(built)
    }
}

// ---- oskac cache ---------------------------------------------------------------

/// What `oskac cache info` reports.
struct Info {
    location: Option<PathBuf>,
    /// Why the cache is not used, or `None` when it is.
    off: Option<String>,
    entries: u64,
    size: u64,
    limit: u64,
    hits: u64,
    misses: u64,
}

/// `oskac cache info`: the cache's location, whether it is used, its
/// entries, their size, the limit, and the hits and misses counted since
/// it was last cleaned. It makes nothing.
pub fn info(json: bool) -> ExitCode {
    let location = location();
    let mut info = Info {
        location: location.clone(),
        off: None,
        entries: 0,
        size: 0,
        limit: limit(),
        hits: 0,
        misses: 0,
    };
    if disabled_by_environment() {
        info.off = Some("OSKA_CACHE=off".to_string());
    }
    match &location {
        None => {
            info.off = Some("no cache folder: set OSKA_CACHE_DIR, or a home directory".to_string());
        }
        Some(root) if root.exists() => match Store::at(root.clone(), false) {
            Ok(store) => {
                for (name, meta) in store.listing() {
                    if meta.file_type().is_file() && !name.to_string_lossy().starts_with(TEMPORARY)
                    {
                        info.entries += 1;
                        info.size += meta.len();
                    }
                }
                info.hits = store.counted(true);
                info.misses = store.counted(false);
            }
            Err(reason) => info.off = info.off.or(Some(reason)),
        },
        Some(_) => {}
    }
    if json {
        outln!(
            "{{\"location\":{},\"used\":{},\"reason\":{},\"entries\":{},\"size\":{},\"limit\":{},\"hits\":{},\"misses\":{}}}",
            info.location
                .as_deref()
                .map_or_else(|| "null".to_string(), |p| quote(&p.display().to_string())),
            info.off.is_none(),
            info.off.as_deref().map_or_else(|| "null".to_string(), quote),
            info.entries,
            info.size,
            info.limit,
            info.hits,
            info.misses,
        );
    } else {
        let location = info
            .location
            .as_deref()
            .map_or_else(|| "none".to_string(), |p| p.display().to_string());
        outln!("location: {location}");
        match &info.off {
            None => outln!("used: yes"),
            Some(reason) => outln!("used: no, {reason}"),
        }
        outln!("entries: {}", info.entries);
        outln!("size: {} bytes", info.size);
        outln!("limit: {} bytes", info.limit);
        outln!("hits: {}", info.hits);
        outln!("misses: {}", info.misses);
    }
    ExitCode::SUCCESS
}

/// `oskac cache clean`: remove every entry and the counts. A cache folder
/// that is refused is left alone, with exit code 1.
pub fn clean(json: bool) -> ExitCode {
    let Some(root) = location() else {
        return fail(
            "no cache folder: set OSKA_CACHE_DIR, or a home directory",
            json,
        );
    };
    let mut removed = 0u64;
    let mut bytes = 0u64;
    if root.exists() {
        let store = match Store::at(root.clone(), false) {
            Ok(store) => store,
            Err(reason) => return fail(&reason, json),
        };
        for (name, meta) in store.listing() {
            if std::fs::remove_file(store.entries.join(&name)).is_ok()
                && meta.file_type().is_file()
                && !name.to_string_lossy().starts_with(TEMPORARY)
            {
                removed += 1;
                bytes += meta.len();
            }
        }
        for count in ["hits", "misses"] {
            let path = store.entries.with_file_name(count);
            if std::fs::symlink_metadata(&path).is_ok() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let shown = root.display().to_string();
    if json {
        outln!(
            "{{\"location\":{},\"removed\":{removed},\"bytes\":{bytes}}}",
            quote(&shown)
        );
    } else {
        outln!("removed {removed} entries ({bytes} bytes) from {shown}");
    }
    ExitCode::SUCCESS
}

/// Report that `oskac cache` cannot do its work, and end with 1.
fn fail(reason: &str, json: bool) -> ExitCode {
    if json {
        outln!("{{\"error\":{}}}", quote(reason));
    } else {
        eprintln!("error: {reason}");
    }
    ExitCode::FAILURE
}

/// The usage of `oskac cache`, as a usage error.
pub fn usage() -> ExitCode {
    eprintln!("usage: oskac cache info [--format=json] | oskac cache clean [--format=json]");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printed_result_round_trips() {
        let printed = Printed {
            stdout: "{\"diagnostics\":[]}\n".to_string(),
            stderr: "f.osk:1:1: error\n".to_string(),
            code: 1,
        };
        assert_eq!(Printed::decode(&printed.encode()), Some(printed));
        assert_eq!(Printed::decode(&[1, 9]), None, "a truncated payload");
    }

    #[test]
    fn an_entry_is_intact_only_with_its_own_digest() {
        let payload = b"executable";
        let mut entry = MAGIC.to_vec();
        entry.extend_from_slice(&sha256::digest(payload));
        entry.extend_from_slice(payload);
        assert_eq!(intact(&entry), Some(&payload[..]));
        let last = entry.len() - 1;
        entry[last] ^= 1;
        assert_eq!(intact(&entry), None, "a changed byte");
        assert_eq!(intact(&entry[..MAGIC.len() + 8]), None, "a truncated entry");
    }

    #[test]
    fn fields_are_length_prefixed() {
        let key = |fields: &[&str]| {
            let mut key = Key::new("test");
            for field in fields {
                key.field(field.as_bytes());
            }
            key.finish()
        };
        assert_ne!(key(&["ab", "c"]), key(&["a", "bc"]));
    }
}
