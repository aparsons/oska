//! `oskac release`: build a source file or package for every supported
//! platform into one directory, write a `SHA256SUMS` naming the release,
//! and sign it with the developer's SSH key (spec #155, ticket #180).
//!
//! The directory holds the format `oskac`'s own releases use
//! (`scripts/release/assemble.sh` and `sign.sh`):
//!
//! - one executable per platform, `<name>-<platform>`, with `.exe` for
//!   Windows, as `oskac build --target all` names them;
//! - `SHA256SUMS`, whose first line is `# <name>-release v<version>` and
//!   whose other lines are `sha256sum`'s, `<hex>  <file>`, sorted by file;
//! - `SHA256SUMS.sig`, the signature `ssh-keygen -Y sign` writes, in the
//!   namespace `<name>-release`.
//!
//! The first line binds the name and version into what is signed, so a
//! signed release cannot be passed off as another one. Building and
//! hashing are `oskac`'s own; signing runs the developer's `ssh-keygen`.
//!
//! Every check comes before anything is written: the version
//! (`E0395`, a usage error, exit code 2), the key (`E0396`, exit code 1),
//! the program, and every platform's build. The directory must not exist
//! yet (`E0391`). If writing or signing fails, the directory is removed,
//! so a failed release leaves nothing, never an unsigned release.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use oska_check::program::Program;
use oska_native::Target;
use oska_syntax::{json_escape, Diagnostic, Span};

use crate::cache::Cached;
use crate::{print_diagnostics, sha256, Places};

/// How `oskac release` was asked to release: the output directory `-o`
/// names, the version, and the signing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options<'a> {
    pub output: Option<&'a str>,
    pub version: &'a str,
    pub key: &'a str,
}

/// The first line of a private key in OpenSSH's own format, the one
/// `ssh-keygen` writes and `ssh-keygen -Y sign` reads.
const OPENSSH_PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";

/// Check the version and the key, before anything is read or built:
/// `options` from the command line, where `None` is a flag not given.
pub fn options<'a>(
    output: Option<&'a str>,
    version: Option<&'a str>,
    key: Option<&'a str>,
) -> Result<Options<'a>, (Diagnostic, ExitCode)> {
    let version = match version {
        Some(version) if is_version(version) => version,
        Some(version) => {
            return Err((
                refusal(
                    oska_native::codes::BAD_VERSION,
                    format!(
                        "`{version}` is not a release version; give one as `--version X.Y.Z`, \
                         as in `--version 1.2.0` or `--version 1.2.0-rc.1`"
                    ),
                ),
                ExitCode::from(2),
            ));
        }
        None => {
            return Err((
                refusal(
                    oska_native::codes::BAD_VERSION,
                    "a release needs a version; give one as `--version X.Y.Z`, as in \
                     `--version 1.2.0`"
                        .to_string(),
                ),
                ExitCode::from(2),
            ));
        }
    };
    let Some(key) = key else {
        return Err((
            not_a_key("a release is signed; name your SSH signing key with `--key <file>`"),
            ExitCode::FAILURE,
        ));
    };
    match std::fs::read(key) {
        Ok(bytes) if bytes.starts_with(OPENSSH_PRIVATE_KEY.as_bytes()) => {}
        Ok(_) => {
            return Err((
                not_a_key(&format!(
                    "`{key}` is not an SSH signing key: it is not a private key in OpenSSH's \
                     format; make one with `ssh-keygen -t ed25519 -f <file>`"
                )),
                ExitCode::FAILURE,
            ));
        }
        Err(err) => {
            return Err((
                not_a_key(&format!(
                    "cannot read the signing key `{key}`: {}",
                    reason(&err)
                )),
                ExitCode::FAILURE,
            ));
        }
    }
    Ok(Options {
        output,
        version,
        key,
    })
}

/// Whether `version` is `X.Y.Z` or `X.Y.Z-<pre>`, as `oskac`'s own
/// release tags are without their `v`: numbers with no leading zero, and
/// a pre-release of letters, digits and dots.
fn is_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|n| {
            !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && (n.len() == 1 || !n.starts_with('0'))
        })
        && pre.map_or(true, |pre| {
            !pre.is_empty()
                && pre
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
        })
}

