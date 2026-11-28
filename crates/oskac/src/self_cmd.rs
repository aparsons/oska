// SPDX-License-Identifier: Apache-2.0
//! `oskac self info` and `oskac self uninstall`: ask this `oskac` about its
//! own installation, and remove it, without the installer. Both read the
//! install receipt (`receipt.json` in the Oska home) that the installers
//! write, and neither makes a network connection.
//!
//! `uninstall` applies the receipt rules of `docs/guide/install-receipt.md`
//! in-process, as the shared fixtures in `scripts/install/receipt-fixtures`
//! check (`tests/receipt_fixtures.rs`): every entry of the receipt is checked
//! against the places this platform's installer writes its kind before
//! anything is removed, and one that is not refuses the whole uninstall. It
//! removes this `oskac`, the `version` file and the receipt (only when this
//! `oskac` is the install the receipt records) and the `--hook` files (only
//! when unchanged), and leaves the installer's PATH edits to the installer's
//! own `--uninstall`, reporting them in `path_hint`. It never touches
//! `~/.config/oska`. `oskac` checks no SHA-256 here: it identifies its install
//! by path and version, the `version` file by its text, and a hook file by
//! comparing it with the published file built into it. Anything else it
//! cannot show unchanged, it keeps.

use crate::json::{quote, quote_option, Json};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The files `install.sh --hook` installs, as this release publishes them:
/// the settings with the post-edit hook alone, and the shell script that
/// earlier installers wrote and receipts may still list.
const HOOK_SCRIPT: &str = include_str!("../../../site/agent-setup/oska-check.sh");
const HOOK_SETTINGS: &str =
    include_str!("../../../site/agent-setup/claude-settings-post-edit.json");

/// The marker comment `install.sh --add-to-path` ends its profile line with.
const PROFILE_MARK: &str = "# added by the Oska installer";

/// The shell profiles `install.sh --add-to-path` picks from, directly in
/// the user's home.
const PROFILES: [&str; 4] = [".zshrc", ".bashrc", ".bash_profile", ".profile"];

/// The platform name release assets use, as the installers record it.
pub(crate) fn platform() -> String {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        std::env::consts::OS
    };
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}

/// Which Linux build this `oskac` is, from the target it was compiled for:
/// the glibc build (`gnu`) or the musl build (`musl`), the suffix of its
/// release asset (ADR 0022). `None` off Linux, where there is one build.
pub(crate) fn linux_build() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_env = "musl")) {
        Some("musl")
    } else if cfg!(all(target_os = "linux", target_env = "gnu")) {
        Some("gnu")
    } else {
        None
    }
}

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn user_home() -> Option<PathBuf> {
    env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

/// A path as the installer writes it: absolute, with no trailing separator.
pub(crate) fn clean(path: PathBuf) -> PathBuf {
    let absolute = if path.is_absolute() {
        path
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path,
        }
    };
    absolute.components().collect()
}

/// The Oska home: `OSKA_HOME`, else `.oska` in the user's home directory.
pub(crate) fn oska_home() -> Option<PathBuf> {
    env_path("OSKA_HOME")
        .or_else(|| user_home().map(|h| h.join(".oska")))
        .map(clean)
}

/// This executable's canonical path.
pub(crate) fn executable() -> Option<PathBuf> {
    std::env::current_exe().and_then(|p| p.canonicalize()).ok()
}

/// A path for output. Windows canonical paths carry a `\\?\` prefix; it is
/// dropped when a drive letter follows, as the path means the same without.
pub(crate) fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => text.into_owned(),
    }
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// A failed command: the error code the installer's `--json` uses, and why.
struct Refusal {
    code: &'static str,
    message: String,
}

fn refusal(code: &'static str, message: String) -> Refusal {
    Refusal { code, message }
}

fn fail(json: bool, command: &str, refusal: &Refusal) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":{},\"message\":{}}}",
            quote(refusal.code),
            quote(&refusal.message)
        );
    } else {
        eprintln!("oskac self {command}: {}", refusal.message);
    }
    ExitCode::from(1)
}

