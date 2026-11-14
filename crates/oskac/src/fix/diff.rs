//! A unified diff of a file's text before and after `oskac fix`, as
//! `oskac fix --dry-run` prints it, with three lines of context and the
//! file's path in both headers, so `patch -p0` applies it.

/// Lines of context around each change.
const CONTEXT: usize = 3;

/// One line of the diff, by its index in the old or the new text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same(usize, usize),
    Removed(usize),
    Added(usize),
}

/// The unified diff from `old` to `new`, the texts of the file at
/// `path`: empty when they are the same.
pub fn unified(path: &str, old: &str, new: &str) -> String {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let ops = line_ops(&a, &b);
    let changed: Vec<usize> = (0..ops.len())
        .filter(|&i| !matches!(ops[i], Op::Same(..)))
        .collect();
    if changed.is_empty() {
        return String::new();
    }
    let mut out = format!("--- {path}\n+++ {path}\n");
    let mut at = 0;
    while at < changed.len() {
        // A hunk runs from the context before one change to the context
        // after the last change whose context meets the one before's.
        let first = changed[at];
        let mut last = first;
        while at + 1 < changed.len() && changed[at + 1] - last <= 2 * CONTEXT + 1 {
            at += 1;
            last = changed[at];
        }
        let from = first.saturating_sub(CONTEXT);
        let to = (last + CONTEXT + 1).min(ops.len());
        out.push_str(&hunk(&ops[from..to], &ops[..from], &a, &b));
        at += 1;
    }
    out
}

/// One hunk: its header and lines. `before` is every op before it, which
/// says where it starts.
fn hunk(ops: &[Op], before: &[Op], a: &[&str], b: &[&str]) -> String {
    let old_before = before
        .iter()
        .filter(|op| !matches!(op, Op::Added(_)))
        .count();
    let new_before = before
        .iter()
        .filter(|op| !matches!(op, Op::Removed(_)))
        .count();
    let old_count = ops.iter().filter(|op| !matches!(op, Op::Added(_))).count();
    let new_count = ops
        .iter()
        .filter(|op| !matches!(op, Op::Removed(_)))
        .count();
    // An empty side starts at the line before it, as `diff -u` writes it.
    let start = |before: usize, count: usize| if count == 0 { before } else { before + 1 };
    let mut out = format!(
        "@@ -{},{old_count} +{},{new_count} @@\n",
        start(old_before, old_count),
        start(new_before, new_count)
    );
    for op in ops {
        let (mark, line) = match *op {
            Op::Same(i, _) => (' ', a[i]),
            Op::Removed(i) => ('-', a[i]),
            Op::Added(j) => ('+', b[j]),
        };
        out.push(mark);
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    }
    out
}

/// The lines of `a` and `b` as kept, removed and added, in order: the
/// common start and end, and the shortest edit between them (Myers).
fn line_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut ops: Vec<Op> = (0..prefix).map(|i| Op::Same(i, i)).collect();
    let middle = shortest_edit(&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    ops.extend(middle.into_iter().map(|op| match op {
        Op::Same(i, j) => Op::Same(prefix + i, prefix + j),
        Op::Removed(i) => Op::Removed(prefix + i),
        Op::Added(j) => Op::Added(prefix + j),
    }));
    let (a_end, b_end) = (a.len() - suffix, b.len() - suffix);
    ops.extend((0..suffix).map(|k| Op::Same(a_end + k, b_end + k)));
    ops
}

/// The shortest edit from `a` to `b`, by Myers' algorithm: for each
/// number of edits `d`, the furthest each diagonal reaches, then a walk
/// back from the end.
fn shortest_edit(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    let max = n + m;
    let offset = max + 1;
    let mut v = vec![0usize; 2 * max + 3];
    let mut trace: Vec<Vec<usize>> = Vec::new();
    'search: for d in 0..=max {
        trace.push(v.clone());
        for k in (0..=2 * d).step_by(2) {
            // Diagonal `k - d`, stored at `offset + k - d`.
            let at = offset + k - d;
            let mut x = if k == 0 || (k != 2 * d && v[at - 1] < v[at + 1]) {
                v[at + 1]
            } else {
                v[at - 1] + 1
            };
            let mut y = (x + d).saturating_sub(k);
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            v[at] = x;
            if x >= n && y >= m {
                break 'search;
            }
        }
    }
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for (d, v) in trace.iter().enumerate().rev() {
        // `x - y` is the diagonal; `k` the same one counted from `-d`.
        let k = x + d - y;
        let at = offset + k - d;
        let previous = if k == 0 || (k != 2 * d && v[at - 1] < v[at + 1]) {
            at + 1
        } else {
            at - 1
        };
        let previous_x = v[previous];
        let previous_y = (previous_x + offset).saturating_sub(previous);
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Same(x, y));
        }
        if d > 0 {
            if x == previous_x {
                y -= 1;
                ops.push(Op::Added(y));
            } else {
                x -= 1;
                ops.push(Op::Removed(x));
            }
        }
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::unified;

    #[test]
    fn the_same_text_has_no_diff() {
        assert_eq!(unified("a.osk", "x\ny\n", "x\ny\n"), "");
    }

    #[test]
    fn a_changed_line_shows_with_three_lines_of_context() {
        let old = "1\n2\n3\n4;\n5\n6\n7\n8\n";
        let new = "1\n2\n3\n4\n5\n6\n7\n8\n";
        assert_eq!(
            unified("a.osk", old, new),
            "--- a.osk\n+++ a.osk\n@@ -1,7 +1,7 @@\n 1\n 2\n 3\n-4;\n+4\n 5\n 6\n 7\n"
        );
    }

    #[test]
    fn changes_far_apart_are_separate_hunks_and_near_ones_join() {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let new = old
            .replace("\n2\n", "\ntwo\n")
            .replace("\n12\n", "\n12a\n12b\n");
        // `2` and `12` are ten lines apart: two hunks.
        assert_eq!(
            unified("f", &old, &new),
            "--- f\n+++ f\n\
             @@ -1,5 +1,5 @@\n 1\n-2\n+two\n 3\n 4\n 5\n\
             @@ -9,7 +9,8 @@\n 9\n 10\n 11\n-12\n+12a\n+12b\n 13\n 14\n 15\n"
        );
        let near = old
            .replace("\n4\n", "\nfour\n")
            .replace("\n9\n", "\nnine\n");
        assert_eq!(
            unified("f", &old, &near),
            "--- f\n+++ f\n\
             @@ -1,12 +1,12 @@\n 1\n 2\n 3\n-4\n+four\n 5\n 6\n 7\n 8\n-9\n+nine\n 10\n 11\n 12\n"
        );
    }

    #[test]
    fn insertions_deletions_and_a_missing_final_newline() {
        assert_eq!(
            unified("f", "", "a\n"),
            "--- f\n+++ f\n@@ -0,0 +1,1 @@\n+a\n"
        );
        assert_eq!(
            unified("f", "a\nb\n", "a\n"),
            "--- f\n+++ f\n@@ -1,2 +1,1 @@\n a\n-b\n"
        );
        assert_eq!(
            unified("f", "a\nb", "a\nc"),
            "--- f\n+++ f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
        );
    }
}
