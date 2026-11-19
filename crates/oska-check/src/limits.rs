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
    /// `--max-output=<bytes>`: the bytes the program may write to
    /// standard output and standard error together.
    Output,
    /// `--max-memory=<bytes>`: the bytes the program's live values may
    /// take, net of frees, counted by each backend's own allocator.
    Memory,
}

impl Limit {
    /// Every limit, in the order the usage and the docs list them.
    pub const ALL: [Limit; 3] = [Limit::Steps, Limit::Output, Limit::Memory];

    /// The `oskac run` flag that sets the limit, without its `=` and
    /// value.
    pub fn flag(self) -> &'static str {
        match self {
            Limit::Steps => "--max-steps",
            Limit::Output => "--max-output",
            Limit::Memory => "--max-memory",
        }
    }

    /// The panic code a program stops with when it reaches the limit.
    pub fn code(self) -> &'static str {
        match self {
            Limit::Steps => codes::STEP_LIMIT,
            Limit::Output => codes::OUTPUT_LIMIT,
            Limit::Memory => codes::MEMORY_LIMIT,
        }
    }

    /// The message of the panic at the limit `value`, on every backend.
    pub fn message(self, value: u64) -> String {
        match self {
            Limit::Steps => format!(
                "step limit reached: the program would take more than {value} steps, its calls \
                 and loop back-edges (`--max-steps={value}`)"
            ),
            Limit::Output => format!(
                "output limit reached: this write would take standard output and standard \
                 error past {value} bytes (`--max-output={value}`)"
            ),
            Limit::Memory => format!(
                "memory limit reached: the program's values take more than {value} bytes \
                 (`--max-memory={value}`)"
            ),
        }
    }
}

/// The panic codes of the run limits, each defined here only.
pub mod codes {
    /// The program would have taken more than `--max-steps` steps.
    pub const STEP_LIMIT: &str = "P0038";
    /// The program would have written more than `--max-output` bytes to
    /// standard output and standard error together.
    pub const OUTPUT_LIMIT: &str = "P0041";
    /// The program's live values took more than `--max-memory` bytes.
    pub const MEMORY_LIMIT: &str = "P0040";
}

/// The limits one run is under: `None` for each limit not given, else a
/// value above 0 and at most `i64::MAX`. With none given
/// ([`Limits::is_empty`]) a run is exactly what it was before limits
/// existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Limits {
    /// [`Limit::Steps`].
    pub max_steps: Option<u64>,
    /// [`Limit::Output`].
    pub max_output: Option<u64>,
    /// [`Limit::Memory`].
    pub max_memory: Option<u64>,
}

impl Limits {
    /// No limit at all.
    pub const NONE: Limits = Limits {
        max_steps: None,
        max_output: None,
        max_memory: None,
    };

    /// Whether no limit is given.
    pub fn is_empty(&self) -> bool {
        Limit::ALL.iter().all(|&limit| self.get(limit).is_none())
    }

    /// The value of `limit`, when it is given.
    pub fn get(&self, limit: Limit) -> Option<u64> {
        match limit {
            Limit::Steps => self.max_steps,
            Limit::Output => self.max_output,
            Limit::Memory => self.max_memory,
        }
    }

    /// The value of `limit`, to set.
    pub fn get_mut(&mut self, limit: Limit) -> &mut Option<u64> {
        match limit {
            Limit::Steps => &mut self.max_steps,
            Limit::Output => &mut self.max_output,
            Limit::Memory => &mut self.max_memory,
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