/// An unknown `self` subcommand or flag: a usage error, exit code 2, as
/// JSON under `--format=json`.
pub(crate) fn usage(json: bool) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":\"usage\",\"message\":{}}}",
            quote("the self commands are `oskac self info [--format=json]`, `oskac self check [--format=json] [--no-identify]`, `oskac self update [--version X [--allow-downgrade] | --rollback] [--keep-previous] [--format=json]` and `oskac self uninstall [--format=json]`")
        );
    } else {
        eprint!("{}", crate::cli::USAGE);
    }
    ExitCode::from(2)
}

/// What a receipt entry is. Every kind an installer writes is here, so a
/// new one is a match arm to add, not a string to miss.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// The `oskac` executable, in the Oska home's `bin`.
    Binary,
    /// A file the installer keeps about the install: the receipt itself
    /// and the legacy `version` file.
    Record,
    /// The line `install.sh --add-to-path` added to a shell profile.
    ProfileLine,
    /// The user-PATH entry `install.ps1 --add-to-path` added.
    UserPathEntry,
    /// The hook script `install.sh --hook` wrote into a project.
    HookScript,
    /// The Claude Code settings `install.sh --hook` created in a project,
    /// or found there (then with no SHA-256).
    HookSettings,
    /// A kind no installer this `oskac` knows writes.
    Unknown(String),
}

/// Every kind `oskac` knows, by the name receipts give it. The one place a
/// name maps to a kind: a test checks these against the kinds
/// `docs/guide/install-receipt.md` defines.
const KINDS: [(&str, Kind); 6] = [
    ("binary", Kind::Binary),
    ("record", Kind::Record),
    ("profile_line", Kind::ProfileLine),
    ("user_path_entry", Kind::UserPathEntry),
    ("hook_script", Kind::HookScript),
    ("hook_settings", Kind::HookSettings),
];

impl Kind {
    fn named(name: &str) -> Kind {
        KINDS
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| Kind::Unknown(name.to_string()), |(_, k)| k.clone())
    }

    fn name(&self) -> &str {
        match self {
            Kind::Unknown(name) => name,
            known => KINDS
                .iter()
                .find(|(_, k)| k == known)
                .map_or("", |(n, _)| n),
        }
    }

    /// Whether this platform's installer writes the kind: `install.ps1`
    /// the user PATH entry, `install.sh` the profile line and the hook
    /// files. A receipt that lists another is refused, as that installer's
    /// own `--uninstall` refuses it.
    fn written_here(&self) -> bool {
        match self {
            Kind::Binary | Kind::Record | Kind::Unknown(_) => true,
            Kind::UserPathEntry => cfg!(windows),
            Kind::ProfileLine | Kind::HookScript | Kind::HookSettings => !cfg!(windows),
        }
    }
}

/// One `files` entry of a receipt.
struct Entry {
    kind: Kind,
    sha256: String,
    path: String,
    /// A `profile_line`'s exact line.
    line: String,
    /// The project directory a hook entry was written into.
    project: String,
}

fn entries(receipt: &Json) -> Result<Vec<Entry>, String> {
    if receipt.get("schema").and_then(Json::as_f64) != Some(1.0) {
        return Err("its schema is not 1".to_string());
    }
    let Some(Json::Arr(files)) = receipt.get("files") else {
        return Err("it has no \"files\" array".to_string());
    };
    let text = |file: &Json, key: &str| {
        file.get(key)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(files
        .iter()
        .map(|file| Entry {
            kind: Kind::named(&text(file, "kind")),
            sha256: text(file, "sha256"),
            path: text(file, "path"),
            line: text(file, "line"),
            project: text(file, "project"),
        })
        .collect())
}

pub(crate) fn read_receipt(path: &Path) -> Result<Json, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Json::parse(&text)
}

/// Whether this `oskac` is the install the receipt records: the receipt's
/// version is this version, and one of its binary entries is this
/// executable, by path.
pub(crate) fn identity(receipt: &Json, exe: Option<&Path>, running: &str) -> Result<(), String> {
    let version = receipt.get("version").and_then(Json::as_str);
    if version != Some(running) {
        return Err(format!(
            "the receipt records version {}, and this oskac is {running}",
            version.unwrap_or("none")
        ));
    }
    let Some(exe) = exe else {
        return Err("this oskac's own path cannot be found".to_string());
    };
    let files = entries(receipt)?;
    let listed = files.iter().filter(|e| e.kind == Kind::Binary).any(|e| {
        let path = Path::new(&e.path);
        path.canonicalize().is_ok_and(|p| p == exe) || path == exe
    });
    if listed {
        Ok(())
    } else {
        Err(format!(
            "the receipt does not list {} as its binary",
            display(exe)
        ))
    }
}

