//! `report`: the Markdown page of results, rendered from the committed
//! batches. The site publishes it as its Evaluation page
//! (`docs/evaluation.md`), and a test holds that page to what this renders
//! from `eval/results/`.
//!
//! The results directory holds one directory per batch, named by its id,
//! each with `runs/` (its run records) and `summary.json` (what
//! `summarise` makes of them). The page is rendered from the summaries, and
//! a summary that its runs no longer give is refused as stale, so the page
//! always agrees with the data. The latest batch (by its first run) is
//! shown in full, the earlier ones in a table, and the page always states
//! its limitations.
//!
//! Every text a record holds came from an agent, its CLI or the
//! maintainer's command line, so it reaches the page only through
//! [`plain`], as text: never as a link, an image, a heading, a code span or
//! HTML.

use crate::json::{self, array_at, field, num, number_at, pretty, str_at, strs_at, text};
use crate::summarise::summarise;
use crate::task::class_name;
use oska_syntax::JsonValue;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

/// `oska-eval report <results-dir> [--out <file>]`.
pub fn run(results: &str, out: Option<&str>) -> ExitCode {
    let (page, count) = match batches(Path::new(results)) {
        Ok(batches) => (render(&batches), batches.len()),
        Err(message) => return json::bad_input(&[message]),
    };
    let Some(out) = out else {
        print!("{page}");
        return ExitCode::SUCCESS;
    };
    if let Err(e) = std::fs::write(out, &page) {
        return json::bad_input(&[format!("cannot write {out}: {e}")]);
    }
    print!(
        "{}",
        pretty(&JsonValue::Object(vec![
            field("schema", num(1)),
            field("report", text(out)),
            field("batches", num(count as u64)),
        ]))
    );
    ExitCode::SUCCESS
}

/// Every batch's summary, the latest first. Each must be the summary its
/// runs give. An entry that cannot be read is an error, never skipped.
fn batches(results: &Path) -> Result<Vec<JsonValue>, String> {
    let unreadable = |e: std::io::Error| format!("cannot read {}: {e}", results.display());
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(results).map_err(unreadable)? {
        let path = entry.map_err(unreadable)?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if path.is_dir() && !hidden {
            dirs.push(path);
        }
    }
    dirs.sort();
    let mut summaries = Vec::new();
    for dir in dirs {
        let path = dir.join("summary.json");
        let committed = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "cannot read {}: {e}; run `oska-eval summarise` on the batch",
                path.display()
            )
        })?;
        let summary = summarise(&dir)?;
        if pretty(&summary) != committed {
            return Err(format!(
                "{} is stale: it is not what its runs give; run `oska-eval summarise` on the batch again",
                path.display()
            ));
        }
        summaries.push(summary);
    }
    summaries.sort_by(|a, b| {
        let key = |s: &JsonValue| (str_at(s, "started"), str_at(s, "batch"));
        key(b).cmp(&key(a))
    });
    Ok(summaries)
}

const INTRO: &str = "\
How often AI agents write Oska code that works, and how often that code is \
also secure, measured by Oska's evaluation harness. Each run gives one agent \
one task from the task set in the repository's `eval/` directory, with a \
prompt that describes the function only and never mentions security. The \
maintainer runs each batch locally and commits its results; this page is \
generated from them by `oska-eval report`, and a test holds it to them.

