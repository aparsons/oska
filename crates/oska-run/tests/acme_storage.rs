// SPDX-License-Identifier: Apache-2.0
//! `std.acme` storage (#838, spec #824, ADR 0036): `acme.Store` keeps the
//! account key, the account URL, and each certificate's key and chain in one
//! directory, through a `DirRead` and a `DirWrite` for that directory only.
//! Programs run on `FakeHost` (and natively, in a real sandbox, wherever the
//! native backend runs), with the test CA of `acme_core.rs` replaying a
//! transcript. A restart is a second run on the same host: the CA of the
//! second run has no `newAccount` step, so a program that asked for one
//! would fail.
//!
//! Held here: a first run makes an account and a key file of mode 0600; a
//! restarted run reuses both and asks the CA for neither; a certificate
//! saved by one run is loaded by the next, and reissued when it is near its
//! expiry, or does not name what was asked for; a key file others can read
//! is refused with nothing written; nothing is written outside the
//! directory (a label with a path in it, a symlink in the store, a path
//! that climbs); damaged stored files are `Corrupt`; the key never prints.
//! On a real directory (Unix) the file's real mode is checked, and on
//! Windows, where there is no mode, that a private write works and replaces
//! what is there, as `docs/runtime.md` says.

mod common;

use common::{run_differential, test_fake};
use oska_run::FakeHost;
use oska_run::TestStatus;

const SINGLE_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBmDCCAT+gAwIBAgIUR9SGAGczGqCcf16HKcmczKu6ZJUwCgYIKoZIzj0EAwIw
FjEUMBIGA1UEAwwLZXhhbXBsZS5jb20wHhcNMjYxMTI3MTgyODIxWhcNMjcwMjI1
MTgyODIxWjAWMRQwEgYDVQQDDAtleGFtcGxlLmNvbTBZMBMGByqGSM49AgEGCCqG
SM49AwEHA0IABNoisd0inSbr3linbIsr6J2NUHMNsgSIYp5ZbwCeYk4kQjMtWPeF
cySR8iIGaRWwZjsZSZXe4GuSYCRBmcuNI9ajazBpMB0GA1UdDgQWBBShlusAyeoY
/MPSRNnwW7p43MJkVzAfBgNVHSMEGDAWgBShlusAyeoY/MPSRNnwW7p43MJkVzAP
BgNVHRMBAf8EBTADAQH/MBYGA1UdEQQPMA2CC2V4YW1wbGUuY29tMAoGCCqGSM49
BAMCA0cAMEQCIET+VVsvE2ZyBLBjwQGVMqRjYgpIGFImJG1NiGEvhURDAiB+tMlc
i3yTpqZvMG7Oeiu/TOf9z6/BRBb3n7h+LdIF4Q==
-----END CERTIFICATE-----
";

/// When `SINGLE_PEM` expires: 2027-02-25 18:28:21 UTC.
const NOT_AFTER: i64 = 1_803_580_101;

const PRELUDE: &str = r#"use std.acme
use std.base64url
use std.http

const D: String = "jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI"
const DIR: String = "https://ca.example/dir"
const NEW_NONCE: String = "https://ca.example/nonce"
const NEW_ACCT: String = "https://ca.example/new-acct"
const NEW_ORDER: String = "https://ca.example/new-order"
const ACCT: String = "https://ca.example/acct/1"
const PEM: String = "@PEM@"

fn key() -> Secret<Bytes> {
    Secret.new(base64url.decode(D).unwrap_or(Bytes.new()))
}

// One recorded exchange of the test CA.
struct Step { url: String, post: Bool, status: Int, body: String, next: String, location: String }

fn response_of(s: Step) -> http.Response {
    var headers: List<http.Header> = []
    if s.next != "" {
        headers.push(http.Header(name: "replay-nonce", value: s.next))
    }
    if s.location != "" {
        headers.push(http.Header(name: "Location", value: s.location))
    }
    http.Response(status: s.status, headers: headers, body: s.body.to_bytes())
}

struct Ca { steps: List<Step> }

impl acme.Transport for Ca {
    fn get(self, url: acme.Url) -> Result<http.Response, http.HttpError> {
        for s in self.steps {
            if s.url == url.text && !s.post {
                return Ok(response_of(s))
            }
        }
        Err(http.HttpError.Malformed("no recorded GET"))
    }

