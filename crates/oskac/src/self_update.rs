// SPDX-License-Identifier: Apache-2.0
//! `oskac self update [--version X [--allow-downgrade] | --rollback]
//! [--keep-previous] [--format=json]` (spec #230): replace this `oskac` with
//! a verified release, as the installer does, and only on a managed install.
//!
//! `--rollback` goes back exactly one version, the receipt's
//! `previous_version`. `--keep-previous` keeps a copy of the binary this
//! update replaces, for that update only: beside the live binary, recorded
//! in the receipt (`kept_previous`: version, path and the SHA-256 from the
//! signed `SHA256SUMS`, and as a `binary` entry of `files`, which the
//! installers' uninstall and `oskac self uninstall` remove). A rollback uses
//! the kept copy only if the update checker, which re-hashes it, finds the
//! recorded digest; otherwise it downloads the release as an install does.
//! Every update or rollback discards a kept copy it does not keep again.
//!
//! The update checker (an Oska program, `update_checker.rs`) does everything
//! that needs trust and the network: it verifies the signed index, chooses
//! the release, applies the downgrade rule, verifies the signed `SHA256SUMS`
//! and the downloaded binary's digest, and only then writes the binary to a
//! staging file, the one path its grant lets it write. It never writes the
//! live binary. This side plans the work and does the plumbing: the
//! managed-install rule (before any network), running the staged binary to
//! read its version, replacing the live binary and updating the receipt. It
//! does no cryptography and no TLS.
//!
//! Any failure before the replacement leaves the live binary and the
//! receipt as they were and removes the staging file. The replacement is
//! one rename on Unix; on Windows, where a running executable cannot be
//! overwritten but can be renamed, the running file is renamed aside and the
//! new one moved in, and the renamed-aside file is deleted as soon as the
//! system lets it be (the next `oskac self check` or `self update` at the latest).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use crate::json::{quote, Json};
use crate::self_check::{
    assets, failed, next_step, receipt, remembered, report_failure, running, version, Failed,
    Receipt, SITE,
};
use crate::self_cmd::{display, identity};
use crate::update_checker::{self, Ask, UpdateAsk, UpdateVerdict, Updated};

/// How long the staged binary may take to say its version.
const VERSION_DEADLINE: Duration = Duration::from_secs(30);

/// The marker in the name of a file renamed aside on Windows.
const ASIDE: &str = ".oskac-old-";

/// What was asked of `oskac self update`.
pub(crate) struct Request {
    /// `--version X`.
    pub(crate) version: Option<String>,
    pub(crate) allow_downgrade: bool,
    /// `--rollback`: go back to the receipt's `previous_version`.
    pub(crate) rollback: bool,
    /// `--keep-previous`: keep the replaced binary.
    pub(crate) keep_previous: bool,
}

/// `oskac self update`.
pub(crate) fn update(json: bool, request: &Request) -> ExitCode {
    match run(request) {
        Ok(done) => {
            done.print(json);
            ExitCode::SUCCESS
        }
        Err(failure) => report_failure("update", json, &failure),
    }
}

struct Done {
    updated: Box<Updated>,
    notes: Vec<String>,
    rollback: bool,
    /// `Some` when `--keep-previous` was given: the copy kept, or none.
    keep: Option<Option<Kept>>,
}

/// A copy of a replaced binary, as the receipt records it.
#[derive(Clone)]
pub(crate) struct Kept {
    version: String,
    pub(crate) path: PathBuf,
    sha256: String,
}

/// Where the copy of version `version` is kept: beside the live binary, a
/// name of its own. Anything outside a plain file name is made `_`.
fn kept_path(exe: &Path, version: &str) -> Option<PathBuf> {
    let stem = exe.file_stem()?.to_str()?;
    let plain: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(exe.with_file_name(format!("{stem}-{plain}{}", std::env::consts::EXE_SUFFIX)))
}

