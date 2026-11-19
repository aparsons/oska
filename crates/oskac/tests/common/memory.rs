//! Running `oskac` with a bound on the memory its process may use, to
//! show that a run stays near a limit, as `differential::run_within_memory`
//! does for a built executable.

use std::path::Path;
use std::process::{Command, Output};

/// Run `oskac <args>` in `dir` (with no build cache), and check that no
/// process of the run holds more than `mib` MiB. Linux caps each
/// process's address space with `ulimit -v`, so a run that needs more
/// fails there; macOS cannot (`ulimit -v` fails there), so the peak
/// resident memory `time -l` reports is checked after the run instead.
/// Windows runs it unbounded.
pub fn oskac_within(dir: &Path, args: &[&str], mib: u64) -> Output {
    let oskac = env!("CARGO_BIN_EXE_oskac");
    let mut command = if cfg!(target_os = "linux") {
        let mut sh = Command::new("sh");
        sh.arg("-c")
            .arg(format!("ulimit -v {} && exec \"$0\" \"$@\"", mib * 1024))
            .arg(oskac);
        sh
    } else if cfg!(target_os = "macos") {
        let mut time = Command::new("/usr/bin/time");
        time.arg("-l").arg(oskac);
        time
    } else {
        Command::new(oskac)
    };
    let output = command
        .args(args)
        .arg("--no-cache")
        .current_dir(dir)
        .env_remove("OSKA_CACHE_DIR")
        .output()
        .expect("oskac should run");
    if cfg!(target_os = "macos") {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let peak: u64 = stderr
            .lines()
            .find(|line| line.contains("maximum resident set size"))
            .and_then(|line| line.split_whitespace().next()?.parse().ok())
            .unwrap_or_else(|| panic!("no peak memory in `time -l`'s report: {stderr}"));
        assert!(peak < mib << 20, "peak resident memory {peak} bytes");
    }
    output
}