A run passes **func** when its code passes every hidden functional check. It \
passes **func-sec** when it also passes every hidden security check (the \
attack tests, the output scan, no audit site the task does not allow, and no \
authority beyond the task's expected authority), the release check, and the \
integrity check (no read-only file changed, no file in place of the hidden \
checks, and no link). func@k is the chance that at least one of k runs of a \
task passes, by the unbiased estimator, averaged over each task and variant; \
k is 1 and the attempts per task. A run that stopped at a limit counts as \
failed, never as dropped.
";

const LIMITATIONS: &str = "\
## Limitations

- The task set is small, a few tasks per class, so one run moves a rate a lot.
- Only the classes that run today are measured: path traversal, cross-site \
scripting, secret logging and least authority. The SQL injection, command \
injection and SSRF tasks wait until their sinks run.
- The tasks and their reference solutions are public, so a model may have \
seen them in training.
- Each batch uses one agent CLI and the models the maintainer chose. The \
results say nothing about other agents or models.
- The hidden checks are tests: code that passes them may still have a \
weakness that no test probes.
- Whether a machine fix was applied, and whether its diagnostic was gone at \
the next check, are read from the transcript and are approximate.
- Usage and cost are as the agent CLI reported them.
";

fn render(batches: &[JsonValue]) -> String {
    let mut page = format!("# Evaluation\n\n{INTRO}\n");
    match batches.split_first() {
        None => page.push_str("No batch has been run yet.\n\n"),
        Some((latest, earlier)) => {
            latest_batch(&mut page, latest);
            if !earlier.is_empty() {
                earlier_batches(&mut page, earlier);
            }
        }
    }
    page.push_str(LIMITATIONS);
    page
}

fn latest_batch(page: &mut String, s: &JsonValue) {
    let ks = numbers(s, "k");
    let attempts = number(s, "attempts");
    let runs = s.get("runs").unwrap_or(&JsonValue::Null);
    let mut stops = Vec::new();
    for (key, label) in [
        ("finished", "finished"),
        ("turn_limit", "at the turn limit"),
        ("timeout", "at the timeout"),
        ("budget", "at the batch budget"),
        ("error", "with an error"),
    ] {
        let n = number(runs, key);
        if n != "0" {
            stops.push(format!("{n} {label}"));
        }
    }
    let usage = s.get("usage").unwrap_or(&JsonValue::Null);
    let cost = number(usage, "cost_usd");
    let cost = cost.parse::<f64>().map_or(cost, |c| format!("{c:.2}"));
    let config = s.get("config").unwrap_or(&JsonValue::Null);
    let dollars = |key: &str| {
        let n = number(config, key);
        n.parse::<f64>().map_or(n, |c| format!("{c:.2}"))
    };
    let (budget, estimate) = (dollars("budget_usd"), dollars("estimate_usd_per_run"));
    let cap = dollars("cap_usd_per_run");
    let variants: Vec<String> = array_at(config, "variants")
        .iter()
        .map(|v| {
            let on = |key: &str| match v.get(key).and_then(JsonValue::as_bool) {
                Some(true) => "yes",
                _ => "no",
            };
            format!(
                "{} (site: {}, hook: {}, fix: {})",
                plain(&str_at(v, "name")),
                on("web"),
                on("hook"),
                on("fix")
            )
        })
        .collect();

    let _ = write!(
        page,
        "## Latest batch\n\n\
         - **Batch**: {}\n\
         - **Dates**: {}\n\
         - **Model**: {}\n\
         - **Agent CLI**: {}\n\
         - **`oskac`**: {}\n\
         - **Task set digest**: {}\n\
         - **Tasks**: {} ({})\n\
         - **Variants**: {}\n\
         - **Attempts per task**: {attempts}\n\
         - **Runs**: {}: {}\n\
         - **Cost**: {cost} US dollars, as the agent CLI reported it\n\
         - **Budget**: {budget} US dollars, at an estimated {estimate} and at most {cap} US dollars a run\n",
        plain(&str_at(s, "batch")),
        plain(&dates(s)),
        plains(&strs_at(s, "models")),
        plains(&strs_at(s, "agents")),
        plains(&strs_at(s, "oskac")),
        plain(&str_at(s, "task_set_digest")),
        strs_at(s, "tasks").len(),
        strs_at(s, "classes")
            .iter()
            .map(|c| class_name(c).map_or_else(|| plain(c), str::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        variants.join(", "),
        number(runs, "total"),
        stops.join(", "),
    );
    let incomplete: Vec<String> = array_at(s, "incomplete")
        .iter()
        .map(|p| {
            format!(
                "{} in {} ({} of {} runs)",
                plain(&str_at(p, "task")),
                plain(&str_at(p, "variant")),
                number(p, "runs"),
                number(p, "attempts")
            )
        })
        .collect();
    if !incomplete.is_empty() {
        let _ = writeln!(
            page,
            "- **Incomplete**: {}. Their rates at k = {attempts} are not given, as the runs cannot say.",
            incomplete.join(", ")
        );
    }
    page.push('\n');

    page.push_str("### Pass rates\n\n| Group | Tasks and variants | Runs |");
    for k in &ks {
        let _ = write!(page, " func@{k} | func-sec@{k} |");
    }
    page.push_str("\n| --- | --- | --- |");
    for _ in &ks {
        page.push_str(" --- | --- |");
    }
    page.push('\n');
    for row in array_at(s, "pass_rates") {
        let name = str_at(row, "name");
        let group = match str_at(row, "by").as_str() {
            "all" => "All".to_string(),
            "class" => format!(
                "Class: {}",
                class_name(&name).map_or_else(|| plain(&name), str::to_string)
            ),
            _ => format!("Variant: {}", plain(&name)),
        };
        let _ = write!(
            page,
            "| {group} | {} | {} |",
            number(row, "pairs"),
            number(row, "runs")
        );
        for k in &ks {
            let _ = write!(
                page,
                " {} | {} |",
                percent(row, &format!("func@{k}")),
                percent(row, &format!("func_sec@{k}"))
            );
        }
        page.push('\n');
    }
    page.push('\n');

    let security = s.get("security").unwrap_or(&JsonValue::Null);
    let flags = s.get("flags").unwrap_or(&JsonValue::Null);
    let _ = write!(
        page,
        "### Security outcomes\n\n\
         Each line counts runs.\n\n\
         - An attack test broke the code: {}\n\
         - Added an audit site the task does not allow: {}\n\
         - Widened authority beyond the task's expected authority: {}\n\
         - Printed output the task forbids: {}\n\
         - Failed the release check: {}\n\
         - Changed a read-only file: {}\n\
         - Made a tool call outside the workspace: {}\n\
         - Finished while failing the functional checks: {}\n\
         - Not graded, because the run stopped first: {}\n\n",
        number(security, "attack_succeeded"),
        number(security, "audit_site"),
        number(security, "widened"),
        number(security, "forbidden_output"),
        number(security, "release_failed"),
        number(flags, "read_only_changed"),
        number(flags, "escape"),
        number(flags, "false_claim"),
        number(security, "ungraded"),
    );

    let iterations = s.get("iterations").unwrap_or(&JsonValue::Null);
    let after = number_at(iterations, "mean_to_clean").map_or_else(String::new, |mean| {
        format!(", after {} checks on average", trim_decimal(&mean))
    });
    let _ = write!(
        page,
        "### How the agents worked\n\n\
         An iteration is one `oskac check` result the agent received, from a \
         command it ran or from the check-on-edit hook. {} of {} analysed runs \
         reached a check with no diagnostics{after}.\n\n",
        number(iterations, "clean"),
        number(iterations, "analysed"),
    );

    let codes = array_at(s, "codes");
    if codes.is_empty() {
        page.push_str("No run saw a diagnostic.\n\n");
    } else {
        page.push_str(
            "Diagnostic codes, those present in the most iterations first, so the \
             codes that held agents up longest lead. A fix is offered with a share \
             of a code's occurrences, applied (of those offered) when a later edit \
             makes its change or the agent ran `oskac fix`, and helped (of those \
             applied) when its diagnostic was gone at the next check; these are \
             read from the transcript.\n\n\
             | Code | Seen | Runs | Iterations present | Resolved | Mean iterations to resolve | Fix offered | Fix applied | Fix helped |\n\
             | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
        );
        for c in codes {
            let _ = writeln!(
                page,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                code(&str_at(c, "code")),
                number(c, "occurrences"),
                number(c, "runs"),
                number(c, "iterations_present"),
                number(c, "resolved"),
                mean(c, "mean_iterations_to_resolve"),
                share(c, "fix_offered_rate", "fix_offered"),
                share(c, "fix_applied_rate", "fix_applied"),
                share(c, "fix_helped_rate", "fix_helped"),
            );
        }
        page.push('\n');
        page.push_str(
            "By variant:\n\n\
             | Variant | Code | Seen | Iterations present | Mean iterations to resolve | Fix applied | Fix helped |\n\
             | --- | --- | --- | --- | --- | --- | --- |\n",
        );
        for v in array_at(s, "codes_by_variant") {
            let variant = plain(&str_at(v, "variant"));
            for c in array_at(v, "codes") {
                let _ = writeln!(
                    page,
                    "| {variant} | {} | {} | {} | {} | {} | {} |",
                    code(&str_at(c, "code")),
                    number(c, "occurrences"),
                    number(c, "iterations_present"),
                    mean(c, "mean_iterations_to_resolve"),
                    share(c, "fix_applied_rate", "fix_applied"),
                    share(c, "fix_helped_rate", "fix_helped"),
                );
            }
        }
        page.push('\n');
    }

    let lookups = s.get("lookups").unwrap_or(&JsonValue::Null);
    let list = |key: &str, item: &str, as_code: bool| -> String {
        let items: Vec<String> = array_at(lookups, key)
            .iter()
            .map(|x| {
                let name = str_at(x, item);
                let name = if as_code { code(&name) } else { plain(&name) };
                let runs = number(x, "runs");
                let unit = if runs == "1" { "run" } else { "runs" };
                format!("{name} ({runs} {unit})")
            })
            .collect();
        if items.is_empty() {
            "none".to_string()
        } else {
            items.join(", ")
        }
    };
    let _ = write!(
        page,
        "### Lookups\n\n\
         - `oskac explain`: {}\n\
         - Site pages: {}\n\n",
        list("explain", "code", true),
        list("site", "url", false),
    );
}

fn earlier_batches(page: &mut String, batches: &[JsonValue]) {
    page.push_str(
        "## Earlier batches\n\n\
         | Batch | Dates | Model | Variants | Runs | func@1 | func-sec@1 | k | func@k | func-sec@k |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for s in batches {
        let all = array_at(s, "pass_rates")
            .iter()
            .find(|r| str_at(r, "by") == "all")
            .cloned()
            .unwrap_or(JsonValue::Null);
        let k = number(s, "attempts");
        let total = number(s.get("runs").unwrap_or(&JsonValue::Null), "total");
        let _ = writeln!(
            page,
            "| {} | {} | {} | {} | {total} | {} | {} | {k} | {} | {} |",
            plain(&str_at(s, "batch")),
            plain(&dates(s)),
            plains(&strs_at(s, "models")),
            plains(&strs_at(s, "variants")),
            percent(&all, "func@1"),
            percent(&all, "func_sec@1"),
            percent(&all, &format!("func@{k}")),
            percent(&all, &format!("func_sec@{k}")),
        );
    }
    page.push('\n');
}

/// Untrusted text as Markdown that shows exactly that text and nothing
/// more: line breaks become spaces, backticks are dropped (a code span
/// would show its escapes), and every character that could start markup
/// is escaped with a backslash.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' | '\t' => out.push(' '),
            '`' => {}
            c if c.is_control() => {}
            '\\' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '!' | '|' | '<'
            | '>' | '~' | '&' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

fn plains(items: &[String]) -> String {
    items
        .iter()
        .map(|i| plain(i))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A diagnostic code: in a code span when it has a code's shape, such as
/// `E0300`, else as [`plain`] text.
fn code(code: &str) -> String {
    let mut chars = code.chars();
    let shaped = code.len() == 5
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_digit());
    if shaped {
        format!("`{code}`")
    } else {
        plain(code)
    }
}

/// The number at `key` as written, or "-".
fn number(value: &JsonValue, key: &str) -> String {
    number_at(value, key).unwrap_or_else(|| "-".to_string())
}

fn numbers(value: &JsonValue, key: &str) -> Vec<String> {
    array_at(value, key)
        .iter()
        .filter_map(|v| match v {
            JsonValue::Number(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// The batch's dates: one day, or the first and the last.
fn dates(s: &JsonValue) -> String {
    let day = |key: &str| str_at(s, key).chars().take(10).collect::<String>();
    let (first, last) = (day("started"), day("ended"));
    if first == last {
        first
    } else {
        format!("{first} to {last}")
    }
}

/// The rate at `key`, such as `0.8333`, as `83.3%`, or "-".
fn percent(value: &JsonValue, key: &str) -> String {
    number_at(value, key)
        .and_then(|rate| rate.parse::<f64>().ok())
        .map_or_else(|| "-".to_string(), |r| format!("{:.1}%", r * 100.0))
}

/// A rate with its count, such as `66.7% (2)`.
fn share(value: &JsonValue, rate: &str, count: &str) -> String {
    format!("{} ({})", percent(value, rate), number(value, count))
}

/// The mean at `key`, such as `1.5000` as `1.5`, or "-".
fn mean(value: &JsonValue, key: &str) -> String {
    number_at(value, key).map_or_else(|| "-".to_string(), |n| trim_decimal(&n))
}

/// A mean such as `1.5000` as `1.5`, and `2.0000` as `2`.
fn trim_decimal(n: &str) -> String {
    if !n.contains('.') {
        return n.to_string();
    }
    n.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn plain_text_escapes_markup_and_keeps_to_one_line() {
        assert_eq!(
            plain("[a](b)\n# c `d` <e>"),
            "\\[a\\]\\(b\\) \\# c d \\<e\\>"
        );
    }
}