/// `oskac self info [--format=json]`.
pub(crate) fn info(json: bool) -> ExitCode {
    let exe = executable();
    let home = oska_home();
    let mut receipt: Option<Json> = None;
    let mut receipt_error: Option<String> = None;
    if let Some(home) = &home {
        let path = home.join("receipt.json");
        if path.exists() {
            match read_receipt(&path) {
                Ok(parsed) => receipt = Some(parsed),
                Err(why) => receipt_error = Some(why),
            }
        }
    }
    let managed = receipt
        .as_ref()
        .map(|r| identity(r, exe.as_deref(), VERSION))
        .unwrap_or_else(|| Err("there is no readable install receipt".to_string()));

    if json {
        let mut receipt_json = String::from("null");
        if let Some(receipt) = &receipt {
            receipt_json.clear();
            receipt.write(&mut receipt_json);
        }
        outln!(
            "{{\"version\":{},\"platform\":{},\"linux_build\":{},\"executable\":{},\"oska_home\":{},\"receipt\":{},\"receipt_error\":{},\"receipt_matches\":{},\"digest_checked\":false}}",
            quote(VERSION),
            quote(&platform()),
            quote_option(linux_build()),
            quote_option(exe.as_deref().map(display).as_deref()),
            quote_option(home.as_deref().map(display).as_deref()),
            receipt_json,
            quote_option(receipt_error.as_deref()),
            managed.is_ok()
        );
        return ExitCode::SUCCESS;
    }
    match linux_build() {
        Some(build) => outln!("oskac {VERSION} ({}, {build} build)", platform()),
        None => outln!("oskac {VERSION} ({})", platform()),
    }
    outln!(
        "executable: {}",
        exe.as_deref()
            .map_or_else(|| "unknown".to_string(), display)
    );
    outln!(
        "Oska home: {}",
        home.as_deref().map_or_else(
            || "none (neither OSKA_HOME nor a home directory is set)".to_string(),
            display
        )
    );
    match (&receipt, &receipt_error) {
        (Some(r), _) => {
            let field = |key: &str| r.get(key).and_then(Json::as_str).unwrap_or("unknown");
            outln!(
                "receipt: version {}, installed {} by the {} installer",
                field("version"),
                field("installed_at"),
                field("installer")
            );
        }
        (None, Some(why)) => outln!("receipt: unreadable ({why})"),
        (None, None) => outln!("receipt: none"),
    }
    match managed {
        Ok(()) => outln!("managed install: yes, the receipt matches this oskac by path and version (its SHA-256 is not checked)"),
        Err(why) => outln!("managed install: no ({why})"),
    }
    ExitCode::SUCCESS
}

/// A file Windows would not delete because it is this running `oskac`,
/// moved aside instead: where it was, and where it went.
struct Pending {
    path: String,
    moved_to: String,
}

/// What `uninstall` found and did, for the report.
#[derive(Default)]
struct Outcome {
    removed: Vec<String>,
    /// A path kept, and why.
    kept: Vec<(String, String)>,
    pending: Vec<Pending>,
}

impl Outcome {
    fn keep(&mut self, path: &Path, why: impl Into<String>) {
        self.kept.push((display(path), why.into()));
    }
}

/// What a checked entry asks of the uninstall.
enum Plan {
    /// A file directly in the Oska home or its `bin`.
    InHome(PathBuf),
    /// A `--hook` file, removed if it is still the published one.
    Hook(PathBuf, &'static str),
    /// A profile line, left to the installer's `--uninstall`.
    Profile(PathBuf, String),
    /// The user-PATH entry, left to the installer's `--uninstall`.
    UserPath,
}

/// Where uninstall may act: the Oska home, its `bin`, and the user's home.
struct Places<'a> {
    home: &'a Path,
    bin: &'a Path,
    user_home: Option<&'a Path>,
}

fn unsafe_path(why: String) -> Refusal {
    refusal("unsafe_path", format!("{why}; nothing was removed"))
}

