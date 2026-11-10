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

/// The flag variables of one body, for one kind of flag.
#[derive(Debug, Default, Clone)]
pub(super) struct Flags {
    vars: Vec<FlagVar>,
}

impl Flags {
    /// A new variable, with nothing flowing into or out of it.
    pub(super) fn fresh(&mut self) -> u32 {
        self.vars.push(FlagVar::default());
        self.vars.len() as u32 - 1
    }

    /// Whether a high flag has reached `v`, which is final.
    pub(super) fn is_high(&self, v: u32) -> bool {
        self.vars[v as usize].high
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
        let into = var.into.clone();
        into.into_iter().all(|w| self.raise(w))
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
        let from = var.from.clone();
        from.into_iter().all(|w| self.pin(w))
    }
}
