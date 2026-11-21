//! `Budget`: authority to spend model tokens and calls (#251).
//!
//! A budget's counters live in the interpreter, not in its value: a
//! [`Capability::Budget`] is only the number of its counters, so every
//! copy of a budget draws from the same ones. Each budget but the root
//! has a parent, the budget `limit` was called on. A budget's limit is
//! clamped to what its parent had left when it was made, and a
//! reservation must fit what it and every ancestor has left, so a
//! limited budget never spends more than any budget above it.
//!
//! The root, number [`ROOT`], is `main`'s and is unlimited; a test's is
//! `t.budget`, with [`TEST_TOKENS`] and [`TEST_CALLS`].
//!
//! `std.rt` keeps the same counters natively (its "Budgets" section), with
//! the same rules, so both backends refuse and report alike.

use std::rc::Rc;

use oska_check::builtins;
use oska_syntax::Span;

use crate::interp::Interpreter;
use crate::intrinsics::Intrinsic::{HigherOrder, Plain};
use crate::intrinsics::Table;
use crate::panics::Panic;
use crate::value::{Capability, Value};

/// The number of the root budget's counters.
pub const ROOT: usize = 0;

/// The tokens a test's `t.budget` may spend.
pub const TEST_TOKENS: i128 = 100_000;

/// The calls a test's `t.budget` may make.
pub const TEST_CALLS: i128 = 100;

/// The largest `Int`: counters saturate there rather than overflow.
const MAX: i128 = i64::MAX as i128;

/// Every budget intrinsic, by qualified name.
pub const INTRINSICS: Table = &[
    ("Budget.limit", HigherOrder(budget_limit)),
    ("Budget.remaining", HigherOrder(budget_remaining)),
    ("Budget.reserve", HigherOrder(budget_reserve)),
    ("Budget.spent", HigherOrder(budget_spent)),
    ("OverBudget.to_string", Plain(over_budget_to_string)),
    ("Reservation.debit", HigherOrder(reservation_debit)),
    ("Reservation.release", HigherOrder(reservation_release)),
];

/// One amount a budget counts, tokens or calls.
#[derive(Debug, Clone, Copy, Default)]
struct Count {
    /// The most it may spend, or `None` for no limit.
    limit: Option<i128>,
    spent: i128,
    /// Reserved and not yet settled.
    reserved: i128,
}

impl Count {
    /// What it has left to reserve, never below 0; `None` when unlimited.
    fn left(&self) -> Option<i128> {
        self.limit.map(|l| (l - self.spent - self.reserved).max(0))
    }
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<usize>,
    tokens: Count,
    calls: Count,
}

#[derive(Debug, Clone, Copy)]
struct Hold {
    budget: usize,
    tokens: i128,
    open: bool,
}

/// The budgets and reservations of one run.
#[derive(Debug, Default)]
pub struct Budgets {
    /// Every budget made so far; the root is made on first use.
    nodes: Vec<Node>,
    reservations: Vec<Hold>,
    /// Whether the root is a test's `t.budget`.
    test: bool,
}

impl Budgets {
    /// Make the root `t.budget`, with a test's fixed limits.
    pub fn for_test(&mut self) {
        self.test = true;
    }

    fn ensure_root(&mut self) {
        if self.nodes.is_empty() {
            let limit = |n| self.test.then_some(n);
            self.nodes.push(Node {
                parent: None,
                tokens: Count {
                    limit: limit(TEST_TOKENS),
                    ..Count::default()
                },
                calls: Count {
                    limit: limit(TEST_CALLS),
                    ..Count::default()
                },
            });
        }
    }

    /// `budget` and each budget above it, nearest first.
    fn chain(&mut self, budget: usize) -> Vec<usize> {
        self.ensure_root();
        let mut chain = vec![budget];
        while let Some(parent) = self.nodes[*chain.last().expect("not empty")].parent {
            chain.push(parent);
        }
        chain
    }

    /// What `budget` has left: the least of its chain's, tokens then calls.
    fn left(&mut self, budget: usize) -> (Option<i128>, Option<i128>) {
        let least = |a: Option<i128>, b: Option<i128>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.chain(budget)
            .into_iter()
            .fold((None, None), |(tokens, calls), n| {
                let node = &self.nodes[n];
                (
                    least(tokens, node.tokens.left()),
                    least(calls, node.calls.left()),
                )
            })
    }

    fn limit(&mut self, budget: usize, tokens: i128, calls: i128) -> usize {
        let (tokens_left, calls_left) = self.left(budget);
        let clamp = |asked: i128, left: Option<i128>| {
            let asked = asked.max(0);
            Some(left.map_or(asked, |left| asked.min(left)))
        };
        self.nodes.push(Node {
            parent: Some(budget),
            tokens: Count {
                limit: clamp(tokens, tokens_left),
                ..Count::default()
            },
            calls: Count {
                limit: clamp(calls, calls_left),
                ..Count::default()
            },
        });
        self.nodes.len() - 1
    }

