// SPDX-License-Identifier: Apache-2.0
//! Compiles the update checker (`update/checker.osk`) with the in-repo
//! checker and native backend for the platform cargo builds `oskac` for,
//! and writes the executable to `$OUT_DIR/update_checker.bin`, which
//! `oskac` embeds (spec #230). No crate from outside the repository.
//!
//! The executable must equal `oskac build --target <platform>` of the
//! same file: the same path as `oskac` is given, the same debug-build
//! DNS hooks, and the same placing of panic sites.

use std::path::PathBuf;

#[path = "update/assemble.rs"]
mod assemble;

use oska_native::{Locate, Target};
use oska_syntax::{Diagnostic, Frame};

/// The source's file name: `oskac build` is run on a file of this name
/// in its directory, so panic sites are placed the same way.
const SOURCE: &str = "checker.osk";

/// Places a panic site in the file, as `oskac build` of a single file does.
struct File<'a> {
    path: &'a str,
    src: &'a str,
}

impl Locate for File<'_> {
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        match diagnostic.location {
            Some(_) => diagnostic,
            None => diagnostic.locate(self.path, self.src),
        }
    }

    fn locate_frame(&self, frame: Frame) -> Frame {
        frame.locate(self.path, self.src)
    }

    fn places(&self) -> bool {
        true
    }
}

fn fail(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

fn main() {
    let dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|e| fail(e.to_string())));
    let source = dir.join("update").join(SOURCE);
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=build.rs");

    let var = |name: &str| std::env::var(name).unwrap_or_else(|e| fail(format!("{name}: {e}")));
    let platform = format!(
        "{}-{}",
        var("CARGO_CFG_TARGET_OS"),
        var("CARGO_CFG_TARGET_ARCH")
    );
    println!("cargo:rustc-env=OSKA_UPDATE_CHECKER_PLATFORM={platform}");
    let target = Target::named(&platform)
        .unwrap_or_else(|d| fail(format!("no update checker for `{platform}`: {}", d.message)));

    let src = std::fs::read_to_string(&source)
        .unwrap_or_else(|e| fail(format!("cannot read {}: {e}", source.display())));
    // The test-only feature swaps the pinned site, key and roots for the
    // loopback test server's (`update/test_site.osk`); a release has none.
    let test_site_path = dir.join("update").join("test_site.osk");
    println!("cargo:rerun-if-changed={}", test_site_path.display());
    println!("cargo:rerun-if-changed=update/assemble.rs");
    let test_site = std::fs::read_to_string(&test_site_path)
        .unwrap_or_else(|e| fail(format!("cannot read {}: {e}", test_site_path.display())));
    let src = assemble::checker_source(
        &src,
        &test_site,
        std::env::var_os("CARGO_FEATURE_TEST_SITE").is_some(),
    );
    let report = oska_check::check_file(SOURCE, &src, oska_check::CheckOptions::default());
    let program = match report
        .program
        .as_ref()
        .filter(|_| report.diagnostics.is_empty())
    {
        Some(program) => program,
        None => fail(format!(
            "the update checker does not check: {:?}",
            report.diagnostics
        )),
    };
    // The hooks are on exactly when `oskac`'s own `cfg!(debug_assertions)` is.
    let hooks = std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some();
    let place = File {
        path: SOURCE,
        src: &src,
    };
    let bytes = oska_native::build_hooked(
        program,
        target,
        &place,
        oska_check::limits::Limits::NONE,
        hooks,
    )
    .unwrap_or_else(|d| fail(format!("the update checker does not build: {}", d.message)));
    let out = PathBuf::from(var("OUT_DIR")).join("update_checker.bin");
    std::fs::write(&out, bytes)
        .unwrap_or_else(|e| fail(format!("cannot write {}: {e}", out.display())));
    println!("cargo:rustc-env=OSKA_UPDATE_CHECKER_BIN={}", out.display());
}
