// SPDX-License-Identifier: Apache-2.0
//! Reading a source file for a command.

pub(crate) fn read_source(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(src) => Some(src),
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            None
        }
    }
}
