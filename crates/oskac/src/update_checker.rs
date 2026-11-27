// SPDX-License-Identifier: Apache-2.0
//! The update checker (spec #230): an Oska program, `update/checker.osk`,
//! that `build.rs` compiles with the in-repo checker and native backend
//! when `oskac` is built, so nothing is compiled when `oskac` runs. Its
//! executable is embedded here, for the platform `oskac` is built for.
//!
//! [`check`] runs it for `oskac self check`: it writes the executable to a
//! private directory (mode 0700, under the temporary directory, or in the
//! cache when that cannot run programs), starts it with a cleared
//! environment and its working directory beside the running `oskac`, and
//! reads the one JSON object it prints. Every trust decision is made by
//! the program; this side supplies inputs and parses the verdict strictly:
//! anything it does not understand is [`Failure`], reported as `internal`.
//!
//! Only `self_check` starts it (a test, `network_guard`, keeps it so): the
//! network is used when the user asks.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::json::Json;
use crate::native::Executable;

/// The platform whose executable is embedded, named as `--target` names
/// it (`linux-x86_64`).
#[cfg(test)]
pub const PLATFORM: &str = env!("OSKA_UPDATE_CHECKER_PLATFORM");

/// The update checker's executable for [`PLATFORM`]: the bytes
/// `oskac build --target <platform>` makes of `update/checker.osk` (with
/// `update/test_site.osk` swapped in when the `test-site` feature is on).
pub const EXECUTABLE: &[u8] = include_bytes!(env!("OSKA_UPDATE_CHECKER_BIN"));

/// The longest the program may run, in all. Its own network limits (a
/// connect 10 seconds, a read 60 seconds of silence, a few requests) end
/// it sooner.
const DEADLINE: Duration = Duration::from_secs(240);

/// The most output taken from the program, in bytes.
const MAX_OUTPUT: u64 = 65536;

/// What `self check` asks the update checker.
pub struct Ask<'a> {
    /// This `oskac`'s version.
    pub version: &'a str,
    /// The host clock, in seconds since 1970.
    pub now: u64,
    /// The highest index serial remembered; 0 for none.
    pub remembered: u64,
    /// Whether to identify the running binary.
    pub identify: bool,
    /// The running executable.
    pub exe: &'a Path,
    /// The release file names this build may be, best first.
    pub assets: &'a [String],
}

/// The identity of the running binary, as the program reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// `genuine`, `modified`, `unknown_release` or `skipped`.
    pub state: String,
    pub sha256: Option<String>,
    pub expected_sha256: Option<String>,
    /// Where the expected digest came from.
    pub source: Option<String>,
}

/// A verified index and what was learned from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub latest: String,
    pub tag: String,
    pub serial: u64,
    pub issued: String,
    pub expires: String,
    pub warnings: Vec<String>,
    pub identity: Identity,
}

/// What the program decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Checked(Box<Checked>),
    /// A refusal, with the program's stable code.
    Refused {
        code: String,
        message: String,
    },
}

/// The program could not be run or understood: reported as `internal`.
#[derive(Debug)]
pub struct Failure(pub String);

/// The codes the program may refuse with.
const CODES: [&str; 7] = [
    "bad_index",
    "stale_index",
    "rollback_refused",
    "network_error",
    "tls_error",
    "bad_checksums",
    "internal",
];

fn failure(why: impl Into<String>) -> Failure {
    Failure(why.into())
}

/// The arguments after the program's own name.
fn arguments(ask: &Ask, file: &str) -> Result<Vec<String>, Failure> {
    let mut args = vec![
        "check".to_string(),
        ask.version.to_string(),
        ask.now.to_string(),
        ask.remembered.to_string(),
        if ask.identify { "1" } else { "0" }.to_string(),
        file.to_string(),
        ask.assets.join(","),
    ];
    args.extend(test_site()?);
    Ok(args)
}

/// What the test-only build feature adds: the loopback site, the root the
/// connection trusts and the test key (`update/test_site.osk`). A release
/// build has no such code, and the variables are never read.
#[cfg(feature = "test-site")]
fn test_site() -> Result<Vec<String>, Failure> {
    let var = |name: &str| {
        std::env::var(name).map_err(|_| failure(format!("this test build needs {name}")))
    };
    Ok(vec![
        "127.0.0.1".to_string(),
        var("OSKAC_TEST_SITE_PORT")?,
        var("OSKAC_TEST_SITE_ROOT")?,
        var("OSKAC_TEST_SITE_KEY")?,
    ])
}