/// Whether two PATH entries are the same directory, as the PowerShell
/// installer compares them: trailing separators aside, and on Windows in
/// any case.
fn same_entry(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['\\', '/']).to_string();
    let (a, b) = (trim(a), trim(b));
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// Whether `text`, split on either separator, has a `.` or `..` part.
fn has_dot_part(text: &str) -> bool {
    text.split(['/', '\\'])
        .any(|part| part == "." || part == "..")
}

/// An entry, checked against where the installer writes its kind: what to
/// do with it, or the refusal.
fn checked_entry(entry: &Entry, places: &Places) -> Result<Plan, Refusal> {
    let listed = &entry.path;
    let path = Path::new(listed);
    if [listed, &entry.line, &entry.project, &entry.sha256]
        .iter()
        .any(|text| text.chars().any(char::is_control))
    {
        return Err(unsafe_path(
            "the receipt lists a path with a control character in it".to_string(),
        ));
    }
    if !entry.kind.written_here() {
        return Err(unsafe_path(format!(
            "the receipt lists {listed} as a '{}', which the installer for this platform never writes",
            entry.kind.name()
        )));
    }
    if entry.kind == Kind::UserPathEntry {
        // The bin directory, as it went into the user PATH.
        return if same_entry(listed, &places.bin.to_string_lossy()) {
            Ok(Plan::UserPath)
        } else {
            Err(unsafe_path(format!(
                "the receipt lists {listed} as an entry of the user PATH, which is not the bin directory {}",
                display(places.bin)
            )))
        };
    }
    if !path.is_absolute() {
        return Err(unsafe_path(format!(
            "the receipt lists '{listed}', which is not an absolute path"
        )));
    }
    // The text is checked as well as the parsed path: on Windows, `Path`
    // treats a `.` in a `\\?\` path as a name, not as a step.
    let parts: Vec<Component> = path.components().collect();
    if parts.contains(&Component::ParentDir) || has_dot_part(listed) {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which climbs out with .."
        )));
    }
    // Windows names are case-insensitive, so `.Config\Oska` is the same
    // directory there.
    let named = |part: &Component, name: &str| match part {
        Component::Normal(n) if cfg!(windows) => n.to_string_lossy().eq_ignore_ascii_case(name),
        Component::Normal(n) => *n == name,
        _ => false,
    };
    let in_config = parts
        .windows(2)
        .any(|w| named(&w[0], ".config") && named(&w[1], "oska"));
    if in_config {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, in a .config/oska directory of settings and credentials, which uninstall never touches"
        )));
    }
    let plan = match &entry.kind {
        Kind::Binary | Kind::Record => in_home(path, places)?,
        Kind::ProfileLine => profile_line(path, &entry.line, places)?,
        Kind::HookScript | Kind::HookSettings => hook_file(entry, path)?,
        // Checked above, before the path rules, as it is not a file.
        Kind::UserPathEntry => Plan::UserPath,
        Kind::Unknown(kind) => {
            return Err(unsafe_path(format!(
                "the receipt lists {listed} as a '{kind}', which this installer does not remove"
            )))
        }
    };
    if is_symlink(path) {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which is reached through a symlink"
        )));
    }
    Ok(plan)
}

/// A `binary` or `record`: directly in the Oska home or its `bin`.
fn in_home(path: &Path, places: &Places) -> Result<Plan, Refusal> {
    let home = places.home;
    let Ok(rest) = path.strip_prefix(home) else {
        return Err(unsafe_path(format!(
            "the receipt lists {}, which is outside the Oska home {}",
            path.display(),
            display(home)
        )));
    };
    let names: Vec<&std::ffi::OsStr> = rest
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let dir = match names.as_slice() {
        [_] => home,
        [first, _] if *first == "bin" => places.bin,
        _ => {
            let home = display(home);
            return Err(unsafe_path(format!(
                "the receipt lists {}, which is not directly in {home} or {home}/bin",
                path.display()
            )));
        }
    };
    if is_symlink(dir) {
        return Err(unsafe_path(format!(
            "the receipt lists {}, which is reached through a symlink",
            path.display()
        )));
    }
    Ok(Plan::InHome(path.to_path_buf()))
}

