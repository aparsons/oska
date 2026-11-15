//! Writing an executable a test then runs (#270).
//!
//! Test threads run in parallel, and many of them start processes. A
//! process is started by a fork, and between the fork and its `exec` the
//! child holds a copy of every descriptor the parent has open, one open
//! for writing included. While any copy lives, the kernel refuses to run
//! that file (`ETXTBSY`, "Text file busy"). So a test that writes an
//! executable in its own process and runs it straight away fails now and
//! then, whenever another thread forks in between.

use std::path::Path;
use std::process::Command;

/// Write the executable `bytes` to `path`, executable by everyone, so
/// that running it never fails with `ETXTBSY`. The test process holds no
/// descriptor open for writing to `path`: the bytes go to a scratch file,
/// never run, and `cp`, a process of its own whose descriptors no sibling
/// fork can copy, writes the executable.
pub fn write_executable(path: &Path, bytes: &[u8]) {
    let scratch = path.with_extension("bytes");
    std::fs::write(&scratch, bytes).expect("should write the executable's bytes");
    let _ = std::fs::remove_file(path);
    let copied = Command::new("cp")
        .arg(&scratch)
        .arg(path)
        .status()
        .expect("cp should run");
    assert!(copied.success(), "cp should copy the executable");
    let _ = std::fs::remove_file(&scratch);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("should make it executable");
}