#[cfg(not(feature = "test-site"))]
fn test_site() -> Result<Vec<String>, Failure> {
    Ok(Vec::new())
}

/// The current time in seconds since 1970; a test build may fix it
/// (`OSKAC_TEST_NOW`), so that expiry cases do not depend on the day.
pub fn now() -> u64 {
    #[cfg(feature = "test-site")]
    if let Some(fixed) = std::env::var("OSKAC_TEST_NOW")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return fixed;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Where the executable may be put, in order: the temporary directory,
/// then the user's cache, which a `noexec` temporary directory does not
/// reach.
fn bases() -> Vec<std::path::PathBuf> {
    let mut bases = vec![std::env::temp_dir()];
    if let Some(cache) = crate::cache::location() {
        bases.push(cache);
    }
    bases
}

/// Write the executable to a private directory (mode 0700) under the first
/// of `bases` where it can be written and started, and start it. A base
/// that cannot be written to, or that forbids running programs (a spawn
/// refused with a permission error), is passed over for the next.
fn launch(
    bases: &[std::path::PathBuf],
    args: &[String],
    dir: &Path,
) -> Result<(Executable, std::process::Child), Failure> {
    let mut last = String::from("there is no directory to put it in");
    for base in bases {
        if base != &std::env::temp_dir() {
            let _ = std::fs::create_dir_all(base);
        }
        let exe = match Executable::create_in(base, EXECUTABLE) {
            Ok(exe) => exe,
            Err(e) => {
                last = format!("{}: {e}", base.display());
                continue;
            }
        };
        match Command::new(exe.path())
            .args(args)
            .env_clear()
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => return Ok((exe, child)),
            Err(e) => last = format!("{}: {e}", base.display()),
        }
    }
    Err(failure(format!(
        "cannot write and start the update checker in a private directory ({last}); set `{}` to a directory you can run programs from",
        if cfg!(windows) { "TMP" } else { "TMPDIR" }
    )))
}

/// Read up to [`MAX_OUTPUT`] bytes of `source` in a thread.
fn drain<R: Read + Send + 'static>(source: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = source.take(MAX_OUTPUT).read_to_end(&mut out);
        out
    })
}

/// Run the update checker and read its verdict.
pub fn check(ask: &Ask) -> Result<Verdict, Failure> {
    let dir = ask
        .exe
        .parent()
        .ok_or_else(|| failure("the running executable has no directory"))?;
    let file = ask
        .exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| failure("the running executable's name is not text"))?;
    let args = arguments(ask, file)?;
    let (exe, mut child) = launch(&bases(), &args, dir)?;
    // It runs from its file now; on Unix the file can go at once, so that
    // an interrupt leaves nothing behind (Windows removes it on drop).
    #[cfg(unix)]
    exe.unlink();
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure("the update checker ran out of time"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(failure(format!("cannot wait for the update checker: {e}"))),
        }
    };
    let out = stdout.and_then(|t| t.join().ok()).unwrap_or_default();
    let err = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    if !status.success() {
        let said = String::from_utf8_lossy(&err);
        let said = said.trim();
        return Err(failure(if said.is_empty() {
            format!("the update checker ended with {status}")
        } else {
            format!("the update checker ended with {status}: {said}")
        }));
    }
    parse(&out)
}

/// A string, if `value` is one.
fn string(value: Option<&Json>) -> Result<String, Failure> {
    value
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| failure("the update checker's answer is not what was asked"))
}

fn option_string(value: Option<&Json>) -> Result<Option<String>, Failure> {
    match value {
        Some(Json::Null) => Ok(None),
        other => string(other).map(Some),
    }
}