/// A `profile_line`: one of the profiles `--add-to-path` picks, directly in
/// the user's home, and the marked line it writes, so that a receipt
/// cannot name some other line or file.
fn profile_line(path: &Path, line: &str, places: &Places) -> Result<Plan, Refusal> {
    let listed = path.display();
    let Some(user_home) = places.user_home else {
        return Err(unsafe_path(format!(
            "the receipt lists the profile {listed}, and there is no home directory to check it against"
        )));
    };
    let is_profile = path.parent() == Some(user_home)
        && path
            .file_name()
            .is_some_and(|name| PROFILES.iter().any(|p| name == *p));
    if !is_profile {
        return Err(unsafe_path(format!(
            "the receipt lists the profile {listed}, which is not a shell profile directly in {}",
            display(user_home)
        )));
    }
    if !(line.starts_with("export PATH=") && line.ends_with(PROFILE_MARK)) {
        return Err(unsafe_path(format!(
            "the receipt lists a line of {listed} that is not the marked line the installer adds"
        )));
    }
    no_symlink_above(path)?;
    Ok(Plan::Profile(path.to_path_buf(), line.to_string()))
}

/// A `hook_script` or `hook_settings`: in the project recorded with it, at
/// the place `--hook` writes it.
fn hook_file(entry: &Entry, path: &Path) -> Result<Plan, Refusal> {
    let listed = path.display();
    let project = Path::new(&entry.project);
    if !project.is_absolute() {
        return Err(unsafe_path(format!(
            "the receipt lists {listed} with the project '{}', which is not an absolute path",
            entry.project
        )));
    }
    if has_dot_part(&entry.project) {
        return Err(unsafe_path(format!(
            "the receipt lists the project {}, which climbs out with ..",
            entry.project
        )));
    }
    let claude = project.join(".claude");
    let (place, published) = if entry.kind == Kind::HookScript {
        (claude.join("hooks").join("oska-check.sh"), HOOK_SCRIPT)
    } else {
        (claude.join("settings.json"), HOOK_SETTINGS)
    };
    if path != place {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which is not where --hook writes it in the project {}",
            entry.project
        )));
    }
    no_symlink_above(path)?;
    Ok(Plan::Hook(path.to_path_buf(), published))
}

/// Refuse if a directory above `path` is a symlink, unless root owns it
/// (on Unix): only root can make a symlink root owns, so such a one, like
/// macOS's `/var`, is the system's. One anyone else made could steer a
/// deletion out of where it should be.
fn no_symlink_above(path: &Path) -> Result<(), Refusal> {
    for above in path.ancestors().skip(1) {
        let Ok(meta) = std::fs::symlink_metadata(above) else {
            continue;
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() == 0 {
                continue;
            }
        }
        return Err(unsafe_path(format!(
            "{} is reached through the symlink {}",
            display(path),
            display(above)
        )));
    }
    Ok(())
}

/// What became of a file uninstall removes.
enum Removal {
    Removed,
    /// Windows would not delete the running `oskac`, so it was moved here.
    Pending(PathBuf),
    Failed,
}

/// Remove a file. Windows will not delete a running executable but will
/// rename it, so this `oskac` is moved into the temporary directory under
/// a name of its own, which empties its place; it is reported as pending.
fn remove_file(path: &Path) -> Removal {
    if std::fs::remove_file(path).is_ok() {
        return Removal::Removed;
    }
    if !cfg!(windows)
        || !executable().is_some_and(|exe| path.canonicalize().is_ok_and(|p| p == exe))
    {
        return Removal::Failed;
    }
    let aside = std::env::temp_dir().join(format!("oskac-uninstalled-{}.exe", std::process::id()));
    match std::fs::rename(path, &aside) {
        Ok(()) => Removal::Pending(aside),
        Err(_) => Removal::Failed,
    }
}

/// Whether the file at `path` is byte for byte `published`.
fn is_published(path: &Path, published: &str) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes == published.as_bytes())
}

/// Whether `text` has a `\u` escape for anything but printable ASCII. The
/// installers refuse a receipt with one, as they cannot tell it from a
/// control character, so `oskac` does too.
fn has_escape_outside_ascii(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'u') {
            let code = text
                .get(i + 2..i + 6)
                .and_then(|hex| u32::from_str_radix(hex, 16).ok());
            if !code.is_some_and(|c| (0x20..0x7f).contains(&c)) {
                return true;
            }
        }
        i += 2;
    }
    false
}