/// Build `program`, named `name`, for every platform, and write, hash and
/// sign the release.
pub fn release(
    program: &Program,
    places: &Places,
    name: &str,
    options: Options,
    json: bool,
    cached: &Cached,
) -> ExitCode {
    let mut executables = Vec::with_capacity(Target::ALL.len());
    for target in Target::ALL {
        match cached.native("build", target, json, || {
            oska_native::build(program, target, &|d| places.locate(d))
        }) {
            Ok(bytes) => {
                let file = format!("{name}-{}{}", target.name(), target.suffix());
                executables.push((file, bytes));
            }
            Err(refusal) => return refuse(&[places.locate(refusal)], json, ExitCode::from(2)),
        }
    }
    executables.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let dir = options
        .output
        .map_or_else(|| format!("{name}-v{}", options.version), str::to_string);
    if Path::new(&dir).exists() {
        return refuse(
            &[unwritable(&dir, "it already exists")],
            json,
            ExitCode::FAILURE,
        );
    }
    let namespace = format!("{name}-release");
    let mut sums = format!("# {namespace} v{}\n", options.version);
    let mut files = Vec::with_capacity(executables.len());
    for (file, bytes) in &executables {
        let hex = sha256::hex(&sha256::digest(bytes));
        sums.push_str(&format!("{hex}  {file}\n"));
        files.push((path_in(&dir, file), hex));
    }
    if let Err(err) = std::fs::create_dir(&dir) {
        return refuse(&[unwritable(&dir, &reason(&err))], json, ExitCode::FAILURE);
    }
    let written = executables
        .iter()
        .try_for_each(|(file, bytes)| crate::build::write_executable(&path_in(&dir, file), bytes))
        .and_then(|()| std::fs::write(Path::new(&dir).join("SHA256SUMS"), &sums));
    if let Err(err) = written {
        let _ = std::fs::remove_dir_all(&dir);
        return refuse(&[unwritable(&dir, &reason(&err))], json, ExitCode::FAILURE);
    }
    if let Err(why) = sign(&dir, options.key, &namespace) {
        let _ = std::fs::remove_dir_all(&dir);
        return refuse(&[not_a_key(&why)], json, ExitCode::FAILURE);
    }
    if json {
        let files: Vec<String> = files
            .iter()
            .map(|(path, hex)| {
                format!(
                    "{{\"path\":\"{}\",\"sha256\":\"{hex}\"}}",
                    json_escape(path)
                )
            })
            .collect();
        outln!(
            "{{\"directory\":\"{}\",\"name\":\"{}\",\"version\":\"{}\",\"files\":[{}],\
             \"checksums\":\"{}\",\"signature\":\"{}\",\"namespace\":\"{}\",\"diagnostics\":[]}}",
            json_escape(&dir),
            json_escape(name),
            json_escape(options.version),
            files.join(","),
            json_escape(&path_in(&dir, "SHA256SUMS")),
            json_escape(&path_in(&dir, "SHA256SUMS.sig")),
            json_escape(&namespace),
        );
    }
    ExitCode::SUCCESS
}

/// `file` in the release directory `dir`, as the user names it.
fn path_in(dir: &str, file: &str) -> String {
    Path::new(dir).join(file).to_string_lossy().into_owned()
}

/// Sign `dir/SHA256SUMS` with the private key `key`, writing
/// `SHA256SUMS.sig`, as `scripts/release/sign.sh` signs `oskac`'s own. A
/// passphrase is asked for on the terminal, as `ssh-keygen` asks for it.
fn sign(dir: &str, key: &str, namespace: &str) -> Result<(), String> {
    let sums: PathBuf = Path::new(dir).join("SHA256SUMS");
    let output = Command::new("ssh-keygen")
        .args(["-q", "-Y", "sign", "-f", key, "-n", namespace])
        .arg(&sums)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                "cannot sign the release: `ssh-keygen` is not installed; install OpenSSH's \
                 client tools"
                    .to_string()
            }
            _ => format!("cannot sign the release: cannot run `ssh-keygen`: {err}"),
        })?;
    if output.status.success() && Path::new(dir).join("SHA256SUMS.sig").is_file() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let said = said.trim();
    Err(if said.is_empty() {
        format!("`ssh-keygen` could not sign with `{key}`")
    } else {
        format!("`ssh-keygen` could not sign with `{key}`: {said}")
    })
}

/// Print a refusal's `diagnostics`, already located, and return `code`.
pub fn refuse(diagnostics: &[Diagnostic], json: bool, code: ExitCode) -> ExitCode {
    if json {
        let entries: Vec<String> = diagnostics.iter().map(Diagnostic::to_json).collect();
        outln!(
            "{{\"directory\":null,\"diagnostics\":[{}]}}",
            entries.join(",")
        );
    } else {
        print_diagnostics(diagnostics);
    }
    code
}

/// The refusal of a release directory that could not be written
/// (`E0391`).
fn unwritable(dir: &str, reason: &str) -> Diagnostic {
    refusal(
        oska_native::codes::UNWRITABLE_OUTPUT,
        format!("cannot write the release `{dir}`: {reason}; name another directory with `-o`"),
    )
}

/// The refusal of a key that cannot sign (`E0396`).
fn not_a_key(message: &str) -> Diagnostic {
    refusal(oska_native::codes::NOT_A_SIGNING_KEY, message.to_string())
}

fn refusal(code: &'static str, message: String) -> Diagnostic {
    Diagnostic::new(code, message, Span { start: 0, end: 0 })
}

/// An I/O error in words of its own, the same on every host.
fn reason(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::is_version;

    #[test]
    fn versions_are_x_y_z_with_an_optional_pre_release() {
        for good in ["0.1.0", "1.2.3", "10.0.20", "1.2.3-rc.1", "1.2.3-beta"] {
            assert!(is_version(good), "{good}");
        }
        for bad in [
            "",
            "1",
            "1.2",
            "v1.2.3",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-a b",
            "1.2.3+build",
            " 1.2.3",
        ] {
            assert!(!is_version(bad), "{bad}");
        }
    }
}