/// The keys of an object, which must be exactly `keys`, in any order.
fn exactly(value: &Json, keys: &[&str]) -> Result<(), Failure> {
    match value {
        Json::Obj(fields)
            if fields.len() == keys.len() && keys.iter().all(|k| value.get(k).is_some()) =>
        {
            Ok(())
        }
        _ => Err(failure("the update checker's answer is not what was asked")),
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The program's output, strictly: one JSON object and a line break.
fn parse(out: &[u8]) -> Result<Verdict, Failure> {
    let text = std::str::from_utf8(out)
        .map_err(|_| failure("the update checker printed text that is not UTF-8"))?;
    let line = text
        .strip_suffix('\n')
        .filter(|l| !l.contains('\n'))
        .ok_or_else(|| failure("the update checker's output is not one line"))?;
    let json = Json::parse(line)
        .map_err(|e| failure(format!("the update checker's output is not JSON: {e}")))?;
    match json.get("ok") {
        Some(Json::Bool(false)) => {
            exactly(&json, &["ok", "code", "message"])?;
            let code = string(json.get("code"))?;
            if !CODES.contains(&code.as_str()) {
                return Err(failure(format!(
                    "the update checker refused with a code it does not have: {code}"
                )));
            }
            Ok(Verdict::Refused {
                code,
                message: string(json.get("message"))?,
            })
        }
        Some(Json::Bool(true)) => {
            exactly(
                &json,
                &[
                    "ok",
                    "latest",
                    "tag",
                    "index_serial",
                    "index_issued",
                    "index_expires",
                    "warnings",
                    "identity",
                ],
            )?;
            let serial = match json.get("index_serial") {
                Some(Json::Num(n)) => n
                    .parse::<u64>()
                    .ok()
                    .filter(|s| (1..=999_999_999_999_999).contains(s)),
                _ => None,
            }
            .ok_or_else(|| failure("the update checker's serial is not a serial"))?;
            let warnings = match json.get("warnings") {
                Some(Json::Arr(items)) => items
                    .iter()
                    .map(|i| string(Some(i)))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(failure("the update checker's warnings are not a list")),
            };
            let who = json
                .get("identity")
                .ok_or_else(|| failure("the update checker gave no identity"))?;
            exactly(who, &["state", "sha256", "expected_sha256", "source"])?;
            let identity = Identity {
                state: string(who.get("state"))?,
                sha256: option_string(who.get("sha256"))?,
                expected_sha256: option_string(who.get("expected_sha256"))?,
                source: option_string(who.get("source"))?,
            };
            if !["genuine", "modified", "unknown_release", "skipped"]
                .contains(&identity.state.as_str())
                || identity
                    .sha256
                    .iter()
                    .chain(&identity.expected_sha256)
                    .any(|d| !is_digest(d))
            {
                return Err(failure("the update checker's identity is not an identity"));
            }
            let checked = Checked {
                latest: string(json.get("latest"))?,
                tag: string(json.get("tag"))?,
                serial,
                issued: string(json.get("index_issued"))?,
                expires: string(json.get("index_expires"))?,
                warnings,
                identity,
            };
            if checked.tag != format!("v{}", checked.latest)
                || crate::semver::parse(&checked.latest).is_none()
            {
                return Err(failure("the update checker's release is not a release"));
            }
            Ok(Verdict::Checked(Box::new(checked)))
        }
        _ => Err(failure("the update checker's answer has no verdict")),
    }
}

/// The checker's source as this build embeds it.
#[cfg(test)]
fn source() -> String {
    #[path = "../update/assemble.rs"]
    mod assemble;
    assemble::checker_source(
        include_str!("../update/checker.osk"),
        include_str!("../update/test_site.osk"),
        cfg!(feature = "test-site"),
    )
}

#[cfg(test)]
mod tests {
    use super::EXECUTABLE;
    use crate::Places;

    /// [`EXECUTABLE`] is what `oskac`'s own pipeline (`oskac build`'s
    /// check and native build, with its places and debug hooks) makes of
    /// the source, not only what `build.rs` made of it.
    #[test]
    fn embedded_executable_equals_oskacs_own_build_of_the_source() {
        let path = "checker.osk";
        let src = &crate::update_checker::source();
        let report = oska_check::check_file(path, src, oska_check::CheckOptions::default());
        assert!(report.diagnostics.is_empty(), "the checker does not check");
        let program = report.program.expect("a program");
        let target = oska_native::Target::named(super::PLATFORM).expect("a supported target");
        let places = Places::File { path, src };
        let bytes = oska_native::build_hooked(
            &program,
            target,
            &places,
            oska_check::limits::Limits::NONE,
            crate::DNS_TEST_HOOKS,
        )
        .expect("the checker builds");
        assert!(
            bytes == EXECUTABLE,
            "EXECUTABLE differs from oskac's own build"
        );
    }

    /// A release has no way to change what it trusts: its checker source
    /// has no trust assertion, no argument that names a site, root or key,
    /// and the pinned key is the line the site publishes.
    #[cfg(not(feature = "test-site"))]
    #[test]
    fn the_release_checker_pins_the_site_the_key_and_the_roots() {
        let src = include_str!("../update/checker.osk");
        assert!(!src.contains("assume_trusted"), "a trust assertion");
        assert!(src.contains("\"oskalang.dev\""));
        assert!(src.contains("x509.TrustConfig.default()"));
        let key = include_str!("../../../site/oska-release.pub");
        assert!(
            src.contains(&format!("\"{}\"", key.trim_end())),
            "the pinned key is not site/oska-release.pub"
        );
        assert!(!super::source().contains("args.get(8)"));
    }

    fn done(identity: &str) -> String {
        format!(
            "{{\"ok\":true,\"latest\":\"0.7.0\",\"tag\":\"v0.7.0\",\"index_serial\":5,\"index_issued\":\"a\",\"index_expires\":\"b\",\"warnings\":[],\"identity\":{identity}}}\n"
        )
    }

    const SKIPPED: &str =
        "{\"state\":\"skipped\",\"sha256\":null,\"expected_sha256\":null,\"source\":null}";

    #[test]
    fn the_verdict_is_read_strictly() {
        use super::{parse, Verdict};
        assert!(matches!(
            parse(done(SKIPPED).as_bytes()),
            Ok(Verdict::Checked(_))
        ));
        let refusal = "{\"ok\":false,\"code\":\"stale_index\",\"message\":\"m\"}\n";
        assert!(matches!(
            parse(refusal.as_bytes()),
            Ok(Verdict::Refused { .. })
        ));
        let bad: Vec<String> = vec![
            String::new(),
            "up-to-date\n".to_string(),
            done(SKIPPED).trim_end().to_string(),
            format!("{}{}", done(SKIPPED), done(SKIPPED)),
            done(SKIPPED).replace("\"skipped\"", "\"fine\""),
            done(SKIPPED).replace("\"latest\"", "\"newest\""),
            done(SKIPPED).replace("v0.7.0", "v0.8.0"),
            done(SKIPPED).replace("\"index_serial\":5", "\"index_serial\":0"),
            done(SKIPPED).replace("\"source\":null", "\"source\":null,\"extra\":1"),
            done("{\"state\":\"genuine\",\"sha256\":\"zz\",\"expected_sha256\":null,\"source\":null}"),
            "{\"ok\":false,\"code\":\"made_up\",\"message\":\"m\"}\n".to_string(),
            "{\"ok\":false,\"code\":\"bad_index\"}\n".to_string(),
        ];
        for text in bad {
            assert!(parse(text.as_bytes()).is_err(), "accepted {text:?}");
        }
    }

    /// The checker runs from a private directory (mode 0700, file 0700),
    /// and a base that cannot hold it, as a temporary directory that is
    /// missing or forbids running programs cannot, is passed over for the
    /// next. (A `noexec` mount cannot be made without privilege, so the
    /// pass-over is exercised with a base that cannot be written.)
    #[cfg(unix)]
    #[test]
    fn it_runs_from_a_private_0700_directory_and_falls_back_to_the_next_base() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| {
            std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777
        };
        let root = std::env::temp_dir().join(format!("oskac-launch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a root");
        // A file where the first base's directory should be: it cannot be made.
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"x").expect("a file");
        let first = blocked.join("under");
        let second = root.join("cache");
        let args = vec!["nothing".to_string()];
        let (exe, mut child) =
            super::launch(&[first, second.clone()], &args, &root).expect("it falls back");
        let dir = exe.path().parent().expect("a directory").to_path_buf();
        assert_eq!(
            dir.parent(),
            Some(second.as_path()),
            "the second base was used"
        );
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(exe.path()), 0o700);
        let _ = child.wait();
        drop(exe);
        assert!(!dir.exists(), "the directory is removed");
        assert!(super::launch(&[blocked.join("a")], &args, &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