/// Whether an object anywhere in `value` has two keys that differ only in
/// ASCII case. The parser refuses an exact repeat; PowerShell cannot tell
/// these apart either, so `install.ps1` refuses them, and `oskac` does too.
fn repeats_a_key_but_for_case(value: &Json) -> bool {
    match value {
        Json::Obj(fields) => {
            fields.iter().enumerate().any(|(i, (key, _))| {
                fields[..i]
                    .iter()
                    .any(|(earlier, _)| earlier.eq_ignore_ascii_case(key))
            }) || fields.iter().any(|(_, v)| repeats_a_key_but_for_case(v))
        }
        Json::Arr(items) => items.iter().any(repeats_a_key_but_for_case),
        _ => false,
    }
}

/// Whether a `binary` or `record` entry with a SHA-256, other than the
/// receipt, is still what the installer wrote, as far as `oskac` can tell
/// without computing a SHA-256: a binary only if it is this `oskac` (or the
/// kept copy, whose SHA-256 is computed), and a
/// record only if it is the `version` file holding the receipt's version.
/// Anything else is kept, with why.
fn unchanged_in_home(
    entry: &Entry,
    path: &Path,
    receipt: &Json,
    home: &Path,
) -> Result<(), String> {
    match entry.kind {
        Kind::Binary => {
            let this = executable().is_some_and(|exe| path.canonicalize().is_ok_and(|p| p == exe));
            // The copy `self update --keep-previous` made: only at the name
            // it gives one beside this oskac, as a regular file and not a
            // link; the receipt's word for the path is not enough.
            let kept = executable()
                .and_then(|exe| crate::self_update::recorded_kept(receipt, &exe))
                .is_some_and(|k| k.path == path)
                && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file());
            if this {
                Ok(())
            } else if kept {
                // The copy `self update --keep-previous` made: removed if it
                // still has the SHA-256 recorded from the signed SHA256SUMS.
                let sum = std::fs::read(path)
                    .map(|bytes| crate::sha256::hex(&crate::sha256::digest(&bytes)));
                if sum.is_ok_and(|sum| sum == entry.sha256) {
                    Ok(())
                } else {
                    Err("changed since it was installed".to_string())
                }
            } else {
                Err("a binary other than this oskac, which cannot tell it is unchanged without its SHA-256; the installer's --uninstall removes it".to_string())
            }
        }
        _ if path == home.join("version") => {
            let version = receipt.get("version").and_then(Json::as_str).unwrap_or_default();
            if std::fs::read(path).is_ok_and(|text| text == format!("{version}\n").as_bytes()) {
                Ok(())
            } else {
                Err("changed since it was installed".to_string())
            }
        }
        _ => Err("a record other than the version file and the receipt, which oskac cannot tell is unchanged without its SHA-256; the installer's --uninstall removes it".to_string()),
    }
}

