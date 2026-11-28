// SPDX-License-Identifier: Apache-2.0
//! Semantic Versioning 2.0.0 precedence, as the installers and `oskac` use
//! it to say whether a release is newer (spec #230): `X.Y.Z` with an
//! optional `-pre-release` and `+build`, the build ignored, a pre-release
//! lower than its release, numeric identifiers below alphanumeric ones.

use std::cmp::Ordering;

/// A parsed version: its numbers and pre-release identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    numbers: [u64; 3],
    pre: Vec<String>,
}

/// Parse `text`, or `None` when it is not a version.
pub fn parse(text: &str) -> Option<Version> {
    let text = text.split_once('+').map_or(
        text,
        |(head, build)| {
            if build.is_empty() {
                "\u{0}"
            } else {
                head
            }
        },
    );
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let mut parts = core.split('.');
    let mut numbers = [0u64; 3];
    for slot in &mut numbers {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    let pre: Vec<String> = match pre {
        None => Vec::new(),
        Some(pre) => pre.split('.').map(str::to_string).collect(),
    };
    for id in &pre {
        let plain = !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let padded = id.len() > 1 && id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit());
        if !plain || padded {
            return None;
        }
    }
    Some(Version { numbers, pre })
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let numeric = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
        let order = match (numeric(x), numeric(y)) {
            (true, true) => x.len().cmp(&y.len()).then_with(|| x.cmp(y)),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn v(text: &str) -> super::Version {
        parse(text).expect("a version")
    }

    #[test]
    fn precedence_follows_the_semver_spec_example() {
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.10.0",
            "2.0.0",
        ];
        for pair in order.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn build_metadata_is_ignored_and_junk_is_refused() {
        assert_eq!(v("1.2.3+a"), v("1.2.3+b"));
        for bad in [
            "", "1", "1.2", "1.2.3.4", "01.2.3", "1.2.3-", "1.2.3-01", "v1.2.3", "1.2.3+",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}