/// Remove a kept copy this `oskac` made: a regular file, never a link
/// (which could stand for something else), at the name it would give it.
fn remove_kept(copy: &Kept) {
    if std::fs::symlink_metadata(&copy.path).is_ok_and(|m| m.is_file()) {
        let _ = std::fs::remove_file(&copy.path);
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The kept copy the receipt records, if what it says is where this
/// `oskac` would put one (so that a receipt cannot name another file to be
/// used or deleted).
pub(crate) fn recorded_kept(receipt: &Json, exe: &Path) -> Option<Kept> {
    let entry = receipt.get("kept_previous")?;
    let text = |key: &str| entry.get(key).and_then(Json::as_str).map(str::to_string);
    let (version, path, sha256) = (text("version")?, text("path")?, text("sha256")?);
    let path = PathBuf::from(path);
    (is_digest(&sha256) && Some(&path) == kept_path(exe, &version).as_ref()).then_some(Kept {
        version,
        path,
        sha256,
    })
}

/// Why this install is not managed, and what to do about it.
fn unmanaged(why: &str) -> Failed {
    failed(
        "unmanaged_install",
        format!(
            "this oskac is not a managed install ({why}), so it will not replace itself; nothing was changed. `oskac self update` replaces only a binary the installer put in place and recorded. Re-run the installer ({}), or rebuild from source",
            next_step(false)
        ),
    )
}

/// The receipt of a managed install, or the refusal.
fn managed(exe: &Path) -> Result<Receipt, Failed> {
    let Some(found) = receipt(exe) else {
        return Err(unmanaged("there is no readable install receipt"));
    };
    match identity(&found.json, Some(exe), &version()) {
        Ok(()) => Ok(found),
        Err(why) if why.starts_with("the receipt records version") => Err(unmanaged(&format!(
            "{why}, so this binary is not the one the installer recorded: a copy, or one replaced by hand"
        ))),
        Err(why) => Err(unmanaged(&why)),
    }
}

/// The staging file: a name of its own beside the live binary, so that the
/// final step is a rename on one volume. It is removed unless it was moved
/// into place, and on a crash of `oskac`.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn new(exe: &Path) -> Result<Staging, Failed> {
        let name = format!(
            ".oskac-update-{}{}",
            std::process::id(),
            std::env::consts::EXE_SUFFIX
        );
        let path = exe.with_file_name(name);
        // A file left by an earlier run with this process id is not ours to
        // write through.
        match std::fs::symlink_metadata(&path) {
            Ok(_) => std::fs::remove_file(&path).map_err(|e| {
                failed(
                    "internal",
                    format!("cannot clear the staging file {}: {e}", path.display()),
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(failed(
                    "internal",
                    format!("cannot look at the staging file {}: {e}", path.display()),
                ))
            }
        }
        crate::crash::track(&path);
        Ok(Staging { path })
    }

    fn name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        crate::crash::untrack(&self.path);
    }
}

fn run(request: &Request) -> Result<Done, Failed> {
    let exe =
        running().ok_or_else(|| failed("internal", "this oskac's own path cannot be found"))?;
    // Before any network use.
    let receipt = managed(&exe)?;
    sweep_aside(&exe);
    let version = version();
    let old_kept = recorded_kept(&receipt.json, &exe);
    let mut allow_downgrade = request.allow_downgrade;
    let mut requested = request.version.as_deref();
    let mut use_kept = None;
    if request.rollback {
        let previous = receipt
            .json
            .get("previous_version")
            .and_then(Json::as_str)
            .filter(|p| crate::semver::parse(p).is_some())
            .ok_or_else(|| {
                failed(
                    "no_previous_version",
                    "the install receipt records no previous version, so there is nothing to roll back to; nothing was changed. To install an older release, use `oskac self update --version X --allow-downgrade`",
                )
            })?;
        if previous == version {
            return Err(failed(
                "no_previous_version",
                format!("the install receipt's previous version is {version}, the one installed; nothing was changed"),
            ));
        }
        requested = Some(previous);
        allow_downgrade = true;
        use_kept = old_kept
            .as_ref()
            .filter(|k| k.version == previous)
            .and_then(|k| {
                let meta = std::fs::symlink_metadata(&k.path).ok()?;
                let name = k.path.file_name()?.to_str()?;
                meta.is_file().then(|| (name.to_string(), k.sha256.clone()))
            });
    }
    let assets = assets();
    let staging = Staging::new(&exe)?;
    let ask = UpdateAsk {
        ask: Ask {
            version: &version,
            now: update_checker::now(),
            remembered: remembered(&receipt),
            identify: request.keep_previous,
            exe: &exe,
            assets: &assets,
        },
        requested,
        allow_downgrade,
        staging: staging.name(),
        kept: use_kept.as_ref().map(|(n, s)| (n.as_str(), s.as_str())),
    };
    let mut updated = match update_checker::update(&ask) {
        Ok(UpdateVerdict::Done(updated)) => updated,
        Ok(UpdateVerdict::Refused { code, message }) => return Err(Failed { code, message }),
        Err(why) => return Err(failed("internal", why.0)),
    };
    agrees(&updated, &version, requested, allow_downgrade)?;
    // A rollback is always reported as one, even when the release it returns
    // to is newer than the installed one (after an `--allow-downgrade`).
    if request.rollback && updated.action == "updated" {
        updated.action = "downgraded".to_string();
    }
    let mut notes = Vec::new();
    if updated.action == "current" {
        if let Err(why) = crate::self_check::raise_serial(&receipt, updated.serial) {
            notes.push(format!("the receipt's index serial was not raised: {why}"));
        }
        return Ok(Done {
            updated,
            notes,
            rollback: false,
            keep: request.keep_previous.then_some(None),
        });
    }
    let staged = &staging.path;
    ready(staged, &exe)?;
    let reported = reported_version(staged)?;
    if reported != format!("oskac {}", updated.to) {
        return Err(failed(
            "version_mismatch",
            format!(
                "the {} binary reports '{reported}', not 'oskac {}'; nothing was changed",
                updated.tag, updated.to
            ),
        ));
    }
    let keep: Option<Option<Kept>> =
        request
            .keep_previous
            .then(|| match keep_copy(&exe, &version, &updated.previous) {
                Ok(copy) => Some(copy),
                Err(why) => {
                    notes.push(format!("the replaced oskac was not kept: {why}"));
                    None
                }
            });
    let new_kept = keep.clone().flatten();
    let discard_kept = |path: &Option<Kept>| {
        if let Some(copy) = path {
            remove_kept(copy);
        }
    };
    let record = Records::prepare(
        &receipt,
        &updated,
        &exe,
        old_kept.as_ref(),
        new_kept.as_ref(),
    )
    .map_err(|why| {
        discard_kept(&new_kept);
        failed(
            "replace_failed",
            format!("cannot prepare the receipt: {why}; the installed oskac is as it was"),
        )
    })?;
    replace(staged, &exe).map_err(|e| {
        record.discard();
        discard_kept(&new_kept);
        failed(
            "replace_failed",
            format!(
                "cannot replace {}: {e}; the installed oskac is as it was",
                crate::self_cmd::display(&exe)
            ),
        )
    })?;
    // The new binary is in place: it is what is installed, whatever happens
    // to the records.
    match record.commit() {
        Ok(()) => {
            // A kept copy not kept again goes, now that the receipt no
            // longer records it. (Its name is never the new one's.)
            if let Some(old) = &old_kept {
                if new_kept.as_ref().map(|k| &k.path) != Some(&old.path) {
                    remove_kept(old);
                }
            }
        }
        Err(why) => {
            discard_kept(&new_kept);
            notes.push(format!(
                "oskac was replaced, but the install receipt was not updated ({why}); re-run the installer to repair it"
            ));
        }
    }
    let keep = keep.map(|_| new_kept.filter(|k| k.path.exists()));
    Ok(Done {
        updated,
        notes,
        rollback: request.rollback,
        keep,
    })
}

/// Copy the running binary, which the update checker found to be the
/// published release `version` (state `genuine`, with the digest the signed
/// `SHA256SUMS` lists), to its kept name, and check the copy has that
/// digest. Any other binary is not kept: there is no signed digest to record
/// for it.
fn keep_copy(
    exe: &Path,
    version: &str,
    identity: &update_checker::Identity,
) -> Result<Kept, String> {
    let (true, Some(digest)) = (identity.state == "genuine", identity.sha256.as_deref()) else {
        return Err(
            "it is not a binary the signed SHA256SUMS of its release lists, so no SHA-256 from them can be recorded for it".to_string(),
        );
    };
    let kept = kept_path(exe, version).ok_or("its name is not text")?;
    let tmp = exe.with_file_name(format!(
        ".oskac-keep-{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    let _ = std::fs::remove_file(&tmp);
    crate::crash::track(&tmp);
    let copied = (|| {
        // A running executable can be read on every platform, Windows
        // included, where it can only be renamed, not overwritten.
        std::fs::copy(exe, &tmp).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
        if crate::sha256::hex(&crate::sha256::digest(&bytes)) != digest {
            return Err("the copy's SHA-256 is not the published one".to_string());
        }
        let _ = std::fs::remove_file(&kept);
        std::fs::rename(&tmp, &kept).map_err(|e| format!("{}: {e}", kept.display()))
    })();
    if copied.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    crate::crash::untrack(&tmp);
    copied?;
    Ok(Kept {
        version: version.to_string(),
        path: kept,
        sha256: digest.to_string(),
    })
}

/// The program's answer must be the one that was asked for, whatever it
/// printed: the version it started from, the release named, and an action
/// that is what the two versions and the flags make it.
fn agrees(
    updated: &Updated,
    version: &str,
    requested: Option<&str>,
    allow_downgrade: bool,
) -> Result<(), Failed> {
    let from = crate::semver::parse(&updated.from);
    let to = crate::semver::parse(&updated.to);
    let order = from
        .as_ref()
        .zip(to.as_ref())
        .map(|(from, to)| to.cmp(from));
    use std::cmp::Ordering::{Equal, Greater, Less};
    let ok = updated.from == version
        && requested.map_or(true, |r| r == updated.to)
        && match (updated.action.as_str(), order) {
            ("updated", Some(Greater)) | ("current", Some(Equal)) => true,
            ("downgraded", Some(Less)) => allow_downgrade && requested.is_some(),
            _ => false,
        };
    if ok {
        Ok(())
    } else {
        Err(failed(
            "internal",
            "the update checker's answer is not the one asked for",
        ))
    }
}

/// The staged file must be the file the program wrote: a regular file, not
/// a link. It gets the mode of an installed binary.
fn ready(staged: &Path, live: &Path) -> Result<(), Failed> {
    let meta = std::fs::symlink_metadata(staged).map_err(|e| {
        failed(
            "internal",
            format!("the update checker staged no binary: {e}"),
        )
    })?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(failed(
            "internal",
            "the update checker's staging file is not a binary",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = live;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
            failed(
                "internal",
                format!("cannot make the staged binary executable: {e}"),
            )
        })?;
    }
    #[cfg(not(unix))]
    let _ = live;
    Ok(())
}

/// What the staged binary says `--version` is: its first line. It runs only
/// after the update checker has verified its signature chain and checksum.
fn reported_version(staged: &Path) -> Result<String, Failed> {
    let mut child = Command::new(staged)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            failed(
                "version_mismatch",
                format!(
                    "the new binary cannot be run to read its version: {e}; nothing was changed"
                ),
            )
        })?;
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(stdout) = stdout {
            let _ = stdout.take(4096).read_to_end(&mut out);
        }
        out
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > VERSION_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let out = reader.join().unwrap_or_default();
    let text = String::from_utf8_lossy(&out);
    let first = text.lines().next().unwrap_or("").trim_end_matches('\r');
    if status.is_some_and(|s| s.success()) {
        Ok(first.to_string())
    } else {
        Err(failed(
            "version_mismatch",
            format!(
                "the new binary did not report its version (it said '{first}'); nothing was changed"
            ),
        ))
    }
}

