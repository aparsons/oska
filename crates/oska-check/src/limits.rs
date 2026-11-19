//! **Run limits** (spec #446): the bounds an embedder puts on one `oskac
//! run`, which stop the program with a panic of its own and exit code
//! 101 when it reaches one.
//!
//! This module holds what both backends need: [`Limits`], the plain
//! value `oskac` hands to `oska-run`'s interpreter and `oska-native`'s
//! lowering, and the panic code and message of each [`Limit`]. The flags
//! that set them are `oskac`'s (`oskac/src/limits.rs`). To add a limit: a
//! variant of [`Limit`] with its code and message, a field of [`Limits`],
//! its flag in `oskac`, and its row in `docs/runtime.md`; then enforce it
//! in each backend.

/// One kind of run limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// `--max-steps=<n>`: the steps the program may take (ADR 0021): calls
    /// of functions, methods and closures written in Oska, and loop
    /// back-edges.
    Steps,
    /// `--timeout=<ms>`: the wall-clock milliseconds the program may run,
    /// waiting included.
    Time,
    /// `--max-output=<bytes>`: the bytes the program may write to
    /// standard output and standard error together.
    Output,
}

impl Limit {
    /// Every limit, in the order the usage and the docs list them.
    pub const ALL: [Limit; 3] = [Limit::Steps, Limit::Time, Limit::Output];

    /// The `oskac run` flag that sets the limit, without its `=` and
    /// value.
    pub fn flag(self) -> &'static str {
        match self {
            Limit::Steps => "--max-steps",
            Limit::Time => "--timeout",
            Limit::Output => "--max-output",
        }
    }

    /// The panic code a program stops with when it reaches the limit.
    pub fn code(self) -> &'static str {
        match self {
            Limit::Steps => codes::STEP_LIMIT,
            Limit::Time => codes::TIME_LIMIT,
            Limit::Output => codes::OUTPUT_LIMIT,
        }
    }

    /// The message of the panic at the limit `value`, on every backend.
    pub fn message(self, value: u64) -> String {
        match self {
            Limit::Steps => format!(
                "step limit reached: the program would take more than {value} steps, its calls \
                 and loop back-edges (`--max-steps={value}`)"
            ),
            Limit::Time => format!(
                "time limit reached: the program ran for {value} ms of wall-clock time, \
                 waiting included (`--timeout={value}`)"
            ),
            Limit::Output => format!(
                "output limit reached: this write would take standard output and standard \
                 error past {value} bytes (`--max-output={value}`)"
            ),
        }
    }
}

/// The panic codes of the run limits, each defined here only.
pub mod codes {
    /// The program would have taken more than `--max-steps` steps.
    pub const STEP_LIMIT: &str = "P0038";
    /// The program ran for longer than `--timeout` milliseconds.
    pub const TIME_LIMIT: &str = "P0039";
    /// The program would have written more than `--max-output` bytes to
    /// standard output and standard error together.
    pub const OUTPUT_LIMIT: &str = "P0041";
}

/// The limits one run is under: `None` for each limit not given, else a
/// value above 0 and at most `i64::MAX`. With none given
/// ([`Limits::is_empty`]) a run is exactly what it was before limits
/// existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Limits {
    /// [`Limit::Steps`].
    pub max_steps: Option<u64>,
    /// [`Limit::Time`], in milliseconds.
    pub timeout: Option<u64>,
    /// [`Limit::Output`].
    pub max_output: Option<u64>,
}

impl Limits {
    /// No limit at all.
    pub const NONE: Limits = Limits {
        max_steps: None,
        timeout: None,
        max_output: None,
    };

    /// Whether no limit is given.
    pub fn is_empty(&self) -> bool {
        Limit::ALL.iter().all(|&limit| self.get(limit).is_none())
    }

    /// The value of `limit`, when it is given.
    pub fn get(&self, limit: Limit) -> Option<u64> {
        match limit {
            Limit::Steps => self.max_steps,
            Limit::Time => self.timeout,
            Limit::Output => self.max_output,
        }
    }

    /// The value of `limit`, to set.
    pub fn get_mut(&mut self, limit: Limit) -> &mut Option<u64> {
        match limit {
            Limit::Steps => &mut self.max_steps,
            Limit::Time => &mut self.timeout,
            Limit::Output => &mut self.max_output,
        }
    }
}

/// `text`, program-made text that a panic report quotes, cut to what
/// `--max-output` has `left`: all of it when it fits, else as many of its
/// first bytes as fit, ending where a character ends, then `...`. Both
/// backends cut alike (`std.rt`'s `cut_quoted`).
pub fn cut_quoted(text: &str, left: u64) -> std::borrow::Cow<'_, str> {
    let left = usize::try_from(left).unwrap_or(usize::MAX);
    if text.len() <= left {
        return text.into();
    }
    let mut end = left;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end]).into()
}
