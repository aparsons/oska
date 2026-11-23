// SPDX-License-Identifier: Apache-2.0
//! A panic's frames in a human report.

use oska_syntax::Frames;

/// `stderr` without a panic's frames, the lines under its report
/// (`oska_syntax::Frames::human`): what the program wrote, and the
/// report's own line. For a test about the report itself; the
/// differential suite's `assert_same` holds the frames equal all the same.
pub fn without_frames(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| !Frames::is_human_line(line))
        .map(|line| format!("{line}\n"))
        .collect()
}
