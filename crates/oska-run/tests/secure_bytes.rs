// SPDX-License-Identifier: Apache-2.0
//! `Random.secure_bytes(n)` (#240): `n` secret bytes. On the test host they
//! are fixed, the generator's words little-endian from `FAKE_SEED`, so a
//! recorded handshake replays byte for byte, and the interpreter and the
//! native backend agree on them. A bad length panics as `Bytes.zeroed`'s.

mod common;

use std::path::Path;
use std::process::Command;

use common::{assert_panics, assert_runs, test_fake};
use oska_run::{run, FakeHost, Host, Output, RootHandle, TestStatus, FAKE_SEED};

/// The first `len` bytes the test host's generator gives, as hex.
fn fixed_hex(len: usize) -> String {
    let mut state = FAKE_SEED;
    let mut bytes = Vec::new();
    while bytes.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        bytes.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
    }
    bytes.truncate(len);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_run_gives_the_length_asked_for_in_bytes_that_vary() {
    // A run has no fixed bytes on a real OS; the plain test below holds
    // the test host's.
    let src = "fn main(io: Stdout, random: Random) {\n\
               \x20   let a = random.secure_bytes(300)\n\
               \x20   let b = random.secure_bytes(300)\n\
               \x20   let n = a.len()\n\
               \x20   let _ = io.println(\"{n}\")\n\
               \x20   let same = a.constant_time_eq_secret(b)\n\
               \x20   let _ = io.println(if same { \"same\" } else { \"varied\" })\n\
               }\n";
    assert_runs(src, &[], "300\nvaried\n");
}

#[test]
fn a_plain_test_gets_the_same_fixed_bytes_every_run() {
    let src = format!(
        "test \"fixed\" {{\n\
         \x20   let key = t.random.secure_bytes(8)\n\
         \x20   t.assert_eq(key.expose().to_hex(), \"{}\")\n\
         \x20   let more = t.random.secure_bytes(8)\n\
         \x20   t.assert(!key.constant_time_eq_secret(more))\n\
         }}\n",
        fixed_hex(8)
    );
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert_eq!(report.tests[0].status, TestStatus::Passed, "{report:#?}");
}

#[test]
fn zero_bytes_is_empty() {
    let src = "fn main(io: Stdout, random: Random) {\n\
               \x20   let n = random.secure_bytes(0).len()\n\
               \x20   let _ = io.println(\"{n}\")\n\
               }\n";
    assert_runs(src, &[], "0\n");
}

#[test]
fn a_negative_length_panics_as_zeroed_does() {
    let src = "fn main(random: Random) {\n    let _ = random.secure_bytes(-1)\n}\n";
    assert_panics(src, "P0042", "random.secure_bytes(-1)");
}

#[test]
fn a_length_past_the_limit_panics_as_zeroed_does() {
    let src = "fn main(random: Random) {\n    let _ = random.secure_bytes(268435457)\n}\n";
    assert_panics(src, "P0043", "random.secure_bytes(268435457)");
}

#[test]
fn a_plain_test_gets_many_fixed_bytes_the_same_on_both_backends() {
    // `test_fake` also runs the test natively on the test host, so the
    // native bytes must equal the interpreter's: 300 bytes is 38 words.
    let src = format!(
        "test \"fixed long\" {{\n\
         \x20   let key = t.random.secure_bytes(300)\n\
         \x20   t.assert_eq(key.expose().to_hex(), \"{}\")\n\
         }}\n",
        fixed_hex(300)
    );
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert_eq!(report.tests[0].status, TestStatus::Passed, "{report:#?}");
}

/// A [`FakeHost`] whose operating system has no secure random source.
struct NoSecureSource(FakeHost);

impl Host for NoSecureSource {
    fn write_stdout(&mut self, text: &str) -> std::io::Result<()> {
        self.0.write_stdout(text)
    }
    fn write_stderr(&mut self, text: &str) -> std::io::Result<()> {
        self.0.write_stderr(text)
    }
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        self.0.read_line()
    }
    fn env_var(&mut self, name: &str) -> Option<String> {
        self.0.env_var(name)
    }
    fn now_millis(&mut self) -> i128 {
        self.0.now_millis()
    }
    fn next_random(&mut self) -> u64 {
        self.0.next_random()
    }
    fn secure_bytes(&mut self, _: usize) -> std::io::Result<Vec<u8>> {
        Err(std::io::Error::other("no secure source"))
    }
    fn open_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.0.open_root(path)
    }
    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<RootHandle> {
        self.0.open_beneath(root, names)
    }
    fn open_file_root(&mut self, path: &Path) -> std::io::Result<RootHandle> {
        self.0.open_file_root(path)
    }
    fn open_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<RootHandle> {
        self.0.open_file_beneath(root, names)
    }
    fn read_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<Vec<u8>> {
        self.0.read_file_beneath(root, names)
    }
    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> std::io::Result<()> {
        self.0.write_file_beneath(root, names, contents)
    }
    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> std::io::Result<()> {
        self.0.create_dir_beneath(root, names)
    }
    fn list_dir_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
    ) -> std::io::Result<Vec<String>> {
        self.0.list_dir_beneath(root, names)
    }
    fn printed(&self) -> Vec<String> {
        self.0.printed()
    }
    fn take_output(&mut self) -> Output {
        self.0.take_output()
    }
}

/// The program the failing host runs: it must never print `got bytes`.
const NEEDS_SECURE_BYTES: &str = "fn main(io: Stdout, random: Random) {\n\
    \x20   let key = random.secure_bytes(16)\n\
    \x20   let _ = io.println(\"got bytes\")\n\
    }\n";

#[test]
fn a_host_with_no_secure_source_ends_the_program_with_101_and_no_fallback() {
    // The intrinsic ends the process, so the test runs itself again as a
    // child with `CHILD` set, and looks at how the child ended.
    const CHILD: &str = "OSKA_T240_NO_SECURE_SOURCE";
    if std::env::var_os(CHILD).is_some() {
        let mut host = NoSecureSource(FakeHost::new());
        let outcome = run(NEEDS_SECURE_BYTES, &mut host, &["prog.osk".to_string()]);
        panic!("the program should have ended the process: {outcome:#?}");
    }
    let me = std::env::current_exe().expect("the test binary");
    let output = Command::new(me)
        .args([
            "--exact",
            "a_host_with_no_secure_source_ends_the_program_with_101_and_no_fallback",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("the child should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{stdout}{stderr}");
    assert!(
        stderr.contains("the operating system has no secure random source"),
        "{stderr}"
    );
    assert!(!stdout.contains("got bytes"), "no fallback: {stdout}");
}
