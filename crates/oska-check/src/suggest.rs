//! "Did you mean" suggestions: the one place that picks the closest known
//! name to an unknown one and turns it into a message and a fix.

use oska_syntax::{Diagnostic, Edit, Span};

/// A diagnostic `code` about the unknown `name` at `span`. When exactly
/// one of `candidates` is close enough, the message asks whether it was
/// meant and the fix replaces `name` with it.
pub fn unknown(
    code: &'static str,
    message: impl Into<String>,
    name: &str,
    span: Span,
    candidates: &[&str],
) -> Diagnostic {
    let pairs: Vec<(&str, &str)> = candidates.iter().map(|c| (*c, *c)).collect();
    unknown_written(code, message, name, span, &pairs)
}

/// [`unknown`], where each candidate is compared by its first element and
/// written by the fix as its second, such as `Empty` written
/// `Shape.Empty`.
pub fn unknown_written(
    code: &'static str,
    message: impl Into<String>,
    name: &str,
    span: Span,
    candidates: &[(&str, &str)],
) -> Diagnostic {
    let message = message.into();
    let names: Vec<&str> = candidates.iter().map(|(n, _)| *n).collect();
    let suggestion = closest(name, &names)
        .and_then(|n| candidates.iter().find(|(c, _)| *c == n))
        .map(|(_, written)| *written);
    match suggestion {
        Some(suggestion) => Diagnostic::new(
            code,
            format!("{message}; did you mean `{suggestion}`?"),
            span,
        )
        .with_fix(
            format!("replace with `{suggestion}`"),
            vec![Edit {
                span,
                replacement: suggestion.to_string(),
            }],
        ),
        None => Diagnostic::new(code, message, span),
    }
}

/// The single candidate closest to `name`, if one is close enough: at
/// most a third of the name's length in edits (at least one), ignoring
/// case. A tie means no suggestion.
fn closest<'c>(name: &str, candidates: &[&'c str]) -> Option<&'c str> {
    let limit = (name.chars().count() / 3).max(1);
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for &candidate in candidates {
        let d = distance(&name.to_lowercase(), &candidate.to_lowercase());
        if d > limit {
            continue;
        }
        match best {
            Some((best_d, best_name)) if d == best_d && best_name != candidate => tied = true,
            Some((best_d, _)) if d >= best_d => {}
            _ => {
                best = Some((d, candidate));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, candidate)| candidate)
    }
}

/// Edit distance, counting an adjacent transposition as one edit.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}