/// Put `staged` where `live` is. The staged file is beside the live one,
/// so this is one rename.
fn replace(staged: &Path, live: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        replace_aside(staged, live)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(staged, live)
    }
}

/// The Windows way: a running executable cannot be overwritten or deleted,
/// but it can be renamed. The live file is renamed aside, the new file is
/// moved to its name, and the old one is deleted when the system allows,
/// which is not while it runs; [`sweep_aside`] deletes it later. If the new
/// file cannot be moved in, the old one is put back.
#[cfg_attr(not(windows), allow(dead_code))]
fn replace_aside(staged: &Path, live: &Path) -> std::io::Result<()> {
    let aside = live.with_file_name(format!(
        "{ASIDE}{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(live, &aside)?;
    if let Err(e) = std::fs::rename(staged, live) {
        let _ = std::fs::rename(&aside, live);
        return Err(e);
    }
    let _ = std::fs::remove_file(&aside);
    Ok(())
}

/// Whether `name` is one `replace_aside` generates: the marker, a process id
/// and the executable suffix, and nothing else.
fn is_aside_name(name: &str) -> bool {
    name.strip_prefix(ASIDE)
        .and_then(|rest| rest.strip_suffix(std::env::consts::EXE_SUFFIX))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Delete what an earlier Windows update renamed aside, now that nothing
/// runs from it. Nothing is kept of a replaced binary. Only a file with a
/// name `replace_aside` generates, beside `exe` and not `exe` itself, is
/// deleted. `self check` and `self update` both do this, so the file is
/// gone at the next `oskac self` command that starts it.
pub(crate) fn sweep_aside(exe: &Path) {
    let Some(dir) = exe.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_aside_name(&name.to_string_lossy()) && entry.path() != exe {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Write `contents` to a new file at `path`, a name this process picked: a
/// file or link already there (left by a process with the same id) is
/// removed first and never written through. The file is removed if `oskac`
/// crashes, until [`crate::crash::untrack`].
fn write_new(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    crate::crash::track(path);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(contents)
}

/// The receipt, and the version file it records, as they are to be after
/// the update: written to files beside the old ones first, so that nothing
/// can fail between the replacement and the final renames but a rename.
struct Records {
    receipt: (PathBuf, PathBuf),
    version: Option<(PathBuf, PathBuf)>,
    /// What the version file held, to put back if the receipt cannot be.
    old_version: Option<Vec<u8>>,
}

impl Records {
    /// `old_kept` is the kept copy the old receipt records, to be dropped
    /// from the new one; `new_kept` the one this update keeps, to be
    /// recorded.
    fn prepare(
        old: &Receipt,
        updated: &Updated,
        exe: &Path,
        old_kept: Option<&Kept>,
        new_kept: Option<&Kept>,
    ) -> Result<Records, String> {
        let digest = updated.sha256.as_deref().ok_or("no digest")?;
        let home = old.path.parent().ok_or("the receipt has no directory")?;
        let mut json = old.json.clone();
        let version_text = format!("{}\n", updated.to);
        let mut version_path = None;
        let Json::Obj(fields) = &mut json else {
            return Err("the receipt is not an object".to_string());
        };
        let old_serial = remembered(old);
        set(fields, "version", Json::Str(updated.to.clone()), None);
        set(
            fields,
            "previous_version",
            Json::Str(updated.from.clone()),
            Some("version"),
        );
        fields.retain(|(k, _)| k != "kept_previous");
        if let Some(kept) = new_kept {
            let text = |s: &str| Json::Str(s.to_string());
            let entry = vec![
                ("version".to_string(), text(&kept.version)),
                ("path".to_string(), text(&kept.path.to_string_lossy())),
                ("sha256".to_string(), text(&kept.sha256)),
            ];
            set(
                fields,
                "kept_previous",
                Json::Obj(entry),
                Some("previous_version"),
            );
        }
        if fields.iter().any(|(k, _)| k == "tag") {
            set(fields, "tag", Json::Str(updated.tag.clone()), None);
        }
        set(fields, "sha256", Json::Str(digest.to_string()), None);
        set(
            fields,
            "index_serial",
            Json::Num(old_serial.max(updated.serial).to_string()),
            None,
        );
        if let Some((_, Json::Arr(files))) = fields.iter_mut().find(|(k, _)| k == "files") {
            // The kept copy the receipt recorded goes; the one kept now is
            // listed as a binary, so that an uninstall removes it, before
            // the receipt itself.
            if let Some(gone) = old_kept {
                let gone = gone.path.to_string_lossy();
                files.retain(|file| {
                    let field = |key: &str| file.get(key).and_then(Json::as_str);
                    !(field("kind") == Some("binary") && field("path") == Some(gone.as_ref()))
                });
            }
            if let Some(kept) = new_kept {
                let text = |s: &str| Json::Str(s.to_string());
                let entry = Json::Obj(vec![
                    ("path".to_string(), text(&kept.path.to_string_lossy())),
                    ("kind".to_string(), text("binary")),
                    ("sha256".to_string(), text(&kept.sha256)),
                ]);
                let at = files
                    .iter()
                    .position(|f| {
                        f.get("kind").and_then(Json::as_str) == Some("record")
                            && f.get("path").and_then(Json::as_str)
                                == Some(old.path.to_string_lossy().as_ref())
                    })
                    .unwrap_or(files.len());
                files.insert(at, entry);
            }
            for file in files {
                let Json::Obj(entry) = file else { continue };
                let text = |key: &str| {
                    entry
                        .iter()
                        .find(|(k, _)| k == key)
                        .and_then(|(_, v)| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                let (kind, path) = (text("kind"), PathBuf::from(text("path")));
                if kind == "binary" && (path == exe || path.canonicalize().is_ok_and(|p| p == exe))
                {
                    set(entry, "sha256", Json::Str(digest.to_string()), None);
                } else if kind == "record"
                    && path.parent() == Some(home)
                    && path.file_name().is_some_and(|n| n == "version")
                    && entry.iter().any(|(k, _)| k == "sha256")
                {
                    let sum = crate::sha256::hex(&crate::sha256::digest(version_text.as_bytes()));
                    set(entry, "sha256", Json::Str(sum), None);
                    version_path = Some(path);
                }
            }
        }
        let text = pretty(&json);
        let parsed = Json::parse(&text)?;
        if parsed != json {
            return Err("the new receipt does not read back as written".to_string());
        }
        let id = std::process::id();
        let receipt_tmp = old.path.with_file_name(format!("receipt.json.{id}.tmp"));
        let mut records = Records {
            receipt: (receipt_tmp, old.path.clone()),
            version: None,
            old_version: None,
        };
        if let Err(e) = write_new(&records.receipt.0, text.as_bytes()) {
            records.discard();
            return Err(format!("{}: {e}", records.receipt.0.display()));
        }
        if let Some(path) = version_path {
            let tmp = path.with_file_name(format!("version.{id}.tmp"));
            records.old_version = std::fs::read(&path).ok();
            records.version = Some((tmp.clone(), path));
            if let Err(e) = write_new(&tmp, version_text.as_bytes()) {
                records.discard();
                return Err(format!("{}: {e}", tmp.display()));
            }
        }
        Ok(records)
    }

    /// Remove the prepared files that were not renamed into place.
    fn discard(&self) {
        let temps = self
            .version
            .iter()
            .map(|(tmp, _)| tmp)
            .chain([&self.receipt.0]);
        for tmp in temps {
            let _ = std::fs::remove_file(tmp);
            crate::crash::untrack(tmp);
        }
    }

    /// Put the version file back as it was, after its rename and before a
    /// failed receipt rename, so the two never disagree.
    fn restore_version(&self) {
        let (Some((_, target)), Some(old)) = (&self.version, &self.old_version) else {
            return;
        };
        let tmp = target.with_file_name(format!("version.{}.old.tmp", std::process::id()));
        let put_back = write_new(&tmp, old).and_then(|()| std::fs::rename(&tmp, target));
        if put_back.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        crate::crash::untrack(&tmp);
    }

    /// Rename the prepared files into place: the version file, then the
    /// receipt, as the installer does. If the receipt cannot be renamed, the
    /// version file is put back, so a failure never leaves the version file
    /// new and the receipt old.
    fn commit(self) -> Result<(), String> {
        if let Some((tmp, target)) = &self.version {
            if let Err(e) = std::fs::rename(tmp, target) {
                self.discard();
                return Err(format!("{}: {e}", target.display()));
            }
        }
        if let Err(e) = std::fs::rename(&self.receipt.0, &self.receipt.1) {
            self.restore_version();
            self.discard();
            return Err(format!("{}: {e}", self.receipt.1.display()));
        }
        self.discard();
        Ok(())
    }
}

/// Set `key` to `value`: in place when the object has it, else as the field
/// after `after` (or last).
fn set(fields: &mut Vec<(String, Json)>, key: &str, value: Json, after: Option<&str>) {
    if let Some((_, slot)) = fields.iter_mut().find(|(k, _)| k == key) {
        *slot = value;
        return;
    }
    let at = after
        .and_then(|a| fields.iter().position(|(k, _)| k == a))
        .map_or(fields.len(), |i| i + 1);
    fields.insert(at, (key.to_string(), value));
}

/// An object as the installer writes one inside the receipt: `{"a": 1, "b":
/// "x"}`.
fn spaced(fields: &[(String, Json)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(k, v)| {
            let mut text = String::new();
            v.write(&mut text);
            format!("{}: {text}", quote(k))
        })
        .collect();
    format!("{{{}}}", parts.join(", "))
}

/// The receipt as the installers lay it out: one field to a line, the
/// `files` entries one to a line.
fn pretty(json: &Json) -> String {
    let Json::Obj(fields) = json else {
        let mut out = String::new();
        json.write(&mut out);
        return out;
    };
    let mut lines = Vec::new();
    for (key, value) in fields {
        let body = match value {
            Json::Arr(items)
                if key == "files" && items.iter().all(|i| matches!(i, Json::Obj(_))) =>
            {
                let entries: Vec<String> = items
                    .iter()
                    .map(|i| match i {
                        Json::Obj(entry) => format!("    {}", spaced(entry)),
                        _ => String::new(),
                    })
                    .collect();
                if entries.is_empty() {
                    "[]".to_string()
                } else {
                    format!("[\n{}\n  ]", entries.join(",\n"))
                }
            }
            Json::Obj(entry) if key == "kept_previous" => spaced(entry),
            other => {
                let mut text = String::new();
                other.write(&mut text);
                text
            }
        };
        lines.push(format!("  {}: {body}", quote(key)));
    }
    format!("{{\n{}\n}}\n", lines.join(",\n"))
}

impl Done {
    fn print(&self, json: bool) {
        let updated = &self.updated;
        if json {
            let list: Vec<String> = updated
                .warnings
                .iter()
                .chain(&self.notes)
                .map(|w| quote(w))
                .collect();
            let mut extra = String::new();
            if self.rollback {
                extra.push_str(&format!(
                    ",\"source\":{}",
                    quote(updated.source.as_deref().unwrap_or("downloaded"))
                ));
            }
            if let Some(kept) = &self.keep {
                let text = kept.as_ref().map_or_else(
                    || "null".to_string(),
                    |k| {
                        format!(
                            "{{\"version\":{},\"path\":{},\"sha256\":{}}}",
                            quote(&k.version),
                            quote(&display(&k.path)),
                            quote(&k.sha256)
                        )
                    },
                );
                extra.push_str(&format!(",\"kept_previous\":{text}"));
            }
            outln!(
                "{{\"ok\":true,\"action\":{},\"from\":{},\"to\":{},\"index_serial\":{},\"site\":{},\"warnings\":[{}]{extra}}}",
                quote(&updated.action),
                quote(&updated.from),
                quote(&updated.to),
                updated.serial,
                quote(SITE),
                list.join(",")
            );
            return;
        }
        let kept_source = updated.source.as_deref() == Some("kept");
        match updated.action.as_str() {
            _ if self.rollback => outln!(
                "rolled back oskac from {} to {} ({})",
                updated.from,
                updated.to,
                if kept_source {
                    "from the copy kept by --keep-previous"
                } else {
                    "downloaded"
                }
            ),
            "updated" => outln!("updated oskac from {} to {}", updated.from, updated.to),
            "downgraded" => outln!(
                "downgraded oskac from {} to {} (you asked for an older release)",
                updated.from,
                updated.to
            ),
            _ => outln!(
                "oskac {} is the latest release; nothing was changed",
                updated.to
            ),
        }
        outln!(
            "release index: serial {}, expires {}, signature verified",
            updated.serial,
            updated.expires
        );
        if kept_source {
            outln!(
                "verified: the kept copy's SHA-256 is the one recorded from the signed SHA256SUMS, and the version it reports"
            );
        } else if updated.action != "current" {
            outln!(
                "verified: the release's SHA256SUMS signature, the binary's SHA-256 and the version it reports"
            );
        }
        if let Some(kept) = &self.keep {
            match kept {
                Some(k) => outln!(
                    "kept oskac {} at {} (SHA-256 {}); the next update discards it unless it also passes --keep-previous",
                    k.version,
                    display(&k.path),
                    k.sha256
                ),
                None => outln!("no copy of the replaced oskac was kept"),
            }
        }
        for warning in updated.warnings.iter().chain(&self.notes) {
            outln!("warning: {warning}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt_text() -> String {
        "{\n  \"schema\": 1,\n  \"version\": \"0.6.0\",\n  \"tag\": \"v0.6.0\",\n  \"sha256\": \"aa\",\n  \"index_serial\": 3,\n  \"files\": [\n    {\"path\": \"/h/bin/oskac\", \"kind\": \"binary\", \"sha256\": \"aa\"},\n    {\"path\": \"/h/receipt.json\", \"kind\": \"record\"}\n  ]\n}\n".to_string()
    }

    #[test]
    fn the_receipt_is_laid_out_as_the_installers_write_it() {
        let text = receipt_text();
        let json = Json::parse(&text).expect("a receipt");
        assert_eq!(pretty(&json), text);
    }

    #[test]
    fn a_field_is_set_in_place_or_after_another() {
        let Json::Obj(mut fields) = Json::parse(&receipt_text()).expect("a receipt") else {
            panic!("an object");
        };
        set(&mut fields, "version", Json::Str("0.7.0".into()), None);
        set(
            &mut fields,
            "previous_version",
            Json::Str("0.6.0".into()),
            Some("version"),
        );
        let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(&keys[..3], ["schema", "version", "previous_version"]);
        assert_eq!(fields[1].1, Json::Str("0.7.0".into()));
    }

    /// A scratch directory removed when it is dropped, a panic included.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("oskac-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a directory");
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The Windows replacement, run here on any platform: the live file
    /// moves aside and the new one takes its name; if the new one cannot be
    /// moved in, the live one is back.
    #[test]
    fn the_aside_replacement_swaps_the_files_and_puts_the_old_one_back_on_failure() {
        let temp = Temp::new("aside");
        let dir = &temp.0;
        let live = dir.join("oskac");
        let staged = dir.join("staged");
        std::fs::write(&live, b"old").expect("live");
        std::fs::write(&staged, b"new").expect("staged");
        replace_aside(&staged, &live).expect("replaced");
        assert_eq!(std::fs::read(&live).expect("live"), b"new");
        assert!(!staged.exists());
        // The old file is not kept.
        let left: Vec<_> = std::fs::read_dir(dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        // A staged file that is not there: the live file stays.
        assert!(replace_aside(&dir.join("missing"), &live).is_err());
        assert_eq!(std::fs::read(&live).expect("live"), b"new");
    }

    #[test]
    fn only_files_renamed_aside_by_an_update_are_swept() {
        let temp = Temp::new("sweep");
        let dir = &temp.0;
        let suffix = std::env::consts::EXE_SUFFIX;
        let exe = dir.join(format!("oskac{suffix}"));
        let aside = dir.join(format!("{ASIDE}1234{suffix}"));
        let others = [
            dir.join(format!("{ASIDE}x{suffix}")),
            dir.join(format!("{ASIDE}1234.bak")),
            dir.join(format!("{ASIDE}{suffix}")),
            dir.join("notes.txt"),
        ];
        for path in others.iter().chain([&exe, &aside]) {
            std::fs::write(path, b"x").expect("a file");
        }
        sweep_aside(&exe);
        assert!(exe.exists());
        assert!(!aside.exists());
        for path in &others {
            assert!(path.exists(), "{} was deleted", path.display());
        }
    }

    /// What Windows needs: the live file is a running executable, which
    /// cannot be overwritten or deleted but can be renamed, so the
    /// replacement still works while it runs, and nothing of the old file is
    /// left once it has stopped and been swept.
    #[cfg(windows)]
    #[test]
    fn a_running_executable_is_replaced_by_renaming_it_aside() {
        let temp = Temp::new("running");
        let dir = &temp.0;
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let live = dir.join("oskac.exe");
        std::fs::copy(Path::new(&system).join(r"System32\ping.exe"), &live).expect("a program");
        let staged = dir.join("staged.exe");
        std::fs::write(&staged, b"new").expect("staged");
        // Running, so that it cannot be overwritten.
        let mut child = Command::new(&live)
            .args(["-n", "6", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .expect("it runs");
        assert!(
            std::fs::write(&live, b"x").is_err(),
            "a running file is writable"
        );
        replace_aside(&staged, &live).expect("replaced while it runs");
        assert_eq!(std::fs::read(&live).expect("live"), b"new");
        assert!(!staged.exists());
        let _ = child.wait();
        sweep_aside(&live);
        let left: Vec<_> = std::fs::read_dir(dir).expect("dir").flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    /// A receipt that cannot be renamed into place puts the version file
    /// back: the two never disagree.
    #[test]
    fn a_failed_receipt_rename_puts_the_version_file_back() {
        let temp = Temp::new("records");
        let dir = &temp.0;
        let version = dir.join("version");
        std::fs::write(&version, "0.6.0\n").expect("version");
        let (receipt_tmp, version_tmp) = (dir.join("r.tmp"), dir.join("v.tmp"));
        std::fs::write(&receipt_tmp, "new receipt").expect("tmp");
        std::fs::write(&version_tmp, "0.7.0\n").expect("tmp");
        // The receipt's place is a directory with something in it, which a
        // file cannot be renamed over.
        let receipt = dir.join("receipt.json");
        std::fs::create_dir(&receipt).expect("a directory");
        std::fs::write(receipt.join("x"), b"x").expect("a file");
        let records = Records {
            receipt: (receipt_tmp.clone(), receipt.clone()),
            version: Some((version_tmp.clone(), version.clone())),
            old_version: Some(b"0.6.0\n".to_vec()),
        };
        assert!(records.commit().is_err());
        assert_eq!(std::fs::read(&version).expect("version"), b"0.6.0\n");
        assert!(!receipt_tmp.exists() && !version_tmp.exists());
        let names: Vec<_> = std::fs::read_dir(dir).expect("dir").flatten().collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    /// A file or link at the name of a temporary file is replaced, never
    /// written through.
    #[cfg(unix)]
    #[test]
    fn a_link_at_a_temporary_name_is_not_written_through() {
        let temp = Temp::new("link");
        let dir = &temp.0;
        let target = dir.join("target");
        std::fs::write(&target, b"keep").expect("target");
        let tmp = dir.join("receipt.json.1.tmp");
        std::os::unix::fs::symlink(&target, &tmp).expect("a link");
        write_new(&tmp, b"new").expect("written");
        assert_eq!(std::fs::read(&target).expect("target"), b"keep");
        assert_eq!(std::fs::read(&tmp).expect("tmp"), b"new");
        crate::crash::untrack(&tmp);
    }
}
