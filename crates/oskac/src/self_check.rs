// SPDX-License-Identifier: Apache-2.0
//! `oskac self check [--format=json] [--no-identify]` (spec #230): is there
//! a newer Oska, and is this binary a genuine one? The update checker (an
//! Oska program, `update_checker.rs`) fetches and verifies the signed
//! release index over HTTPS and, unless `--no-identify`, compares this
//! binary's SHA-256 with the release's signed `SHA256SUMS`. This side
//! supplies the inputs, prints the verdict and raises the serial the
//! install receipt remembers; it does no cryptography and no TLS.
//!
//! It exits 0 whether or not an update exists, and 1 when it could not
//! verify an answer (a refusal never looks like "no update"). Its only
//! write is the receipt's `index_serial`, raised after a verified check,
//! atomically, and never lowered.

use crate::json::{quote, quote_option, Json};
use crate::self_cmd::{executable, identity, linux_build, oska_home, platform, read_receipt};
use crate::update_checker::{self, Ask, Checked, Verdict};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// This `oskac`'s version, and in a test build (`test-site`) the one
/// `OSKAC_TEST_VERSION` names, so that the fixtures do not move with every
/// release.
pub(crate) fn version() -> String {
    #[cfg(feature = "test-site")]
    if let Ok(fixed) = std::env::var("OSKAC_TEST_VERSION") {
        return fixed;
    }
    VERSION.to_string()
}

/// The running executable; in a test build `OSKAC_TEST_EXE` stands in for
/// it, a fixture file whose digest the fixture `SHA256SUMS` lists.
pub(crate) fn running() -> Option<PathBuf> {
    #[cfg(feature = "test-site")]
    if let Some(fixed) = std::env::var_os("OSKAC_TEST_EXE") {
        return Some(PathBuf::from(fixed));
    }
    executable()
}

/// The site every update command uses. No flag or variable changes it.
pub(crate) const SITE: &str = "https://oskalang.dev";

/// A failed check: the stable code and what to do about it.
pub(crate) struct Failed {
    pub(crate) code: String,
    pub(crate) message: String,
}

pub(crate) fn failed(code: &str, message: impl Into<String>) -> Failed {
    Failed {
        code: code.to_string(),
        message: message.into(),
    }
}

/// The release files this `oskac` may be, best first: what the installer
/// downloads for this platform (the glibc build also under the name
/// releases before the musl build used).
pub(crate) fn assets() -> Vec<String> {
    #[cfg(feature = "test-site")]
    if let Ok(names) = std::env::var("OSKAC_TEST_ASSETS") {
        return names.split(',').map(str::to_string).collect();
    }
    let platform = platform();
    match linux_build() {
        Some("musl") => vec![format!("oskac-{platform}-musl")],
        Some(build) => vec![
            format!("oskac-{platform}-{build}"),
            format!("oskac-{platform}"),
        ],
        None if cfg!(windows) => vec![format!("oskac-{platform}.exe")],
        None => vec![format!("oskac-{platform}")],
    }
}

/// The install receipt, if there is a readable one: where, what it says,
/// and whether it records this executable.
pub(crate) struct Receipt {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    pub(crate) json: Json,
    pub(crate) managed: bool,
}

