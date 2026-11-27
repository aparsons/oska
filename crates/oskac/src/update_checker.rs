// SPDX-License-Identifier: Apache-2.0
//! The update checker (spec #230): an Oska program, `update/checker.osk`,
//! that `build.rs` compiles with the in-repo checker and native backend
//! when `oskac` is built, so nothing is compiled when `oskac` runs. Its
//! executable is embedded here, for the platform `oskac` is built for.

/// The platform whose executable is embedded, named as `--target` names
/// it (`linux-x86_64`).
// Nothing runs the checker yet: #258 adds the code that does.
#[allow(dead_code)]
pub const PLATFORM: &str = env!("OSKA_UPDATE_CHECKER_PLATFORM");

/// The update checker's executable for [`PLATFORM`]: the bytes
/// `oskac build --target <platform>` makes of `update/checker.osk`.
#[allow(dead_code)]
pub const EXECUTABLE: &[u8] = include_bytes!(env!("OSKA_UPDATE_CHECKER_BIN"));

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
        let src = include_str!("../update/checker.osk");
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
}
