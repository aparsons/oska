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
