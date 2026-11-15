//! `oskac self info` and `oskac self uninstall`: ask this `oskac` about its
//! own installation, and remove it, without the installer. Both read the
//! install receipt (`receipt.json` in the Oska home) that the installers
//! write, and neither makes a network connection.
//!
//! `uninstall` applies the installer's `--uninstall` rules in-process: it
//! removes only what the receipt lists, only files directly in the Oska
//! home or its `bin` directory, checks every entry before it removes
//! anything, and never touches `~/.config/oska`. The running `oskac` is
//! identified as the managed install by its path and version; comparing a
//! file's SHA-256 to the receipt's waits for Oska's own crypto, so a file
//! changed since it was installed is not yet kept.

use crate::self_json::{quote, Json};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The platform name release assets use, as the installers record it.
fn platform() -> String {
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

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn user_home() -> Option<PathBuf> {
    env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

/// A path as the installer writes it: absolute, with no trailing separator.
fn clean(path: PathBuf) -> PathBuf {
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
fn oska_home() -> Option<PathBuf> {
    env_path("OSKA_HOME")
        .or_else(|| user_home().map(|h| h.join(".oska")))
        .map(clean)
}

/// This executable's canonical path.
fn executable() -> Option<PathBuf> {
    std::env::current_exe().and_then(|p| p.canonicalize()).ok()
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

/// One `files` entry of a receipt.
struct Entry {
    kind: String,
    sha256: String,
    path: String,
}

fn entries(receipt: &Json) -> Result<Vec<Entry>, String> {
    if receipt.get("schema") != Some(&Json::Num("1".to_string())) {
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
            kind: text(file, "kind"),
            sha256: text(file, "sha256"),
            path: text(file, "path"),
        })
        .collect())
}

fn read_receipt(path: &Path) -> Result<Json, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Json::parse(&text)
}

/// Whether this `oskac` is the install the receipt records: the receipt's
/// version is this version, and one of its binary entries is this
/// executable, by path.
fn identity(receipt: &Json, exe: Option<&Path>) -> Result<(), String> {
    let version = receipt.get("version").and_then(Json::as_str);
    if version != Some(VERSION) {
        return Err(format!(
            "the receipt records version {}, and this oskac is {VERSION}",
            version.unwrap_or("none")
        ));
    }
    let Some(exe) = exe else {
        return Err("this oskac's own path cannot be found".to_string());
    };
    let files = entries(receipt)?;
    let listed = files.iter().filter(|e| e.kind == "binary").any(|e| {
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
        .map(|r| identity(r, exe.as_deref()))
        .unwrap_or_else(|| Err("there is no readable install receipt".to_string()));

    if json {
        let mut receipt_json = String::from("null");
        if let Some(receipt) = &receipt {
            receipt_json.clear();
            receipt.write(&mut receipt_json);
        }
        let string_or_null =
            |s: Option<String>| s.map_or_else(|| "null".to_string(), |s| quote(&s));
        outln!(
            "{{\"version\":{},\"platform\":{},\"executable\":{},\"oska_home\":{},\"receipt\":{},\"receipt_error\":{},\"receipt_matches\":{}}}",
            quote(VERSION),
            quote(&platform()),
            string_or_null(exe.as_deref().map(display)),
            string_or_null(home.as_deref().map(display)),
            receipt_json,
            string_or_null(receipt_error),
            managed.is_ok()
        );
        return ExitCode::SUCCESS;
    }
    outln!("oskac {VERSION} ({})", platform());
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
        Ok(()) => outln!("managed install: yes, the receipt matches this oskac"),
        Err(why) => outln!("managed install: no ({why})"),
    }
    ExitCode::SUCCESS
}

/// What `uninstall` found and did, for the report.
#[derive(Default)]
struct Outcome {
    removed: Vec<String>,
    /// A path kept, and why.
    kept: Vec<(String, &'static str)>,
}

impl Outcome {
    fn keep(&mut self, path: &Path, why: &'static str) {
        self.kept.push((display(path), why));
    }
}

/// An entry's path, checked: the path inside the Oska home it names, or
/// the refusal.
fn checked_entry(entry: &Entry, home: &Path) -> Result<PathBuf, Refusal> {
    let unsafe_path = |why: String| refusal("unsafe_path", format!("{why}; nothing was removed"));
    let listed = &entry.path;
    let path = Path::new(listed);
    if listed.chars().any(char::is_control) {
        return Err(unsafe_path(
            "the receipt lists a path with a control character in it".to_string(),
        ));
    }
    if !path.is_absolute() {
        return Err(unsafe_path(format!(
            "the receipt lists '{listed}', which is not an absolute path"
        )));
    }
    let parts: Vec<Component> = path.components().collect();
    if parts.contains(&Component::ParentDir) {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which climbs out with .."
        )));
    }
    let in_config = parts.windows(2).any(|w| {
        matches!(w[0], Component::Normal(a) if a == ".config")
            && matches!(w[1], Component::Normal(b) if b == "oska")
    });
    if in_config {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, in a .config/oska directory of settings and credentials, which uninstall never touches"
        )));
    }
    if entry.kind != "binary" && entry.kind != "record" {
        return Err(unsafe_path(format!(
            "the receipt lists {listed} as a '{}', which this installer does not remove",
            entry.kind
        )));
    }
    let Ok(rest) = path.strip_prefix(home) else {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which is outside the Oska home {}",
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
    let bin = std::ffi::OsStr::new("bin");
    let dir = match names.as_slice() {
        [_] => home.to_path_buf(),
        [first, _] if *first == bin => home.join("bin"),
        _ => {
            return Err(unsafe_path(format!(
                "the receipt lists {listed}, which is not directly in {0} or {0}/bin",
                display(home)
            )))
        }
    };
    if is_symlink(&dir) || is_symlink(path) {
        return Err(unsafe_path(format!(
            "the receipt lists {listed}, which is reached through a symlink"
        )));
    }
    Ok(path.to_path_buf())
}