    fn post(self, url: acme.Url, body: Bytes) -> Result<http.Response, http.HttpError> {
        if body.to_text().unwrap_or("").contains(D) {
            return Err(http.HttpError.Malformed("the key was sent"))
        }
        for s in self.steps {
            if s.url == url.text && s.post {
                return Ok(response_of(s))
            }
        }
        Err(http.HttpError.Malformed("no recorded POST"))
    }
}

fn directory_step() -> Step {
    let dir = "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\"\}"
    Step(url: DIR, post: false, status: 200, body: dir, next: "", location: "")
}

// The CA's answers to a first start: directory, nonce, new account.
fn first_start() -> Ca {
    Ca(steps: [
        directory_step(),
        Step(url: NEW_NONCE, post: false, status: 204, body: "", next: "n0", location: ""),
        Step(url: NEW_ACCT, post: true, status: 201, body: "\{\"status\":\"valid\"\}", next: "n1", location: ACCT)
    ])
}

// The CA of a restart: only the directory. Any request for an account fails.
fn restart() -> Ca {
    Ca(steps: [directory_step()])
}

// For a run on a real disk, whose output goes to the process's: the line
// is kept in `result.txt` of `dir`.
fn keep(fs: Fs, dir: String, text: Untrusted<String>) -> () {
    let file = fs.restrict_to(dir).write_only().restrict_to_file(Path.new("result.txt"))
    let _ = file.write(text)
}

fn store_in(fs: Fs, name: String) -> acme.Store {
    let dir = fs.restrict_to(name)
    acme.Store.new(dir.read_only(), dir.write_only())
}

fn named(e: acme.StoreError) -> String {
    match e {
        acme.StoreError.Io(_) => "io"
        acme.StoreError.Corrupt(why) => "corrupt: {why}"
        acme.StoreError.InvalidLabel => "invalid label"
        acme.StoreError.Acme(_) => "acme"
    }
}

// The file system's own words, when that is what went wrong.
fn detail(r: Result<(), acme.StoreError>) -> Untrusted<String> {
    match r {
        Err(acme.StoreError.Io(why)) => why
        _ => ""
    }
}

fn unit(r: Result<(), acme.StoreError>) -> String {
    match r {
        Ok(_) => "ok"
        Err(e) => named(e)
    }
}

// Start the client of the store, as a program does at every start, and say
// which account it has.
fn start(ca: Ca, store: acme.Store, random: Random) -> String {
    let directory = match acme.fetch_directory(ca, DIR, false) {
        Ok(d) => d
        Err(_) => { return "no directory" }
    }
    match store.open_client(ca, directory, random, [], true, false) {
        Ok(client) => match client.account_url() {
            Some(u) => "account {u.text}"
            None => "no account"
        }
        Err(e) => named(e)
    }
}

fn loaded(i: acme.Issued) -> String {
    let count = i.chain.certificates.len()
    "loaded {count}"
}

fn issued() -> Result<acme.Issued, String> {
    let response = http.Response(status: 200, headers: [], body: PEM.to_bytes())
    match acme.parse_certificate_chain(response, ["example.com"]) {
        Ok(chain) => Ok(acme.Issued(chain: chain, key: key()))
        Err(_) => Err("no chain")
    }
}
"#;

/// The source of a program whose `main` runs `body`.
fn program(body: &str) -> String {
    let prelude = PRELUDE.replace("@PEM@", &SINGLE_PEM.replace('\n', "\\n"));
    format!(
        "{prelude}\nfn main(io: Stdout, fs: Fs, random: Random) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n"
    )
}

