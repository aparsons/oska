// SPDX-License-Identifier: Apache-2.0
//! `oskac parse`: print a file's syntax tree.

use crate::cli::Flags;
use crate::source::read_source;
use std::process::ExitCode;

/// `oskac parse`: print the module's syntax tree, as JSON with
/// `--format=json`. A parse error is printed as diagnostic JSON on stdout
/// (`--format=json`) or as text on stderr, with a non-zero exit code.
pub(crate) fn command(path: &str, Flags { json, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    match oska_syntax::parse_module(&src) {
        Ok(module) if json => {
            out!("{}", oska_syntax::module_to_json(&module, path));
            ExitCode::SUCCESS
        }
        Ok(module) => {
            out!("{}", oska_syntax::module_to_tree(&module));
            ExitCode::SUCCESS
        }
        Err(err) if json => {
            outln!("{}", err.locate(path, &src).to_json());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{path}: {err}");
            ExitCode::FAILURE
        }
    }
}