/// Refuse if a directory above `home` is a symlink, unless root owns it
/// (on Unix): only root can make a symlink root owns, so such a one, like
/// macOS's `/var`, is the system's. One anyone else made could steer a
/// deletion out of where it should be.
fn no_symlink_above(home: &Path) -> Result<(), Refusal> {
    for above in home.ancestors().skip(1) {
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
        return Err(refusal(
            "unsafe_path",
            format!(
                "{} is reached through the symlink {}; nothing was removed",
                display(home),
                display(above)
            ),
        ));
    }
    Ok(())
}

/// Check everything, then remove. Returns the report, or why nothing was
/// removed.
fn uninstall_in(home: &Path) -> Result<(Outcome, Option<String>), Refusal> {
    let receipt_path = home.join("receipt.json");
    let bin_dir = home.join("bin");
    if is_symlink(home) || is_symlink(&receipt_path) {
        return Err(refusal(
            "unsafe_path",
            format!(
                "{} is reached through a symlink; nothing was removed",
                display(&receipt_path)
            ),
        ));
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
    let receipt = read_receipt(&receipt_path).map_err(cannot_read)?;
    let files = entries(&receipt).map_err(cannot_read)?;

    let mut outcome = Outcome::default();
    let mut acted: HashSet<PathBuf> = HashSet::new();
    let mut remove: Vec<PathBuf> = Vec::new();
    let mut remove_receipt = false;
    for entry in &files {
        let path = checked_entry(entry, home)?;
        if !acted.insert(path.clone()) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            outcome.keep(&path, "not a regular file");
        } else if path == receipt_path {
            remove_receipt = true;
        } else if entry.sha256.is_empty() {
            outcome.keep(&path, "the receipt records no SHA-256 for it");
        } else {
            remove.push(path);
        }
    }

    // Only the install the receipt records may uninstall itself.
    identity(&receipt, executable().as_deref()).map_err(|why| {
        refusal(
            "not_managed",
            format!(
                "this oskac is not the managed install the receipt at {} records ({why}); nothing was removed",
                display(&receipt_path)
            ),
        )
    })?;

    // The receipt goes last, and stays if a file could not be removed, so
    // a second run can finish the job.
    let mut failed = false;
    for path in remove {
        if std::fs::remove_file(&path).is_ok() {
            outcome.removed.push(display(&path));
        } else {
            failed = true;
            outcome.keep(&path, "could not be removed");
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
    if let Some(config) = user_home().map(|h| h.join(".config").join("oska")) {
        if std::fs::symlink_metadata(&config).is_ok() {
            outcome.keep(
                &config,
                "settings and credentials, which uninstall never touches",
            );
        }
    }
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|p| p == bin_dir));
    let path_hint = on_path.then(|| {
        format!(
            "{} is on your PATH; remove it from the shell profile that adds it",
            display(&bin_dir)
        )
    });
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
        outln!(
            "{{\"ok\":true,\"action\":\"uninstalled\",\"removed\":{},\"kept\":{},\"path_hint\":{}}}",
            list(outcome.removed.iter().map(String::as_str).collect()),
            list(outcome.kept.iter().map(|(p, _)| p.as_str()).collect()),
            path_hint.as_deref().map_or_else(|| "null".to_string(), quote)
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