/// What `body` printed on `host`, which keeps what the program wrote.
fn printed(host: &mut FakeHost, body: &str) -> String {
    let src = program(body);
    let outcome = run_differential(host, &src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// The lines of the output, for a test that prints several.
fn lines(out: &str) -> Vec<&str> {
    out.lines().collect()
}

const START: &str = r#"    let store = store_in(fs, "keys")
    io.println(start(@CA@, store, random))?"#;

fn start_body(ca: &str) -> String {
    START.replace("@CA@", ca)
}

// ---- Restarts ---------------------------------------------------------------------------

/// The 32 bytes the programs call `D`, as a key file a first run would have
/// made.
fn seeded_key() -> Vec<u8> {
    let text = "jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI";
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut held = 0;
    let mut out = Vec::new();
    for c in text.bytes() {
        let v = alphabet.iter().position(|a| *a == c).expect("base64url") as u32;
        bits = (bits << 6) | v;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    out
}

/// What `body` printed on `host`, on the interpreter only. A run that makes a
/// key draws it from the generator: the fake host's has a fixed seed and a
/// native executable's comes from the OS, so the two keys differ and the
/// differential comparison of the files would fail for that reason alone.
/// Every other test here seeds its key and is differential.
fn printed_interpreted(host: &mut FakeHost, body: &str) -> String {
    let src = program(body);
    let outcome = oska_run::run(&src, host, &[common::PROGRAM_PATH.to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

#[test]
fn a_first_run_makes_an_account_and_a_restart_reuses_it() {
    let mut host = FakeHost::new().with_dir("keys");
    let out = printed_interpreted(&mut host, &start_body("first_start()"));
    assert_eq!(out, "account https://ca.example/acct/1\n");
    // The account key: 32 bytes, owner only. The URL is not a secret.
    assert_eq!(
        host.file_bytes("keys/account.key").map(<[u8]>::len),
        Some(32)
    );
    assert_eq!(host.file_mode("keys/account.key"), Some(0o600));
    assert_eq!(
        host.file("keys/account.url"),
        Some("https://ca.example/acct/1")
    );
    let key = host.file_bytes("keys/account.key").map(<[u8]>::to_vec);

    // The restart: its CA answers only the directory, so a `newAccount`
    // would fail the program. The same account, and the same key file.
    let out = printed_interpreted(&mut host, &start_body("restart()"));
    assert_eq!(out, "account https://ca.example/acct/1\n");
    assert_eq!(host.file_bytes("keys/account.key").map(<[u8]>::to_vec), key);
    assert_eq!(host.file_mode("keys/account.key"), Some(0o600));
}

#[test]
fn a_restart_from_stored_state_reuses_the_account_and_asks_the_ca_for_none() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &seeded_key())
        .with_file("keys/account.url", "https://ca.example/acct/1");
    let out = printed(&mut host, &start_body("restart()"));
    assert_eq!(out, "account https://ca.example/acct/1\n");
    assert_eq!(host.file_bytes("keys/account.key"), Some(&seeded_key()[..]));
}

#[test]
fn a_stored_key_with_no_stored_account_is_kept_and_its_account_is_asked_for() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &seeded_key());
    let out = printed(&mut host, &start_body("first_start()"));
    assert_eq!(out, "account https://ca.example/acct/1\n");
    assert_eq!(host.file_bytes("keys/account.key"), Some(&seeded_key()[..]));
    assert_eq!(
        host.file("keys/account.url"),
        Some("https://ca.example/acct/1")
    );
}

#[test]
fn a_certificate_saved_by_one_run_is_loaded_by_the_next() {
    let mut host = FakeHost::new().with_dir("keys");
    let save = r#"    let store = store_in(fs, "keys")
    match issued() {
        Ok(i) => io.println(unit(store.save_certificate("site", i)))?
        Err(why) => io.println(why)?
    }"#;
    assert_eq!(printed(&mut host, save), "ok\n");
    assert_eq!(host.file_mode("keys/cert-site.key"), Some(0o600));
    assert_eq!(
        host.file_bytes("keys/cert-site.key").map(<[u8]>::len),
        Some(32)
    );
    assert_eq!(host.file("keys/cert-site.pem"), Some(SINGLE_PEM));
    // The chain is public: it is not made private.
    assert_eq!(host.file_mode("keys/cert-site.pem"), Some(0o644));

    // 2027-01-15 is 41 days before the expiry; renewal is due 30 days before.
    let load = r#"    let store = store_in(fs, "keys")
    let names = ["example.com"]
    match store.load_certificate("site", names, @NOW@, 2592000) {
        Ok(Some(i)) => io.println(loaded(i))?
        Ok(None) => io.println("none")?
        Err(e) => io.println(named(e))?
    }"#;
    let before_window = (NOT_AFTER - 41 * 86400).to_string();
    assert_eq!(
        printed(&mut host, &load.replace("@NOW@", &before_window)),
        "loaded 1\n"
    );
    // Inside the renewal window, or past the expiry: reissue.
    let inside = (NOT_AFTER - 10 * 86400).to_string();
    assert_eq!(
        printed(&mut host, &load.replace("@NOW@", &inside)),
        "none\n"
    );
    let past = (NOT_AFTER + 1).to_string();
    assert_eq!(printed(&mut host, &load.replace("@NOW@", &past)), "none\n");
}

#[test]
fn a_stored_certificate_that_lacks_a_requested_name_is_not_reused() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_file("keys/cert-site.pem", SINGLE_PEM)
        .with_private_file_bytes("keys/cert-site.key", &[7u8; 32]);
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    match store.load_certificate("site", ["example.com", "other.example"], 1, 0) {
        Ok(_) => io.println("loaded")?
        Err(e) => io.println(named(e))?
    }
    match store.load_certificate("missing", ["example.com"], 1, 0) {
        Ok(None) => io.println("none")?
        _ => io.println("something")?
    }"#,
    );
    assert_eq!(
        lines(&out),
        [
            "corrupt: cert-site.pem is not a chain for these names",
            "none"
        ]
    );
}