/// Check everything, then remove. Returns the report, or why nothing was
/// removed.
fn uninstall_in(home: &Path) -> Result<(Outcome, Option<String>), Refusal> {
    let receipt_path = home.join("receipt.json");
    let bin_dir = home.join("bin");
    if is_symlink(home) || is_symlink(&receipt_path) {
        return Err(unsafe_path(format!(
            "{} is reached through a symlink",
            display(&receipt_path)
        )));
    }
    no_symlink_above(home)?;
    if !receipt_path.is_file() {
        return Err(refusal(
            "not_installed",
            format!(
                "there is no install receipt at {}, so nothing is known to have been installed there; nothing was removed",
                display(&receipt_path)
            ),
        ));
    }
    let cannot_read = |why: String| {
        refusal(
            "unsafe_path",
            format!(
                "cannot read the install receipt {}, so none of its paths is known to be safe: {why}",
                display(&receipt_path)
            ),
        )
    };
    // Read as strictly as the installers read it.
    let text = std::fs::read_to_string(&receipt_path).map_err(|e| cannot_read(e.to_string()))?;
    if has_escape_outside_ascii(&text) {
        return Err(cannot_read(
            "it has a \\u escape for something other than printable ASCII".to_string(),
        ));
    }
    let receipt = Json::parse(&text).map_err(cannot_read)?;
    if repeats_a_key_but_for_case(&receipt) {
        return Err(cannot_read(
            "it has keys that differ only in case, which install.ps1 cannot tell apart".to_string(),
        ));
    }
    if receipt.get("schema") != Some(&Json::Num("1".to_string())) {
        return Err(cannot_read("its schema is not the integer 1".to_string()));
    }
    let files = entries(&receipt).map_err(cannot_read)?;
    let user_home = user_home();
    let places = Places {
        home,
        bin: &bin_dir,
        user_home: user_home.as_deref(),
    };

    // Every entry is checked before anything is removed.
    let plans = files
        .iter()
        .map(|entry| Ok((checked_entry(entry, &places)?, entry)))
        .collect::<Result<Vec<_>, Refusal>>()?;

    // The binary and the records are this install's only when the receipt
    // records this oskac; otherwise they are skipped, and the rest goes.
    let managed = identity(&receipt, executable().as_deref(), VERSION);
    let mut outcome = Outcome::default();
    // `bin` is handled with the Oska home, after the files.
    let mut acted: HashSet<PathBuf> = HashSet::from([bin_dir.clone()]);
    let mut remove: Vec<PathBuf> = Vec::new();
    let mut remove_receipt = false;
    let mut path_edits: Vec<String> = Vec::new();
    for (plan, entry) in plans {
        let (path, published) = match plan {
            Plan::Profile(profile, line) => {
                path_edits.push(format!(
                    "remove the line the installer added to {}, if it is still there: {line}",
                    display(&profile)
                ));
                continue;
            }
            Plan::UserPath => {
                path_edits.push(format!("remove {} from your user PATH", display(&bin_dir)));
                continue;
            }
            Plan::InHome(path) => (path, None),
            Plan::Hook(path, published) => (path, Some(published)),
        };
        // A path listed again is checked like any entry, and acted on once.
        if !acted.insert(path.clone()) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            outcome.keep(&path, "not a regular file");
            continue;
        }
        match (published, &managed) {
            (None, Err(why)) => outcome.keep(
                &path,
                format!("this oskac is not the install the receipt records: {why}"),
            ),
            (None, Ok(())) if path == receipt_path => remove_receipt = true,
            (None, Ok(())) if !entry.sha256.is_empty() => {
                match unchanged_in_home(entry, &path, &receipt, home) {
                    Ok(()) => remove.push(path),
                    Err(why) => outcome.keep(&path, why),
                }
            }
            (Some(_), _) if entry.sha256.is_empty() && entry.kind == Kind::HookSettings => {
                outcome.keep(
                    &path,
                    "settings the installer did not create; remove its oskac hook entries by hand",
                );
            }
            _ if entry.sha256.is_empty() => {
                outcome.keep(&path, "the receipt records no SHA-256 for it");
            }
            (Some(published), _) if !is_published(&path, published) => outcome.keep(
                &path,
                "changed since it was installed, or not the published file this oskac knows",
            ),
            (Some(_), _) | (None, Ok(())) => remove.push(path),
        }
    }

    // The receipt goes last, and stays if a file could not be removed, so
    // a second run can finish the job.
    let mut failed = false;
    for path in remove {
        match remove_file(&path) {
            Removal::Removed => outcome.removed.push(display(&path)),
            Removal::Pending(aside) => outcome.pending.push(Pending {
                path: display(&path),
                moved_to: display(&aside),
            }),
            Removal::Failed => {
                failed = true;
                outcome.keep(&path, "could not be removed");
            }
        }
    }
    if remove_receipt {
        if !failed && std::fs::remove_file(&receipt_path).is_ok() {
            outcome.removed.push(display(&receipt_path));
        } else {
            outcome.keep(&receipt_path, "kept while other files remain");
        }
    }
    for dir in [&bin_dir, &home.to_path_buf()] {
        if !dir.is_dir() || is_symlink(dir) {
            continue;
        }
        if std::fs::remove_dir(dir).is_ok() {
            outcome.removed.push(display(dir));
        } else {
            outcome.keep(dir, "not empty");
        }
    }
    if let Some(config) = user_home.map(|h| h.join(".config").join("oska")) {
        if std::fs::symlink_metadata(&config).is_ok() {
            outcome.keep(
                &config,
                "settings and credentials, which uninstall never touches",
            );
        }
    }
    let path_hint = if path_edits.is_empty() {
        let on_path = std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|p| p == bin_dir));
        on_path.then(|| {
            format!(
                "{} is on your PATH; remove it from the shell profile that adds it",
                display(&bin_dir)
            )
        })
    } else {
        Some(format!(
            "{}; or run the installer with --uninstall, which undoes the PATH changes it made",
            path_edits.join("; ")
        ))
    };
    Ok((outcome, path_hint))
}

