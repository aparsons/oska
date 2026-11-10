//! Errors and generics at runtime (#63): `Option` and `Result`, `?` with
//! `From` conversions, `main` returning `Err`, trait calls, and generic
//! code dispatching on each value's runtime type tag (ADR 0007).

mod common;

use common::{assert_exits, assert_runs, run_fake};

// ---- `?` and `main` returning `Err` ----------------------------------------

const HALF: &str = "fn half(n: Int) -> Result<Int, String> {
    if n % 2 == 0 { Ok(n / 2) } else { Err(\"{n} is odd\") }
}
";

#[test]
fn question_mark_unwraps_ok_and_main_returning_err_prints_it_and_exits_1() {
    let src = format!(
        "{HALF}
fn main(io: Stdout) -> Result<(), String> {{
    let a = half(8)?
    let _ = io.println(\"half of 8 is {{a}}\")
    let b = half(3)?
    let _ = io.println(\"unreachable {{b}}\")
    Ok(())
}}
"
    );
    assert_exits(&src, 1, "3 is odd\n");
    assert_eq!(run_fake(&src, &[]).stdout, "half of 8 is 4\n");
}

const APP_ERROR: &str = "struct ParseError { text: String }

struct RangeError { n: Int }

enum AppError {
    Parse(String),
    Range(Int),
}

impl From<ParseError> for AppError {
    fn from(err: ParseError) -> AppError { AppError.Parse(err.text) }
}

impl From<RangeError> for AppError {
    fn from(err: RangeError) -> AppError { AppError.Range(err.n) }
}

impl Display for AppError {
    fn to_string(self) -> String {
        match self {
            AppError.Parse(text) => \"not a number: {text}\"
            AppError.Range(n) => \"out of range: {n}\"
        }
    }
}

fn parse(text: String) -> Result<Int, ParseError> {
    match text.parse_int() {
        Some(n) => Ok(n)
        None => Err(ParseError(text: text))
    }
}

fn small(n: Int) -> Result<Int, RangeError> {
    if n < 10 { Ok(n) } else { Err(RangeError(n: n)) }
}

fn read(text: String) -> Result<Int, AppError> {
    let n = parse(text)?
    let m = small(n)?
    Ok(m)
}
";

/// A `main` that reads `text` through [`APP_ERROR`]'s `read`.
fn read_main(text: &str) -> String {
    format!(
        "{APP_ERROR}
fn main(io: Stdout) -> Result<(), AppError> {{
    let n = read(\"{text}\")?
    let _ = io.println(\"read {{n}}\")
    Ok(())
}}
"
    )
}

#[test]
fn question_mark_converts_each_error_through_the_from_impl_for_its_type() {
    assert_runs(&read_main("7"), &[], "read 7\n");
    assert_exits(&read_main("x"), 1, "not a number: x\n");
    assert_exits(&read_main("12"), 1, "out of range: 12\n");
}