    /// Reserve `tokens` and one call, or give what `budget` has left.
    fn reserve(
        &mut self,
        budget: usize,
        tokens: i128,
    ) -> Result<usize, (Option<i128>, Option<i128>)> {
        let tokens = tokens.max(0);
        let (tokens_left, calls_left) = self.left(budget);
        // A budget with no tokens left refuses even a reservation of none:
        // otherwise every call could overshoot it again.
        let short = |left: Option<i128>, asked: i128| left.is_some_and(|l| l < asked.max(1));
        if short(tokens_left, tokens) || short(calls_left, 1) {
            return Err((tokens_left, calls_left));
        }
        for n in self.chain(budget) {
            let node = &mut self.nodes[n];
            node.tokens.reserved = (node.tokens.reserved + tokens).min(MAX);
            node.calls.reserved = (node.calls.reserved + 1).min(MAX);
        }
        self.reservations.push(Hold {
            budget,
            tokens,
            open: true,
        });
        Ok(self.reservations.len() - 1)
    }

    /// Settle reservation `id`, spending the call and `used` tokens when
    /// `used` is given; nothing when it was settled already.
    fn settle(&mut self, id: usize, used: Option<i128>) {
        let hold = self.reservations[id];
        if !hold.open {
            return;
        }
        self.reservations[id].open = false;
        for n in self.chain(hold.budget) {
            let node = &mut self.nodes[n];
            // Never below 0: an unlimited budget's reservations stop at the
            // largest `Int`, so they may add up to less than is returned.
            node.tokens.reserved = (node.tokens.reserved - hold.tokens).max(0);
            node.calls.reserved = (node.calls.reserved - 1).max(0);
            if let Some(used) = used {
                node.tokens.spent = (node.tokens.spent + used.max(0)).min(MAX);
                node.calls.spent = (node.calls.spent + 1).min(MAX);
            }
        }
    }
}

/// The budget the checker guarantees `value` is.
fn budget(value: &Value) -> usize {
    match value {
        Value::Capability(Capability::Budget(n)) => *n,
        other => unreachable!("the checker guarantees a `Budget`, found {other:?}"),
    }
}

fn record(ty: &str, fields: Vec<Value>) -> Value {
    Value::Struct {
        ty: ty.into(),
        fields: Rc::new(fields),
    }
}

fn allowance((tokens, calls): (Option<i128>, Option<i128>)) -> Value {
    let amount = |n: Option<i128>| Value::option(n.map(Value::int));
    record("Allowance", vec![amount(tokens), amount(calls)])
}

/// `Budget.limit(self, tokens, calls)`.
fn budget_limit(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let child = interp
        .budgets
        .limit(budget(&args[0]), args[1].as_int(), args[2].as_int());
    Ok(Value::Capability(Capability::Budget(child)))
}

/// `Budget.remaining(self)`.
fn budget_remaining(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    Ok(allowance(interp.budgets.left(budget(&args[0]))))
}

/// `Budget.spent(self)`.
fn budget_spent(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let n = budget(&args[0]);
    interp.budgets.ensure_root();
    let node = &interp.budgets.nodes[n];
    Ok(record(
        "Spending",
        vec![Value::int(node.tokens.spent), Value::int(node.calls.spent)],
    ))
}

/// `Budget.reserve(self, tokens)`.
fn budget_reserve(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let asked = args[1].as_int();
    Ok(match interp.budgets.reserve(budget(&args[0]), asked) {
        // The layout native code has (`std.rt`'s `Hold`), but `handle` is
        // the number of its record here, which holds whether it is open:
        // the `open` field is never read.
        Ok(id) => Value::ok(record(
            builtins::RESERVATION,
            vec![
                Value::int(id as i128),
                Value::int(asked.max(0)),
                Value::Bool(true),
            ],
        )),
        Err(left) => Value::err(record(
            "OverBudget",
            vec![Value::int(asked.max(0)), allowance(left)],
        )),
    })
}

/// The reservation the checker guarantees `value` is.
fn reservation(value: &Value) -> usize {
    value.field(0).as_int() as usize
}

/// `Reservation.debit(self, input, output, cache_creation, cache_read)`:
/// every count, each at least 0, spent as tokens.
fn reservation_debit(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let used = args[1..].iter().map(|n| n.as_int().max(0)).sum::<i128>();
    interp
        .budgets
        .settle(reservation(&args[0]), Some(used.min(MAX)));
    Ok(Value::Unit)
}

/// `Reservation.release(self)`.
fn reservation_release(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    interp.budgets.settle(reservation(&args[0]), None);
    Ok(Value::Unit)
}

/// `OverBudget.to_string(self)`: `over budget: asked for 500 tokens and 1
/// call, with 100 tokens and 3 calls left`.
fn over_budget_to_string(
    _: &mut dyn crate::host::Host,
    args: Vec<Value>,
    _: Span,
) -> Result<Value, Panic> {
    let asked = args[0].field(0).as_int();
    let left = args[0].field(1);
    let amount = |value: Value| match value.fields().first() {
        Some(n) if value.is_variant("Some") => n.as_int().to_string(),
        _ => "unlimited".to_string(),
    };
    let tokens = amount(left.field(0));
    let calls = amount(left.field(1));
    Ok(Value::str(&format!(
        "over budget: asked for {asked} tokens and 1 call, with {tokens} tokens and {calls} calls left"
    )))
}