pub(crate) fn receipt(exe: &Path) -> Option<Receipt> {
    let path = oska_home()?.join("receipt.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let json = read_receipt(&path).ok()?;
    if !matches!(json, Json::Obj(_)) {
        return None;
    }
    let managed = identity(&json, Some(exe), &version()).is_ok();
    Some(Receipt {
        path,
        text,
        json,
        managed,
    })
}

/// The serial the receipt remembers: 0 when it holds none.
pub(crate) fn remembered(receipt: &Receipt) -> u64 {
    match receipt.json.get("index_serial") {
        Some(Json::Num(n)) => n.parse().unwrap_or(0),
        _ => 0,
    }
}

/// `text` with the receipt's `index_serial` set to `serial`, changing
/// nothing else: the number in place, or a new first field.
fn with_serial(text: &str, serial: u64) -> Option<String> {
    let key = "\"index_serial\"";
    if let Some(at) = text.find(key) {
        let after = at + key.len();
        let rest = &text[after..];
        let colon = rest.find(':')?;
        if !rest[..colon].trim().is_empty() {
            return None;
        }
        let value = &rest[colon + 1..];
        let lead = value.len() - value.trim_start().len();
        let digits = value[lead..].bytes().take_while(u8::is_ascii_digit).count();
        let from = after + colon + 1 + lead;
        return Some(format!(
            "{}{serial}{}",
            &text[..from],
            &text[from + digits..]
        ));
    }
    let open = text.find('{')?;
    Some(format!(
        "{}\n  \"index_serial\": {serial},{}",
        &text[..=open],
        &text[open + 1..]
    ))
}

/// Raise the receipt's serial to `serial` if that is higher: a temporary
/// file in the same directory, then a rename. The new text must read as
/// the old receipt with that one number changed, or nothing is written.
pub(crate) fn raise_serial(receipt: &Receipt, serial: u64) -> Result<(), String> {
    if serial <= remembered(receipt) {
        return Ok(());
    }
    let text = with_serial(&receipt.text, serial).ok_or("the receipt has an unusual layout")?;
    let parsed = Json::parse(&text)?;
    let mut expected = receipt.json.clone();
    if let Json::Obj(fields) = &mut expected {
        let number = Json::Num(serial.to_string());
        match fields.iter_mut().find(|(k, _)| k == "index_serial") {
            Some((_, v)) => *v = number,
            None => fields.insert(0, ("index_serial".to_string(), number)),
        }
    }
    if parsed != expected {
        return Err("the receipt would change in more than its serial".to_string());
    }
    let temp = receipt
        .path
        .with_file_name(format!("receipt.json.{}.tmp", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &receipt.path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

/// A failure of `command` (`check` or `update`), as the stable JSON error or
/// text on standard error, with exit code 1.
pub(crate) fn report_failure(command: &str, json: bool, failure: &Failed) -> ExitCode {
    if json {
        outln!(
            "{{\"ok\":false,\"error\":{},\"message\":{}}}",
            quote(&failure.code),
            quote(&failure.message)
        );
    } else {
        eprintln!("oskac self {command}: {}", failure.message);
    }
    ExitCode::from(1)
}

/// What to run next when an update exists.
pub(crate) fn next_step(managed: bool) -> &'static str {
    if managed {
        "oskac self update"
    } else if cfg!(windows) {
        "& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1)))"
    } else {
        "curl -fsSL https://oskalang.dev/install.sh | sh"
    }
}

/// `oskac self check`.
pub(crate) fn check(json: bool, identify: bool) -> ExitCode {
    match run(identify) {
        Ok(done) => {
            done.print(json);
            ExitCode::SUCCESS
        }
        Err(failure) => report_failure("check", json, &failure),
    }
}

struct Done {
    checked: Box<Checked>,
    version: String,
    comparison: &'static str,
    managed: bool,
    remembered: bool,
    matches_receipt: Option<bool>,
    notes: Vec<String>,
}

fn run(identify: bool) -> Result<Done, Failed> {
    let exe =
        running().ok_or_else(|| failed("internal", "this oskac's own path cannot be found"))?;
    // What an earlier Windows update renamed aside is deleted now.
    crate::self_update::sweep_aside(&exe);
    let receipt = receipt(&exe);
    let assets = assets();
    let version = version();
    let ask = Ask {
        version: &version,
        now: update_checker::now(),
        remembered: receipt.as_ref().map_or(0, remembered),
        identify,
        exe: &exe,
        assets: &assets,
    };
    let checked = match update_checker::check(&ask) {
        Ok(Verdict::Checked(checked)) => checked,
        Ok(Verdict::Refused { code, message }) => return Err(Failed { code, message }),
        Err(why) => return Err(failed("internal", why.0)),
    };
    if checked.identity.state == "skipped" && identify
        || checked.identity.state != "skipped" && !identify
    {
        return Err(failed(
            "internal",
            "the update checker's identity is not the one asked for",
        ));
    }
    let installed = crate::semver::parse(&version)
        .ok_or_else(|| failed("internal", "this oskac's version is not a version"))?;
    let latest = crate::semver::parse(&checked.latest)
        .ok_or_else(|| failed("internal", "the latest release is not a version"))?;
    let comparison = match installed.cmp(&latest) {
        Ordering::Less => "behind",
        Ordering::Equal => "current",
        Ordering::Greater => "ahead",
    };
    let mut notes = Vec::new();
    if let Some(receipt) = &receipt {
        if let Err(why) = raise_serial(receipt, checked.serial) {
            notes.push(format!("the receipt's index serial was not raised: {why}"));
        }
    }
    let matches_receipt = receipt
        .as_ref()
        .and_then(|r| {
            r.json
                .get("sha256")
                .and_then(Json::as_str)
                .map(str::to_string)
        })
        .zip(checked.identity.sha256.clone())
        .map(|(recorded, computed)| recorded == computed);
    Ok(Done {
        version,
        managed: receipt.as_ref().is_some_and(|r| r.managed),
        remembered: receipt.is_some(),
        comparison,
        matches_receipt,
        notes,
        checked,
    })
}

fn optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

impl Done {
    fn print(&self, json: bool) {
        let checked = &self.checked;
        let version = &self.version;
        let who = &checked.identity;
        let mut warnings = checked.warnings.clone();
        warnings.extend(self.notes.iter().cloned());
        if json {
            let list: Vec<String> = warnings.iter().map(|w| quote(w)).collect();
            outln!(
                "{{\"ok\":true,\"action\":\"checked\",\"installed\":{},\"latest\":{},\"tag\":{},\"comparison\":{},\"update_available\":{},\"index_serial\":{},\"index_expires\":{},\"serial_remembered\":{},\"site\":{},\"warnings\":[{}],\"identity\":{{\"state\":{},\"sha256\":{},\"expected_sha256\":{},\"source\":{},\"matches_receipt\":{}}}}}",
                quote(version),
                quote(&checked.latest),
                quote(&checked.tag),
                quote(self.comparison),
                self.comparison == "behind",
                checked.serial,
                quote(&checked.expires),
                self.remembered,
                quote(SITE),
                list.join(","),
                quote(&who.state),
                quote_option(who.sha256.as_deref()),
                quote_option(who.expected_sha256.as_deref()),
                quote_option(who.source.as_deref()),
                optional_bool(self.matches_receipt),
            );
            return;
        }
        outln!("installed: {version}");
        outln!("latest: {} ({})", checked.latest, checked.tag);
        match self.comparison {
            "behind" => outln!("an update is available"),
            "current" => outln!("up to date"),
            _ => outln!("ahead of the latest release; nothing to update"),
        }
        outln!(
            "release index: serial {}, expires {}, signature verified",
            checked.serial,
            checked.expires
        );
        if !self.remembered {
            outln!("serial remembered: no (this install has no receipt, so an older index is not refused; only the expiry bounds a freeze)");
        }
        match who.state.as_str() {
            "genuine" => outln!("this oskac is genuine: its SHA-256 is the one the signed SHA256SUMS of v{version} lists"),
            "modified" => outln!(
                "this oskac is modified: {version} is a published release, but this file's SHA-256 differs (computed {}, signed {})",
                who.sha256.as_deref().unwrap_or("unknown"),
                who.expected_sha256.as_deref().unwrap_or("unknown")
            ),
            "unknown_release" => outln!("this oskac is not a published release (a source build or an unpublished version), so it cannot be compared"),
            _ => outln!("identity: skipped (--no-identify)"),
        }
        if let Some(sha) = &who.sha256 {
            outln!("sha256: {sha}");
        }
        if let Some(matches) = self.matches_receipt {
            outln!(
                "matches the install receipt: {}",
                if matches {
                    "yes"
                } else {
                    "no (the file changed since it was installed)"
                }
            );
        }
        for warning in &warnings {
            outln!("warning: {warning}");
        }
        if self.comparison == "behind" {
            outln!("to update, run: {}", next_step(self.managed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::with_serial;

    #[test]
    fn the_serial_is_changed_in_place_and_nothing_else() {
        let text = "{\n  \"schema\": 1,\n  \"index_serial\": 3,\n  \"files\": []\n}\n";
        assert_eq!(
            with_serial(text, 12).as_deref(),
            Some("{\n  \"schema\": 1,\n  \"index_serial\": 12,\n  \"files\": []\n}\n")
        );
        let compact = "{\"index_serial\":0,\"version\":\"1\"}";
        assert_eq!(
            with_serial(compact, 7).as_deref(),
            Some("{\"index_serial\":7,\"version\":\"1\"}")
        );
    }

    #[test]
    fn a_receipt_with_no_serial_gets_one_as_its_first_field() {
        assert_eq!(
            with_serial("{\"version\": \"1\"}", 4).as_deref(),
            Some("{\n  \"index_serial\": 4,\"version\": \"1\"}")
        );
    }

    /// "The network is used only when the user asks" is enforced, not just
    /// promised (spec #230): the update checker is started from `self
    /// check` and `self update` alone, and nothing else in `oskac` opens a
    /// socket.
    #[test]
    fn only_self_check_and_self_update_start_the_update_checker_and_no_code_opens_a_socket() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut seen = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("the source directory") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    seen.push(path);
                }
            }
        }
        assert!(seen.len() > 20, "the source files were not found");
        for path in seen {
            let name = path
                .strip_prefix(&src)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).expect("a source file");
            // This file's own test text names what it looks for.
            let text = text.split("#[cfg(test)]").next().unwrap_or_default();
            let starts = text.contains("update_checker::");
            let allowed =
                ["self_check.rs", "self_update.rs", "update_checker.rs"].contains(&name.as_str());
            assert!(
                !starts || allowed,
                "{name} reaches the update checker: only `oskac self check` and `self update` may start it"
            );
            let sockets = text.contains("std::net") || text.contains("TcpStream");
            assert!(
                !sockets,
                "{name} opens a socket: oskac uses no network itself"
            );
        }
    }
}