/// `oskac self uninstall [--format=json]`.
pub(crate) fn uninstall(json: bool) -> ExitCode {
    let Some(home) = oska_home() else {
        return fail(
            json,
            "uninstall",
            &refusal(
                "not_installed",
                "neither OSKA_HOME nor a home directory is set, so there is no Oska home; set OSKA_HOME"
                    .to_string(),
            ),
        );
    };
    let (outcome, path_hint) = match uninstall_in(&home) {
        Ok(done) => done,
        Err(why) => return fail(json, "uninstall", &why),
    };
    if json {
        let list = |items: Vec<&str>| {
            let quoted: Vec<String> = items.into_iter().map(quote).collect();
            format!("[{}]", quoted.join(","))
        };
        let pending: Vec<String> = outcome
            .pending
            .iter()
            .map(|p| {
                format!(
                    "{{\"path\":{},\"moved_to\":{},\"note\":{}}}",
                    quote(&p.path),
                    quote(&p.moved_to),
                    quote("Windows cannot delete a running program; delete moved_to once oskac has exited")
                )
            })
            .collect();
        outln!(
            "{{\"ok\":true,\"action\":\"uninstalled\",\"removed\":{},\"kept\":{},\"pending\":[{}],\"path_hint\":{}}}",
            list(outcome.removed.iter().map(String::as_str).collect()),
            list(outcome.kept.iter().map(|(p, _)| p.as_str()).collect()),
            pending.join(","),
            quote_option(path_hint.as_deref())
        );
        return ExitCode::SUCCESS;
    }
    outln!("Uninstalled oskac from {}.", display(&home));
    if !outcome.removed.is_empty() {
        outln!("Removed:");
        for path in &outcome.removed {
            outln!("  {path}");
        }
    }
    if !outcome.pending.is_empty() {
        outln!("Pending:");
        for p in &outcome.pending {
            outln!(
                "  {} (Windows cannot delete a running program, so it was moved to {}; delete that file once oskac has exited)",
                p.path,
                p.moved_to
            );
        }
    }
    if !outcome.kept.is_empty() {
        outln!("Kept:");
        for (path, why) in &outcome.kept {
            outln!("  {path} ({why})");
        }
    }
    if let Some(hint) = path_hint {
        outln!("{hint}.");
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{has_escape_outside_ascii, Kind, KINDS};

    /// The kinds the receipt doc's "Entry kinds" table defines.
    fn documented_kinds() -> Vec<String> {
        let doc = include_str!("../../../docs/guide/install-receipt.md");
        let table = doc
            .split("## Entry kinds")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("an Entry kinds section");
        table
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn oskac_knows_exactly_the_documented_kinds() {
        let known: Vec<String> = KINDS.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(known, documented_kinds());
        for (name, kind) in KINDS {
            assert_eq!(Kind::named(name), kind);
            assert_eq!(kind.name(), name);
        }
        assert_eq!(Kind::named("directory"), Kind::Unknown("directory".into()));
    }

    #[test]
    fn only_escapes_for_printable_ascii_are_read() {
        // A JSON `\u` escape for `hex`, spelled out so that no tool reads it.
        let u = |hex: &str| ["\"", "\\", "u", hex, "\""].concat();
        let fine = [
            "{\"a\":",
            &u("0041"),
            ",\"b\":\"\\\\",
            "u00e9 \\n \\\" é\"}",
        ]
        .concat();
        assert!(!has_escape_outside_ascii(&fine), "{fine}");
        for bad in [u("00e9"), u("001f"), u("007f"), u("12")] {
            assert!(has_escape_outside_ascii(&bad), "{bad}");
        }
    }
}