// ---- Keys are private ---------------------------------------------------------------------

#[test]
fn a_key_file_others_can_read_is_refused_and_left_as_it_was() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_file_bytes("keys/account.key", &[1u8; 32]);
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    let saved = store.save_account_key(key())
    io.println(unit(saved))?
    io.println(detail(saved))?"#,
    );
    assert!(out.starts_with("io\n"), "{out}");
    assert!(out.contains("a private file never is"), "{out}");
    assert_eq!(host.file_bytes("keys/account.key"), Some(&[1u8; 32][..]));
}

#[test]
fn a_private_key_file_is_replaced_in_place_and_stays_private() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &[1u8; 32]);
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    io.println(unit(store.save_account_key(key())))?"#,
    );
    assert_eq!(out, "ok\n");
    assert_eq!(host.file_mode("keys/account.key"), Some(0o600));
    assert_ne!(host.file_bytes("keys/account.key"), Some(&[1u8; 32][..]));
}

#[test]
fn the_account_key_is_never_printed_or_sent() {
    // The test CA refuses a request whose body holds the key's text (`D`),
    // and this program prints everything the store says about itself.
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &seeded_key());
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    io.println(start(first_start(), store, random))?
    let loaded = match store.load_account_key() {
        Ok(Some(_)) => "key"
        Ok(None) => "none"
        Err(e) => named(e)
    }
    io.println(loaded)?"#,
    );
    assert_eq!(out, "account https://ca.example/acct/1\nkey\n");
    let hex: String = seeded_key().iter().map(|b| format!("{b:02x}")).collect();
    assert!(!out.contains(&hex), "{out}");
}

// ---- Nothing outside the directory --------------------------------------------------------

#[test]
fn a_label_with_a_path_in_it_writes_nothing() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_file("outside.txt", "untouched");
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    let labels = ["../outside", "a/b", "", "/etc/x", ".", "..", "a b", "x.y"]
    for label in labels {
        match issued() {
            Ok(i) => io.println(unit(store.save_certificate(label, i)))?
            Err(why) => io.println(why)?
        }
    }
    match store.load_certificate("../outside", ["example.com"], 1, 0) {
        Ok(_) => io.println("loaded")?
        Err(e) => io.println(named(e))?
    }"#,
    );
    assert_eq!(out, "invalid label\n".repeat(9));
    assert_eq!(host.file("outside.txt"), Some("untouched"));
    assert!(
        host.files_under("keys").is_empty(),
        "{:?}",
        host.files_under("keys")
    );
    assert_eq!(host.files_under("").len(), 1);
}

#[test]
fn a_symlink_in_the_store_cannot_lead_a_write_outside() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_file("outside/victim.txt", "untouched")
        .with_symlink("keys/account.key", "outside/victim.txt")
        .with_symlink("keys/cert-site.pem", "outside/victim.txt")
        .with_symlink("keys/cert-site.key", "outside/victim.txt");
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    io.println(unit(store.save_account_key(key())))?
    match issued() {
        Ok(i) => io.println(unit(store.save_certificate("site", i)))?
        Err(why) => io.println(why)?
    }"#,
    );
    for line in out.lines() {
        assert_eq!(line, "io", "{out}");
    }
    assert_eq!(out.lines().count(), 2);
    assert_eq!(host.file("outside/victim.txt"), Some("untouched"));
}

