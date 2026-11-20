//! `oskac release` (spec #155, ticket #180): one directory with an
//! executable for every platform, a `SHA256SUMS` that names the release,
//! and its signature, which `ssh-keygen -Y verify` accepts; and the
//! refusals, which write nothing.
//!
//! Prior art: `scripts/release/test.sh`, which tests the same format for
//! `oskac`'s own releases.

mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::portable::{json_path, Scratch};

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

const MANIFEST: &str = "[package]\nname = \"greeter\"\nversion = \"0.1.0\"\n";

/// The executables of a release of `name`, in `SHA256SUMS` order.
fn executables(name: &str) -> Vec<String> {
    [
        "linux-aarch64",
        "linux-x86_64",
        "macos-aarch64",
        "macos-x86_64",
        "windows-x86_64.exe",
    ]
    .iter()
    .map(|platform| format!("{name}-{platform}"))
    .collect()
}

/// A new, empty directory for `case`, holding `files`, and a fresh
/// ed25519 key without a passphrase at `key` (`key.pub` its public half);
/// or `None` where `ssh-keygen` is not installed, which the test then
/// says, and skips.
fn workspace(case: &str, files: &[(&str, &str)]) -> Option<Scratch> {
    let dir = Scratch::new("oskac-release", case);
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    let made = Command::new("ssh-keygen")
        .args([
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            "release-test",
            "-f",
            "key",
        ])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output();
    match made {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipped: `ssh-keygen` is not installed, and a release needs it");
            None
        }
        made => {
            let made = made.expect("ssh-keygen should run");
            assert!(made.status.success(), "ssh-keygen: {made:?}");
            Some(dir)
        }
    }
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every entry directly in `dir`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("should list the directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The SHA-256 of `path` as 64 lowercase hex digits, by the `sha256sum`
/// or `shasum` of the host, not by `oskac`'s own code.
fn sha256_of(path: &Path) -> String {
    let output = if cfg!(windows) {
        Command::new("certutil")
            .args(["-hashfile"])
            .arg(path)
            .arg("SHA256")
            .output()
    } else if cfg!(target_os = "macos") {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
    } else {
        Command::new("sha256sum").arg(path).output()
    }
    .expect("a hashing tool should run");
    assert!(output.status.success(), "{output:?}");
    let out = text(&output.stdout);
    let hex = if cfg!(windows) {
        // certutil: a heading line, then the hash, then a status line.
        out.lines().nth(1).unwrap_or("").replace(' ', "")
    } else {
        out.split_whitespace().next().unwrap_or("").to_string()
    };
    hex.to_ascii_lowercase()
}

/// Check the signature of `release/SHA256SUMS` as a user would, with the
/// public key in `dir/key.pub` trusted for `identity`.
fn verify(dir: &Path, release: &Path, namespace: &str) -> Output {
    let public = std::fs::read_to_string(dir.join("key.pub")).expect("the public key");
    let allowed = dir.join("allowed");
    std::fs::write(&allowed, format!("{namespace} {public}")).expect("allowed signers");
    let sums = std::fs::File::open(release.join("SHA256SUMS")).expect("SHA256SUMS");
    // Paths relative to the release, as a user gives them: Windows'
    // OpenSSH cannot open a `\\?\` path, which `Scratch` paths are there.
    let allowed = Path::new("..").join(allowed.file_name().expect("a file name"));
    Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(&allowed)
        .args(["-I", namespace, "-n", namespace, "-s", "SHA256SUMS.sig"])
        .current_dir(release)
        .stdin(sums)
        .output()
        .expect("ssh-keygen should run")
}

#[test]
fn a_package_releases_every_platform_with_signed_checksums() {
    let Some(dir) = workspace(
        "package",
        &[
            ("greeter/oska.toml", MANIFEST),
            ("greeter/src/main.osk", HELLO),
        ],
    ) else {
        return;
    };
    let output = oskac(
        &dir,
        &["release", "greeter", "--version", "1.2.0", "--key", "key"],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(text(&output.stdout), "");
    assert_eq!(text(&output.stderr), "");

    let release = dir.join("greeter-v1.2.0");
    let mut expected = executables("greeter");
    expected.extend(["SHA256SUMS".to_string(), "SHA256SUMS.sig".to_string()]);
    expected.sort();
    assert_eq!(listing(&release), expected);

    let sums = std::fs::read_to_string(release.join("SHA256SUMS")).expect("SHA256SUMS");
    let mut lines = sums.lines();
    assert_eq!(lines.next(), Some("# greeter-release v1.2.0"));
    let listed: Vec<&str> = lines.collect();
    let names = executables("greeter");
    assert_eq!(listed.len(), names.len(), "{sums}");
    for (line, name) in listed.iter().zip(&names) {
        let hash = sha256_of(&release.join(name));
        assert_eq!(*line, format!("{hash}  {name}"), "{sums}");
    }
    assert!(sums.ends_with('\n'));

    let verified = verify(&dir, &release, "greeter-release");
    assert!(verified.status.success(), "{verified:?}");
    assert!(
        text(&verified.stdout).contains("Good \"greeter-release\" signature"),
        "{verified:?}"
    );
    // The namespace is the application's: the signature is no use as
    // another application's, or as `oskac`'s own.
    let other = verify(&dir, &release, "oska-release");
    assert!(!other.status.success(), "{other:?}");

    // The executables are the ones `oskac build --target all` writes.
    std::fs::create_dir_all(dir.join("all")).expect("a directory");
    let built = oskac(&dir, &["build", "greeter", "--target", "all", "-o", "all"]);
    assert_eq!(built.status.code(), Some(0), "{built:?}");
    for name in &names {
        let released = std::fs::read(release.join(name)).expect("the release's executable");
        let built = std::fs::read(dir.join("all").join(name)).expect("the built executable");
        assert!(released == built, "{name} differs from `oskac build`'s");
    }
}

#[test]
fn json_reports_the_files_their_checksums_and_the_signature() {
    let Some(dir) = workspace("json", &[("hello.osk", HELLO)]) else {
        return;
    };
    let output = oskac(
        &dir,
        &[
            "release",
            "hello.osk",
            "--version=0.3.0-rc.1",
            "--key=key",
            "-o",
            "out",
            "--format=json",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(text(&output.stderr), "");
    let release = dir.join("out");
    let files: Vec<String> = executables("hello")
        .iter()
        .map(|name| {
            format!(
                "{{\"path\":{},\"sha256\":\"{}\"}}",
                json_path(&Path::new("out").join(name)),
                sha256_of(&release.join(name))
            )
        })
        .collect();
    let path = |file: &str| json_path(&Path::new("out").join(file));
    let expected = format!(
        "{{\"directory\":\"out\",\"name\":\"hello\",\"version\":\"0.3.0-rc.1\",\"files\":[{}],\
         \"checksums\":{},\"signature\":{},\"namespace\":\"hello-release\",\
         \"diagnostics\":[]}}\n",
        files.join(","),
        path("SHA256SUMS"),
        path("SHA256SUMS.sig"),
    );
    assert_eq!(text(&output.stdout), expected);
    let sums = std::fs::read_to_string(release.join("SHA256SUMS")).expect("SHA256SUMS");
    assert!(sums.starts_with("# hello-release v0.3.0-rc.1\n"), "{sums}");
    let verified = verify(&dir, &release, "hello-release");
    assert!(verified.status.success(), "{verified:?}");
}

/// Run a release that must be refused with `code` and `exit`, and check
/// that it wrote nothing: the case's directory holds what it held.
fn assert_refused(case: &str, args: &[&str], code: &str, exit: i32, message: &str) {
    let Some(dir) = workspace(case, &[("hello.osk", HELLO)]) else {
        return;
    };
    let before = listing(&dir);
    let output = oskac(&dir, args);
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.starts_with(&format!("error[{code}]: ")) && stderr.contains(message),
        "{case}: {stderr}"
    );
    assert_eq!(text(&output.stdout), "");
    assert_eq!(listing(&dir), before, "{case} wrote something");

    let mut json = args.to_vec();
    json.push("--format=json");
    let output = oskac(&dir, &json);
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("{\"directory\":null,\"diagnostics\":[{")
            && stdout.contains(&format!("\"code\":\"{code}\"")),
        "{case}: {stdout}"
    );
    assert_eq!(listing(&dir), before, "{case} wrote something");
}

#[test]
fn a_missing_or_malformed_version_is_a_usage_error() {
    assert_refused(
        "no_version",
        &["release", "hello.osk", "--key", "key"],
        "E0395",
        2,
        "a release needs a version",
    );
    assert_refused(
        "bad_version",
        &["release", "hello.osk", "--version", "v1.2", "--key", "key"],
        "E0395",
        2,
        "`v1.2` is not a release version",
    );
}

#[test]
fn a_key_that_cannot_sign_is_refused() {
    assert_refused(
        "no_key",
        &["release", "hello.osk", "--version", "1.0.0"],
        "E0396",
        1,
        "name your SSH signing key with `--key <file>`",
    );
    assert_refused(
        "public_key",
        &[
            "release",
            "hello.osk",
            "--version",
            "1.0.0",
            "--key",
            "key.pub",
        ],
        "E0396",
        1,
        "`key.pub` is not an SSH signing key",
    );
    assert_refused(
        "missing_key",
        &[
            "release",
            "hello.osk",
            "--version",
            "1.0.0",
            "--key",
            "nokey",
        ],
        "E0396",
        1,
        "cannot read the signing key `nokey`: no such file or directory",
    );
}

#[test]
fn a_key_ssh_keygen_cannot_sign_with_leaves_no_release() {
    let Some(dir) = workspace("broken_key", &[("hello.osk", HELLO)]) else {
        return;
    };
    std::fs::write(
        dir.join("broken"),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nnot a key\n-----END OPENSSH PRIVATE KEY-----\n",
    )
    .expect("should write the key");
    let output = oskac(
        &dir,
        &[
            "release",
            "hello.osk",
            "--version",
            "1.0.0",
            "--key",
            "broken",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.starts_with("error[E0396]: `ssh-keygen` could not sign with `broken`"),
        "{stderr}"
    );
    assert!(
        !dir.join("hello-v1.0.0").exists(),
        "an unsigned release was left"
    );
}

#[test]
fn an_existing_directory_is_not_overwritten() {
    let Some(dir) = workspace("exists", &[("hello.osk", HELLO), ("hello-v1.0.0/keep", "")]) else {
        return;
    };
    let output = oskac(
        &dir,
        &["release", "hello.osk", "--version", "1.0.0", "--key", "key"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        text(&output.stderr),
        "error[E0391]: cannot write the release `hello-v1.0.0`: it already exists; name \
         another directory with `-o` (bytes 0..0)\n"
    );
    assert_eq!(listing(&dir.join("hello-v1.0.0")), ["keep"]);
}

#[test]
fn a_program_the_checker_refuses_is_not_released() {
    let Some(dir) = workspace(
        "refused",
        &[("bad.osk", "fn main() {\n    let x: Int = \"no\"\n}\n")],
    ) else {
        return;
    };
    let before = listing(&dir);
    let output = oskac(
        &dir,
        &["release", "bad.osk", "--version", "1.0.0", "--key", "key"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(text(&output.stderr).contains("error[E"), "{output:?}");
    assert_eq!(listing(&dir), before);
}

#[test]
fn only_release_takes_a_key_or_a_version() {
    let Some(dir) = workspace("usage", &[("hello.osk", HELLO)]) else {
        return;
    };
    for args in [
        &["build", "hello.osk", "--key", "key"][..],
        &["check", "hello.osk", "--version=1.0.0"],
        &[
            "release",
            "hello.osk",
            "--version",
            "1.0.0",
            "--key",
            "key",
            "--target",
            "all",
        ],
    ] {
        let output = oskac(&dir, args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(text(&output.stderr).contains("USAGE:"), "{args:?}");
    }
    let version = oskac(&dir, &["--version"]);
    assert_eq!(version.status.code(), Some(0));
    assert!(text(&version.stdout).starts_with("oskac "));
}

#[test]
fn a_name_that_cannot_name_a_release_is_refused() {
    let Some(dir) = workspace("bad_name", &[("my app.osk", HELLO)]) else {
        return;
    };
    let before = listing(&dir);
    let output = oskac(
        &dir,
        &[
            "release",
            "my app.osk",
            "--version",
            "1.0.0",
            "--key",
            "key",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).starts_with("error[E0397]: cannot release `my app`: "),
        "{output:?}"
    );
    assert_eq!(listing(&dir), before);
}

#[test]
fn a_pem_private_key_signs_too() {
    let Some(dir) = workspace("pem", &[("hello.osk", HELLO)]) else {
        return;
    };
    let made = Command::new("ssh-keygen")
        .args([
            "-q", "-t", "ecdsa", "-m", "PEM", "-N", "", "-C", "pem", "-f", "pem",
        ])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("ssh-keygen should run");
    assert!(made.status.success(), "ssh-keygen: {made:?}");
    let key = std::fs::read_to_string(dir.join("pem")).expect("the key");
    assert!(key.starts_with("-----BEGIN EC PRIVATE KEY-----"), "{key}");
    let output = oskac(
        &dir,
        &["release", "hello.osk", "--version", "1.0.0", "--key", "pem"],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    std::fs::copy(dir.join("pem.pub"), dir.join("key.pub")).expect("the public key");
    let verified = verify(&dir, &dir.join("hello-v1.0.0"), "hello-release");
    assert!(verified.status.success(), "{verified:?}");
}
