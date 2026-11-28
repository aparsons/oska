// SPDX-License-Identifier: Apache-2.0
//! `oskac lex`: print a file's tokens.

use crate::cli::Flags;
use crate::source::read_source;
use std::process::ExitCode;

pub(crate) fn command(path: &str, Flags { json, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    match oska_syntax::lex(&src) {
        Ok(tokens) => {
            for token in tokens {
                outln!(
                    "{:>5}..{:<5} {:?}",
                    token.span.start,
                    token.span.end,
                    token.kind
                );
            }
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
