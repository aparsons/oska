//! **Run limits** (spec #446): the bounds an embedder puts on one `oskac
//! run`, given as flags, which stop the program with a panic of their own
//! and exit code 101 when it reaches one.
//!
//! This module is the one place that knows the limits: their flags, how
//! a flag's value is read, the panic code and message of each, and
//! [`Limits`], the configuration that `oskac` hands to both backends
//! (`oska-run`'s interpreter and `oska-native`'s lowering). To add a
//! limit: add a variant to [`Limit`] (with its flag, code and message), a
//! field to [`Limits`], and its row in `docs/runtime.md`; then enforce it
//! in each backend.
//!
//! A limit is a whole number above 0. A value of 0, a negative number or
//! anything that is not a number is refused, never read as "no limit".

/// One kind of run limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// `--max-output=<bytes>`: the bytes the program may write to
    /// standard output and standard error together.
    Output,
}

impl Limit {
    /// Every limit, in the order the usage and the docs list them.
    pub const ALL: [Limit; 1] = [Limit::Output];

    /// The flag that sets it, without its `=` and value.
    pub fn flag(self) -> &'static str {
        match self {
            Limit::Output => "--max-output",
        }
    }

    /// What its value counts, for a malformed value's error.
    pub fn unit(self) -> &'static str {
        match self {
            Limit::Output => "bytes",
        }
    }

    /// The panic code a program stops with when it reaches the limit.
    pub fn code(self) -> &'static str {
        match self {
            Limit::Output => codes::OUTPUT_LIMIT,
        }
    }

    /// The message of the panic at the limit `value`, on every backend.
    pub fn message(self, value: u64) -> String {
        match self {
            Limit::Output => format!(
                "output limit reached: this write would take standard output and standard \
                 error past {value} bytes (`--max-output={value}`)"
            ),
        }
    }
}

/// The panic codes of the run limits.
pub mod codes {
    /// The program would have written more than `--max-output` bytes to
    /// standard output and standard error together.
    pub const OUTPUT_LIMIT: &str = "P0041";
}

/// The limits one run is under: `None` for each limit not given. With
/// none given ([`Limits::is_empty`]) a run is exactly what it was before
/// limits existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Limits {
    /// [`Limit::Output`].
    pub max_output: Option<u64>,
}

impl Limits {
    /// No limit at all.
    pub const NONE: Limits = Limits { max_output: None };

    /// Whether no limit is given.
    pub fn is_empty(&self) -> bool {
        Limit::ALL.iter().all(|&limit| self.get(limit).is_none())
    }

    /// The value of `limit`, when it is given.
    pub fn get(&self, limit: Limit) -> Option<u64> {
        match limit {
            Limit::Output => self.max_output,
        }
    }

    fn slot(&mut self, limit: Limit) -> &mut Option<u64> {
        match limit {
            Limit::Output => &mut self.max_output,
        }
    }

    /// Read the command-line argument `arg` if it is a limit flag,
    /// `--max-output=<n>`: `None` when it is not one; otherwise the error
    /// that makes it a usage error, if any: a value that is not a whole
    /// number above 0 (up to the largest 64-bit signed integer), or a
    /// limit given twice.
    pub fn take_flag(&mut self, arg: &str) -> Option<Result<(), String>> {
        let (limit, value) = Limit::ALL.iter().find_map(|&limit| {
            let value = arg.strip_prefix(limit.flag())?.strip_prefix('=')?;
            Some((limit, value))
        })?;
        let flag = limit.flag();
        if self.get(limit).is_some() {
            return Some(Err(format!("{flag} is given more than once")));
        }
        match parse_value(value) {
            Some(value) => {
                *self.slot(limit) = Some(value);
                Some(Ok(()))
            }
            None => Some(Err(format!(
                "{flag} takes a whole number of {} above 0, not `{value}`",
                limit.unit()
            ))),
        }
    }

    /// The limits as their flags, in [`Limit::ALL`]'s order: what makes
    /// one run's build differ from another's (the build cache's key).
    pub fn flags(&self) -> Vec<String> {
        Limit::ALL
            .iter()
            .filter_map(|&limit| Some(format!("{}={}", limit.flag(), self.get(limit)?)))
            .collect()
    }
}

/// A limit's value: decimal digits only, above 0, and at most
/// `i64::MAX`, which native code compares it as.
fn parse_value(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: i64 = value.parse().ok()?;
    u64::try_from(value).ok().filter(|&v| v > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_is_a_whole_number_above_zero() {
        assert_eq!(parse_value("1"), Some(1));
        assert_eq!(parse_value("9223372036854775807"), Some(i64::MAX as u64));
        for bad in ["", "0", "-1", "+1", "1.0", " 1", "x", "9223372036854775808"] {
            assert_eq!(parse_value(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_flag_that_is_not_a_limit_is_left_alone() {
        let mut limits = Limits::NONE;
        assert_eq!(limits.take_flag("--max-outputs=1"), None);
        assert_eq!(limits.take_flag("--format=json"), None);
        assert_eq!(limits.take_flag("--max-output=7"), Some(Ok(())));
        assert_eq!(limits.max_output, Some(7));
        assert!(limits
            .take_flag("--max-output=7")
            .is_some_and(|r| r.is_err()));
        assert_eq!(limits.flags(), ["--max-output=7"]);
    }
}