#[test]
fn a_store_cannot_reach_a_sibling_directory() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_file("other/account.key", "not yours");
    let out = printed(
        &mut host,
        r#"    let dir = fs.restrict_to("keys")
    let write = dir.write_only()
    let escaped = write.restrict_to_file(Path.new("../other/account.key"))
    match escaped.write_private_bytes("x".to_bytes()) {
        Ok(_) => io.println("written")?
        Err(_) => io.println("refused")?
    }
    let absolute = write.restrict_to_file(Path.new("/other/account.key"))
    match absolute.write_private_bytes("x".to_bytes()) {
        Ok(_) => io.println("written")?
        Err(_) => io.println("refused")?
    }"#,
    );
    assert_eq!(out, "refused\nrefused\n");
    assert_eq!(host.file("other/account.key"), Some("not yours"));
}

// ---- Damaged state ------------------------------------------------------------------------

#[test]
fn damaged_stored_files_are_corrupt_not_used() {
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &[1u8; 5])
        .with_file("keys/account.url", "not a url");
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    match store.load_account_key() {
        Ok(_) => io.println("loaded")?
        Err(e) => io.println(named(e))?
    }
    match store.load_account_url(false) {
        Ok(_) => io.println("loaded")?
        Err(e) => io.println(named(e))?
    }"#,
    );
    assert_eq!(
        lines(&out),
        [
            "corrupt: account.key is not a 32-byte key",
            "corrupt: account.url is not a valid URL"
        ]
    );
    // A 32-byte file that is not a key (zero is not a valid scalar).
    let mut host = FakeHost::new()
        .with_dir("keys")
        .with_private_file_bytes("keys/account.key", &[0u8; 32]);
    let out = printed(
        &mut host,
        r#"    let store = store_in(fs, "keys")
    match store.load_account_key() {
        Ok(_) => io.println("loaded")?
        Err(e) => io.println(named(e))?
    }"#,
    );
    assert_eq!(out, "corrupt: account.key is not a valid P-256 key\n");
}

// ---- The private write, in a test block ------------------------------------------------

/// `FileWrite.write_private_bytes` in `test` blocks, which run on a fresh
/// fake filesystem (and natively on the test host of `std.rt`): it makes a
/// file, replaces what it made, and refuses a file made by an ordinary
/// write, which is not private.
#[test]
fn a_private_write_in_a_test_makes_replaces_and_refuses() {
    let src = r#"test "a private file is made and replaced" {
    let root = t.fs.restrict_to("/")
    let write = root.write_only()
    let read = root.read_only()
    let file = write.restrict_to_file(Path.new("key.bin"))
    t.assert_eq(file.write_private_bytes("one".to_bytes()).is_ok(), true)
    t.assert_eq(file.write_private_bytes("two".to_bytes()).is_ok(), true)
    let bytes = read.read_bytes(Path.new("key.bin")).unwrap_or("".to_bytes())
    t.assert_eq(bytes.assume_trusted().to_text().unwrap_or("").assume_trusted(), "two")
}

test "a file others can read is refused and kept" {
    let root = t.fs.restrict_to("/")
    let write = root.write_only()
    let read = root.read_only()
    t.assert_eq(write.write(Path.new("plain.txt"), "public").is_ok(), true)
    let file = write.restrict_to_file(Path.new("plain.txt"))
    match file.write_private_bytes("secret".to_bytes()) {
        Ok(_) => t.fail("a file others can read was written")
        Err(e) => t.assert_eq(e.message.assume_trusted().contains("a private file never is"), true)
    }
    t.assert_eq(read.read_to_string(Path.new("plain.txt")).unwrap_or("").assume_trusted(), "public")
}

