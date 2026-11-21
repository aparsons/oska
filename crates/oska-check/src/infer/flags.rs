//! One-bit subtyping: the solver behind both one-way flags that join, the
//! trust flag on text (`trusted ≤ untrusted`) and the capability flag on
//! function types (`fn ≤ cap fn`).
//!
//! A flag is `Low`, `High`, or a **flag variable**. Each flow between
//! flags is an edge `a ≤ b`. A variable is high once a high flag reaches
//! it through the edges, and pinned low once it reaches a low slot; one
//! that would be both is a failed flow. Edges are kept, so a value that
//! turns out high later in the body still reaches every slot it flowed
//! into.
//!
//! Every change to a store is monotone: a variable is added, an edge is
//! added, or a variable becomes high or pinned. So a trial can be undone
//! by logging those changes while it runs and reverting them in reverse
//! ([`Flags::mark`], [`Flags::undo_to`]), with no copy of the store.

/// A flag: low (trusted, plain `fn`), high (untrusted, `cap fn`), or a
/// variable of a [`Flags`] store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flag {
    Low,
    High,
    Var(u32),
}

/// What is known about one flag variable.
#[derive(Debug, Default, Clone)]
struct FlagVar {
    /// A high flag flows into it.
    high: bool,
    /// It flows into a low slot.
    pinned: bool,
    /// The variables it flows into.
    into: Vec<u32>,
    /// The variables that flow into it.
    from: Vec<u32>,
}

/// One change to a [`Flags`] store, logged so that it can be undone.
#[derive(Debug, Clone, Copy)]
enum Change {
    /// A variable was added at the end.
    Fresh,
    /// The edge `x ≤ y` was added at the end of `x`'s `into` and `y`'s
    /// `from`.
    Edge(u32, u32),
    /// The variable became high.
    Raised(u32),
    /// The variable became pinned.
    Pinned(u32),
}

/// A point in a [`Flags`] store's history to undo back to, from
/// [`Flags::mark`].
#[derive(Debug, Clone, Copy)]
pub(super) struct Mark(usize);

/// The flag variables of one body, for one kind of flag.
#[derive(Debug, Default, Clone)]
pub(super) struct Flags {
    vars: Vec<FlagVar>,
    /// The changes since the open [`Mark`], while one is open.
    log: Vec<Change>,
    /// Whether a [`Mark`] is open, so that changes are logged.
    logging: bool,
}

impl Flags {
    /// Start logging changes, and return the point to undo back to. Only
    /// one mark is open at a time; [`Flags::keep`] closes it.
    pub(super) fn mark(&mut self) -> Mark {
        debug_assert!(!self.logging, "a mark is already open");
        self.logging = true;
        Mark(self.log.len())
    }

    /// Undo every change made since `mark`.
    pub(super) fn undo_to(&mut self, mark: Mark) {
        while self.log.len() > mark.0 {
            match self.log.pop() {
                Some(Change::Fresh) => {
                    self.vars.pop();
                }
                Some(Change::Edge(x, y)) => {
                    self.vars[x as usize].into.pop();
                    self.vars[y as usize].from.pop();
                }
                Some(Change::Raised(v)) => self.vars[v as usize].high = false,
                Some(Change::Pinned(v)) => self.vars[v as usize].pinned = false,
                None => break,
            }
        }
    }

    /// Keep every change made since `mark`, and close it: changes are no
    /// longer logged.
    pub(super) fn keep(&mut self, mark: Mark) {
        self.log.truncate(mark.0);
        self.logging = false;
    }

    fn record(&mut self, change: Change) {
        if self.logging {
            self.log.push(change);
        }
    }

    /// A new variable, with nothing flowing into or out of it.
    pub(super) fn fresh(&mut self) -> u32 {
        self.vars.push(FlagVar::default());
        self.record(Change::Fresh);
        self.vars.len() as u32 - 1
    }

    /// Whether a high flag has reached `v`, which is final.
    pub(super) fn is_high(&self, v: u32) -> bool {
        self.vars[v as usize].high
    }

    /// Whether `v` flows into a low slot.
    pub(super) fn is_pinned(&self, v: u32) -> bool {
        self.vars[v as usize].pinned
    }

    /// Whether `from` flows into `to`, directly or through other
    /// variables.
    pub(super) fn reaches(&self, from: u32, to: u32) -> bool {
        let mut seen = vec![false; self.vars.len()];
        let mut stack = vec![from];
        while let Some(v) = stack.pop() {
            if v == to {
                return true;
            }
            if !std::mem::replace(&mut seen[v as usize], true) {
                stack.extend(&self.vars[v as usize].into);
            }
        }
        false
    }

    /// Add the edge `a ≤ b`: a value flagged `a` flows where `b` is
    /// required. Fails when a high flag would reach a low slot.
    pub(super) fn flow(&mut self, a: Flag, b: Flag) -> bool {
        match (a, b) {
            (Flag::Low, _) | (_, Flag::High) => true,
            (Flag::High, Flag::Low) => false,
            (Flag::High, Flag::Var(v)) => self.raise(v),
            (Flag::Var(v), Flag::Low) => self.pin(v),
            (Flag::Var(x), Flag::Var(y)) => {
                if x == y {
                    return true;
                }
                self.vars[x as usize].into.push(y);
                self.vars[y as usize].from.push(x);
                self.record(Change::Edge(x, y));
                (!self.vars[x as usize].high || self.raise(y))
                    && (!self.vars[y as usize].pinned || self.pin(x))
            }
        }
    }

    /// A high flag reaches `v`, and everything `v` flows into.
    fn raise(&mut self, v: u32) -> bool {
        let var = &mut self.vars[v as usize];
        if var.high {
            return true;
        }
        if var.pinned {
            return false;
        }
        var.high = true;
        self.record(Change::Raised(v));
        let mut i = 0;
        while let Some(&w) = self.vars[v as usize].into.get(i) {
            if !self.raise(w) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// `v`, and everything that flows into it, flows into a low slot.
    fn pin(&mut self, v: u32) -> bool {
        let var = &mut self.vars[v as usize];
        if var.pinned {
            return true;
        }
        if var.high {
            return false;
        }
        var.pinned = true;
        self.record(Change::Pinned(v));
        let mut i = 0;
        while let Some(&w) = self.vars[v as usize].from.get(i) {
            if !self.pin(w) {
                return false;
            }
            i += 1;
        }
        true
    }
}
