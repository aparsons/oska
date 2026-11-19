//! The run limits' flags (spec #446): `oskac run --max-output=<bytes>`,
//! read into the [`Limits`] both backends take (`oska_check::limits`,
//! which holds their codes and messages).
//!
//! A limit is a whole number above 0. A value of 0, a negative number or
//! anything that is not a number is refused, never read as "no limit".

use oska_check::limits::{Limit, Limits};

/// The flag that sets `limit`, without its `=` and value.
fn flag(limit: Limit) -> &'static str {
    match limit {
        Limit::Output => "--max-output",
    }
}

/// What `limit`'s value counts, for a malformed value's error.
fn unit(limit: Limit) -> &'static str {
    match limit {
        Limit::Output => "bytes",
    }
}

/// Read the command-line argument `arg` into `limits` if it is a limit
/// flag, `--max-output=<n>`: `None` when it is not one; otherwise the
/// error that makes it a usage error, if any: a value that is not a whole
/// number above 0 (up to the largest 64-bit signed integer), or a limit
/// given twice.
pub fn take_flag(limits: &mut Limits, arg: &str) -> Option<Result<(), String>> {
    let (limit, value) = Limit::ALL.iter().find_map(|&limit| {
        let value = arg.strip_prefix(flag(limit))?.strip_prefix('=')?;
        Some((limit, value))
    })?;
    let name = flag(limit);
    if limits.get(limit).is_some() {
        return Some(Err(format!("{name} is given more than once")));
    }
    match parse_value(value) {
        Some(value) => {
            *limits.get_mut(limit) = Some(value);
            Some(Ok(()))
        }
        None => Some(Err(format!(
            "{name} takes a whole number of {} above 0, not `{value}`",
            unit(limit)
        ))),
    }
}

/// `limits` as their flags, in [`Limit::ALL`]'s order: what makes one
/// run's build differ from another's (the build cache's key).
pub fn flags(limits: &Limits) -> Vec<String> {
    Limit::ALL
        .iter()
        .filter_map(|&limit| Some(format!("{}={}", flag(limit), limits.get(limit)?)))
        .collect()
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
        assert_eq!(take_flag(&mut limits, "--max-outputs=1"), None);
        assert_eq!(take_flag(&mut limits, "--format=json"), None);
        assert_eq!(take_flag(&mut limits, "--max-output=7"), Some(Ok(())));
        assert_eq!(limits.max_output, Some(7));
        assert!(take_flag(&mut limits, "--max-output=7").is_some_and(|r| r.is_err()));
        assert_eq!(flags(&limits), ["--max-output=7"]);
    }
}