test "a private file in a missing directory is an error" {
    let root = t.fs.restrict_to("/")
    let file = root.write_only().restrict_to_file(Path.new("nope/key.bin"))
    t.assert_eq(file.write_private_bytes("x".to_bytes()).is_ok(), false)
}
"#;
    let report = test_fake(src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert_eq!(report.tests.len(), 3, "{report:#?}");
    for test in &report.tests {
        assert_eq!(test.status, TestStatus::Passed, "{report:#?}");
    }
}

// ---- Real files ---------------------------------------------------------------------------

/// A scratch directory, canonical and empty, removed on drop; each test
/// gives its own `name`.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("acme-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.canonicalize().expect("canonical scratch dir"))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What `body` kept in `result.txt` of a second scratch directory, on a
/// real disk, with `@DIR@` the scratch directory the store is in.
fn printed_real(dir: &std::path::Path, body: &str) -> String {
    let results = Scratch::new("results");
    let body = body
        .replace("@DIR@", &common::literal(dir))
        .replace("@OUT@", &common::literal(results.path()));
    let src = program(&body);
    let mut host = oska_run::RealHost::new();
    let outcome = oska_run::run(&src, &mut host, &["prog.osk".to_string()]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    std::fs::read_to_string(results.path().join("result.txt")).unwrap_or_default()
}

/// Save the account key into the store at `@DIR@` and keep what happened.
const SAVE_KEY: &str = r#"    let store = store_in(fs, "@DIR@")
    let saved = store.save_account_key(key())
    let status = unit(saved)
    let why = detail(saved)
    keep(fs, "@OUT@", "{status} {why}")"#;

#[cfg(unix)]
mod real {
    use super::{printed_real, Scratch, SAVE_KEY as SAVE};
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).expect("stat").permissions().mode() & 0o777
    }

    #[test]
    fn a_key_file_made_on_a_real_disk_is_readable_by_its_owner_only() {
        let dir = Scratch::new("owner-only");
        let out = printed_real(dir.path(), SAVE);
        assert_eq!(out, "ok ");
        assert_eq!(mode(&dir.path().join("account.key")), 0o600);
        assert_eq!(
            std::fs::read(dir.path().join("account.key"))
                .expect("read")
                .len(),
            32
        );
    }

    #[test]
    fn a_real_key_file_that_others_can_read_is_refused_before_anything_is_written() {
        let dir = Scratch::new("refused");
        let file = dir.path().join("account.key");
        std::fs::write(&file, [9u8; 32]).expect("write");
        for wide in [0o644, 0o640, 0o604, 0o666, 0o660] {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(wide)).expect("chmod");
            let out = printed_real(dir.path(), SAVE);
            assert!(out.contains("a private file never is"), "{wide:o}: {out}");
            assert_eq!(std::fs::read(&file).expect("read"), [9u8; 32]);
            assert_eq!(mode(&file), wide);
        }
        // An owner-only file is replaced in place and stays so.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert_eq!(printed_real(dir.path(), SAVE), "ok ");
        assert_eq!(mode(&file), 0o600);
        assert_ne!(std::fs::read(&file).expect("read"), [9u8; 32]);
    }

    #[test]
    fn a_real_symlink_in_the_store_is_not_followed() {
        let dir = Scratch::new("symlink");
        let outside = Scratch::new("symlink-outside");
        let victim = outside.path().join("victim");
        std::fs::write(&victim, "untouched").expect("write");
        std::os::unix::fs::symlink(&victim, dir.path().join("account.key")).expect("symlink");
        let out = printed_real(dir.path(), SAVE);
        assert!(out.starts_with("io "), "{out}");
        assert_eq!(std::fs::read_to_string(&victim).expect("read"), "untouched");
    }
}

/// Windows has no mode: a private write there creates the file, and
/// replaces one that is there, which keeps the access control list it
/// inherits from its directory (`docs/runtime.md`). So nothing is refused
/// for its permissions, and nothing is promised about them.
#[cfg(windows)]
#[test]
fn on_windows_a_private_write_creates_and_replaces_a_file_and_promises_no_mode() {
    let dir = Scratch::new("windows");
    assert_eq!(printed_real(dir.path(), SAVE_KEY), "ok ");
    assert_eq!(printed_real(dir.path(), SAVE_KEY), "ok ");
    assert_eq!(
        std::fs::read(dir.path().join("account.key"))
            .expect("read")
            .len(),
        32
    );
}
