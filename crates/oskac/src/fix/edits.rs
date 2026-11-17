//! Fix application: a fix's edits applied to the text of one file, with
//! the rules the agent guide states for tools that apply fixes
//! themselves. Offsets count UTF-8 bytes in the text as it was checked;
//! every edit is against that text, so the edits are applied from the end
//! of the file to the start, and none may overlap another.

use oska_syntax::Edit;

/// Why a fix's edits cannot be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// An edit's span ends past the end of the text, starts after it
    /// ends, or falls inside a character.
    OutOfRange,
    /// Two edits cover the same bytes, or insert at the same offset, so
    /// the result would depend on their order.
    Overlap,
}

impl EditError {
    /// What is wrong, for a refusal's message.
    pub fn describe(self) -> &'static str {
        match self {
            EditError::OutOfRange => "an edit is outside the file's text",
            EditError::Overlap => "its edits overlap",
        }
    }
}

/// `text` with every edit of `edits` applied.
pub fn apply(text: &str, edits: &[Edit]) -> Result<String, EditError> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|edit| (edit.span.start, edit.span.end));
    for edit in &sorted {
        let span = edit.span;
        if span.start > span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            return Err(EditError::OutOfRange);
        }
    }
    for pair in sorted.windows(2) {
        let (before, after) = (pair[0].span, pair[1].span);
        let same_insertion = before == after && before.start == before.end;
        if before.end > after.start || same_insertion {
            return Err(EditError::Overlap);
        }
    }
    let mut out = text.to_string();
    for edit in sorted.iter().rev() {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    Ok(out)
}

/// Where byte `offset` of the text before `edits` is in the text after
/// them: moved by every edit that ends at or before it.
pub fn moved(offset: usize, edits: &[Edit]) -> usize {
    edits
        .iter()
        .filter(|edit| edit.span.end <= offset)
        .fold(offset, |at, edit| {
            (at + edit.replacement.len()).saturating_sub(edit.span.end - edit.span.start)
        })
}

#[cfg(test)]
mod tests {
    use super::{apply, moved, EditError};
    use oska_syntax::{Edit, Span};

    fn edit(start: usize, end: usize, replacement: &str) -> Edit {
        Edit {
            span: Span { start, end },
            replacement: replacement.to_string(),
        }
    }

    #[test]
    fn edits_against_the_original_text_apply_in_any_order() {
        let edits = [edit(0, 0, "cap "), edit(9, 10, ""), edit(4, 4, "x")];
        assert_eq!(apply("abcdefghi;j", &edits), Ok("cap abcdxefghij".into()));
    }

    #[test]
    fn offsets_are_bytes_and_must_fall_between_characters() {
        // `é` is two bytes: offset 1 is inside it.
        assert_eq!(apply("é;", &[edit(2, 3, "")]), Ok("é".into()));
        assert_eq!(apply("é;", &[edit(1, 2, "")]), Err(EditError::OutOfRange));
        assert_eq!(apply("ab", &[edit(1, 3, "")]), Err(EditError::OutOfRange));
        assert_eq!(apply("ab", &[edit(2, 1, "")]), Err(EditError::OutOfRange));
    }

    #[test]
    fn overlapping_edits_and_two_insertions_at_one_offset_are_refused() {
        let overlapping = [edit(0, 3, "x"), edit(2, 4, "y")];
        assert_eq!(apply("abcdef", &overlapping), Err(EditError::Overlap));
        let same_place = [edit(1, 1, "x"), edit(1, 1, "y")];
        assert_eq!(apply("abcdef", &same_place), Err(EditError::Overlap));
        // Touching is not overlapping.
        let touching = [edit(0, 2, "x"), edit(2, 2, "y"), edit(2, 3, "z")];
        assert_eq!(apply("abcdef", &touching), Ok("xyzdef".into()));
    }

    #[test]
    fn an_offset_moves_by_the_edits_before_it() {
        let edits = [edit(0, 0, "cap "), edit(5, 6, ""), edit(20, 21, "")];
        assert_eq!(moved(0, &edits), 4);
        assert_eq!(moved(8, &edits), 11);
        assert_eq!(moved(20, &edits), 23);
    }
}
