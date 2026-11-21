//! `Approval` and `Approver`: a person's yes to one consequential action
//! (#252).
//!
//! An approval is a struct with private fields, the summary and details
//! approved and the number of its record here, which says whether it was
//! used: every copy of it has the same number, so a second use of any copy
//! is refused. Only `Approver.confirm` makes one.
//!
//! In a program every approver is a terminal one: it writes its question
//! to stderr and reads the answer from stdin. In a test the only approver
//! is `t.approver` (a test has no `Stdin` to make another from), and it
//! answers from the test's script, recording what it was asked.
//!
//! `std.rt` does the same natively (its "Approvals" section), with the
//! same question and the same rules, so both backends ask and refuse
//! alike.

use std::collections::VecDeque;
use std::rc::Rc;

use oska_syntax::Span;

use crate::interp::{Interpreter, Stream};
use crate::intrinsics::Intrinsic::{HigherOrder, Plain};
use crate::intrinsics::{debug, Table};
use crate::memory;
use crate::panics::Panic;
use crate::value::Value;

/// Every approval intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("Approver.terminal", Plain(approver_terminal)),
    ("Approver.confirm", HigherOrder(approver_confirm)),
    ("Approval.summary", Plain(approval_summary)),
    ("Approval.details", Plain(approval_details)),
    ("Approval.matches", HigherOrder(approval_matches)),
    ("Approval.consume", HigherOrder(approval_consume)),
    ("ApprovalError.to_string", Plain(approval_error_to_string)),
    (
        "TestContext.answer_approval",
        HigherOrder(test_answer_approval),
    ),
    (
        "TestContext.approvals_asked",
        HigherOrder(test_approvals_asked),
    ),
];

/// The approvals of one run, and in a test `t.approver`'s script.
#[derive(Debug, Default)]
pub struct Approvals {
    /// Whether each approval made so far was used, by number.
    used: Vec<bool>,
    /// Whether the run is a test, where every approver is `t.approver`.
    test: bool,
    /// `t.approver`'s answers still to give, in order.
    answers: VecDeque<bool>,
    /// Every question `t.approver` was asked: its summary and details.
    asked: Vec<(Value, Value)>,
}

impl Approvals {
    /// Make every approver `t.approver`.
    pub fn for_test(&mut self) {
        self.test = true;
    }
}

/// The question an approver shows for `summary` and `details`: each line
/// with its line break. `std.rt`'s `approval_prompt` writes the same.
fn prompt(summary: &str, details: &[Value]) -> String {
    let mut text = "approval needed: ".to_string();
    debug(&Value::str(summary), &mut text);
    text.push('\n');
    for detail in details {
        text.push_str("  ");
        debug(detail, &mut text);
        text.push('\n');
    }
    text.push_str("approve? [y/N]\n");
    text
}

/// Whether `answer` is a yes: `y` or `yes`, in any case, trimmed.
fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

/// `Approver.terminal(stdin, stderr)`.
fn approver_terminal(
    _: &mut dyn crate::host::Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    Ok(Value::Struct {
        ty: "Approver".into(),
        fields: Rc::new(args),
    })
}

/// `Approver.confirm(self, summary, details)`.
fn approver_confirm(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let summary = args[1].as_str().to_string();
    let details = args[2].as_list().clone();
    let question = prompt(&summary, &details);
    let yes = if interp.approvals.test {
        let approvals = &mut interp.approvals;
        approvals
            .asked
            .push((args[1].clone(), Value::List(details.clone())));
        approvals.answers.pop_front().unwrap_or(false)
    } else {
        match interp.write_output(Stream::Stderr, &question, span)? {
            Err(_) => false,
            Ok(()) => match interp.read_line(span)? {
                Ok(Some(answer)) => is_yes(&answer),
                Ok(None) | Err(_) => false,
            },
        }
    };
    if !yes {
        return Ok(Value::none());
    }
    let number = interp.approvals.used.len();
    interp.approvals.used.push(false);
    Ok(Value::some(Value::Struct {
        ty: "Approval".into(),
        fields: Rc::new(vec![
            memory::text(&summary, span)?,
            Value::List(details),
            Value::int(number as i128),
        ]),
    }))
}

/// `Approval.summary(self)`.
fn approval_summary(
    _: &mut dyn crate::host::Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    Ok(args[0].field(0))
}

/// `Approval.details(self)`.
fn approval_details(
    _: &mut dyn crate::host::Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    Ok(args[0].field(1))
}

/// Whether the approval `approval` is for `summary` and `details`.
fn matches(approval: &Value, summary: &Value, details: &Value) -> bool {
    let texts = |list: &Value| -> Vec<String> {
        list.as_list()
            .iter()
            .map(|item| item.as_str().to_string())
            .collect()
    };
    approval.field(0).as_str() == summary.as_str() && texts(&approval.field(1)) == texts(details)
}

/// `Approval.matches(self, summary, details)`: false once it is used.
fn approval_matches(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let unused = !interp.approvals.used[args[0].field(2).as_int() as usize];
    Ok(Value::Bool(unused && matches(&args[0], &args[1], &args[2])))
}

/// `Approval.consume(self, summary, details)`.
fn approval_consume(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let number = args[0].field(2).as_int() as usize;
    let refused = |variant| Value::err(Value::variant("ApprovalError", variant, Vec::new()));
    if interp.approvals.used[number] {
        return Ok(refused("Used"));
    }
    if !matches(&args[0], &args[1], &args[2]) {
        return Ok(refused("Mismatch"));
    }
    interp.approvals.used[number] = true;
    Ok(Value::ok(Value::Unit))
}

/// `ApprovalError.to_string(self)`.
fn approval_error_to_string(
    _: &mut dyn crate::host::Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    Ok(Value::str(if args[0].is_variant("Used") {
        "approval already used: one approval covers one action"
    } else {
        "approval does not match: it was given for a different action"
    }))
}

/// `t.answer_approval(yes)`: `t.approver`'s answer to its next question.
fn test_answer_approval(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    // A program can build a `TestContext` by hand, but its approvers all
    // ask the terminal: nothing is scripted there, as in `std.rt`.
    if interp.approvals.test {
        interp.approvals.answers.push_back(args[1].as_bool());
    }
    Ok(Value::Unit)
}

/// `t.approvals_asked()`: every question `t.approver` was asked.
fn test_approvals_asked(
    interp: &mut Interpreter<'_, '_>,
    _: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let asked = interp
        .approvals
        .asked
        .iter()
        .map(|(summary, details)| Value::Struct {
            ty: "ApprovalRequest".into(),
            fields: Rc::new(vec![summary.clone(), details.clone()]),
        })
        .collect();
    Ok(Value::List(Rc::new(asked)))
}
