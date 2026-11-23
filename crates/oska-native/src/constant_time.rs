// SPDX-License-Identifier: Apache-2.0
//! The constant-time guarantee of native executables (#225, ADR 0012):
//! no conditional branch, and no memory address, depends on a secret.
//!
//! The checker refuses a secret in a condition, an index, a division or a
//! shift amount (`E0353` to `E0356`). This is the backend's half: after
//! lowering and optimization, just before code is selected, [`verify`]
//! follows every secret through the code and fails the build with an
//! internal error (`E0398`) if one reaches
//!
//! - the condition of a branch ([`Inst::Branch`], [`Inst::BranchImm`],
//!   [`Inst::JumpIfZero`]), or checked arithmetic, which branches on
//!   overflow ([`Inst::Arith`], [`Inst::ArithImm`]);
//! - the address of a load or store, or of a call;
//! - an argument of an indirect call, whose callee is not known.
//!
//! That cannot happen for a program that checked: it would be a bug in
//! the lowering or the optimizer, and the build stops rather than write
//! an executable that leaks.
//!
//! # Marks
//!
//! The lowering marks a secret with an [`Inst::Secret`]: the value read
//! out of a `Secret<U8>`, `Secret<U32>`, `Secret<U64>` or `Secret<Bool>`
//! (`data.rs`'s `field`), and in `std.rt` a byte of secret bytes, which
//! `rt_mem_conceal` marks (`bytes_read_secret`, which `Secret<Bytes>`'s
//! reads call, `bytes_constant_time_eq`, `bytes_select`, and the
//! constant-time text comparison). A `Secret<Bytes>`'s object address and
//! length are public; only its bytes are secret. An [`Inst::Expose`]
//! (`std.rt`'s `rt_mem_expose`) makes a value public on purpose: the
//! result of a constant-time comparison, a public `Bool`.
//!
//! Every value an instruction computes from a secret is secret. A call is
//! followed into its callee, with the parameters its secret arguments
//! pass to secret, and its result is secret when the callee can return a
//! secret, from those parameters or from a mark of its own. Recursion is
//! followed to a fixed point. An indirect call may reach any function
//! whose address the code takes: it may not pass a secret, and its result
//! is secret when one of theirs can be.
//!
//! # Limits
//!
//! - A value stored to memory and loaded back is not followed: a load
//!   gives a public value. Secrets are marked again where they are read:
//!   every read of a secret word out of its `Secret`, and every byte
//!   `std.rt` reads from secret bytes through `rt_mem_conceal`. A byte
//!   `std.rt` read from secret bytes without it would not be followed.
//! - Each function is followed as a whole, not path by path: a temporary
//!   is secret if any instruction writes a secret to it.
//! - A function that marks a secret is never inlined (`optimize.rs`), so
//!   the mark stays where the lowering made it.

use std::collections::{HashMap, HashSet};

use oska_syntax::Diagnostic;

use crate::ir::{FnId, Function, Inst, Symbol};
use crate::{codes, NOWHERE};

/// Where a secret would decide control flow or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    /// The function it is in.
    pub function: String,
    /// What the secret would decide, as in "the condition of a branch".
    pub what: &'static str,
}

/// Fail with an internal error (`E0398`) if a secret value decides a
/// conditional branch or a memory address anywhere in `functions`.
pub fn verify(functions: &[Function]) -> Result<(), Diagnostic> {
    secrets(functions).map(|_| ()).map_err(|leak| {
        Diagnostic::new(
            codes::NOT_CONSTANT_TIME,
            format!(
                "internal error: the native backend would make {} depend on a secret value, \
                 in `{}`; this is a bug in `oskac`, not in the program, so the build stops \
                 rather than write an executable that is not constant-time; please report it",
                leak.what, leak.function
            ),
            NOWHERE,
        )
    })
}

/// For each function of `functions`, which of its temporaries ever hold
/// a secret; or the first place a secret decides a branch or an address.
pub fn secrets(functions: &[Function]) -> Result<Vec<Vec<bool>>, Leak> {
    let indirect: Vec<FnId> = functions
        .iter()
        .flat_map(|f| &f.insts)
        .filter_map(|inst| match inst {
            Inst::Addr {
                symbol: Symbol::Function(id),
                ..
            } => Some(*id),
            _ => None,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut analysis = Analysis {
        functions,
        indirect,
        returns: HashMap::new(),
        queue: Vec::new(),
        queued: HashSet::new(),
        secret: functions
            .iter()
            .map(|f| vec![false; f.temps as usize])
            .collect(),
    };
    for (id, function) in functions.iter().enumerate() {
        analysis.request((id, vec![false; function.params as usize]));
    }
    // To a fixed point: what a function may return only grows, from
    // public, as the functions it calls are found to return secrets.
    loop {
        let mut grew = false;
        let mut at = 0;
        while at < analysis.queue.len() {
            let key = analysis.queue[at].clone();
            let returns = analysis.function(&key)?;
            if returns && analysis.returns.insert(key, true) != Some(true) {
                grew = true;
            }
            at += 1;
        }
        if !grew {
            return Ok(analysis.secret);
        }
    }
}

/// A function and which of its parameters are secret.
type Key = (FnId, Vec<bool>);

struct Analysis<'a> {
    functions: &'a [Function],
    /// The functions an indirect call may reach: those whose address the
    /// code takes.
    indirect: Vec<FnId>,
    /// The functions, with their secret parameters, known so far to
    /// return a secret.
    returns: HashMap<Key, bool>,
    /// Every function with its secret parameters to follow, in the order
    /// first asked for.
    queue: Vec<Key>,
    queued: HashSet<Key>,
    /// [`secrets`]'s answer, so far.
    secret: Vec<Vec<bool>>,
}

impl Analysis<'_> {
    /// Follow `key` too, if it is new.
    fn request(&mut self, key: Key) {
        if self.queued.insert(key.clone()) {
            self.queue.push(key);
        }
    }

    /// Whether `key` returns a secret, as far as is known yet; it is
    /// followed in turn.
    fn returns(&mut self, key: Key) -> bool {
        let known = self.returns.get(&key).copied().unwrap_or(false);
        self.request(key);
        known
    }

    /// Follow the secrets of `key`'s function, with its secret
    /// parameters, given what is known of the functions it calls: whether
    /// it returns one.
    fn function(&mut self, key: &Key) -> Result<bool, Leak> {
        let functions = self.functions;
        let (id, params) = key;
        let function = &functions[*id];
        let mut secret = vec![false; function.temps as usize];
        for (param, &is) in params.iter().enumerate() {
            if let Some(s) = secret.get_mut(param) {
                *s = is;
            }
        }
        let is = |secret: &[bool], t: u32| secret.get(t as usize).copied().unwrap_or(false);
        let leak = |what| Leak {
            function: function.name.clone(),
            what,
        };
        let mut returns = false;
        loop {
            let mut changed = false;
            // What the last call returned, for an `Inst::Second` after it.
            let mut last_call = false;
            for inst in &function.insts {
                let reads = inst.uses().into_iter().any(|t| is(&secret, t));
                let result = match inst {
                    Inst::Secret { .. } => true,
                    Inst::Expose { .. } => false,
                    Inst::Branch { .. } | Inst::BranchImm { .. } | Inst::JumpIfZero { .. }
                        if reads =>
                    {
                        return Err(leak("the condition of a branch"));
                    }
                    Inst::Arith { .. } | Inst::ArithImm { .. } if reads => {
                        return Err(leak("checked arithmetic, which branches on overflow,"));
                    }
                    Inst::Load { base, .. } | Inst::Store { base, .. } if is(&secret, *base) => {
                        return Err(leak("the address of a load or store"));
                    }
                    Inst::CallOnStack { stack, .. } if is(&secret, *stack) => {
                        return Err(leak("the address of the stack"));
                    }
                    Inst::CallIndirect { target, args, .. } => {
                        if is(&secret, *target) {
                            return Err(leak("the address of a call"));
                        }
                        if args.iter().any(|&a| is(&secret, a)) {
                            return Err(leak(
                                "an argument of an indirect call, whose callee is not known,",
                            ));
                        }
                        let mut any = false;
                        for callee in self.indirect.clone() {
                            let params = vec![false; functions[callee].params as usize];
                            any |= self.returns((callee, params));
                        }
                        last_call = any;
                        any
                    }
                    Inst::Call { callee, args, .. } => {
                        let params = args.iter().map(|&a| is(&secret, a)).collect();
                        last_call = self.returns((*callee, params));
                        last_call
                    }
                    // The second result of the call just before.
                    Inst::Second { .. } => last_call,
                    // A load's value is not followed through memory.
                    Inst::Load { .. } => false,
                    Inst::Return(_) | Inst::ReturnPair { .. } => {
                        returns |= reads;
                        false
                    }
                    _ => reads,
                };
                if let Some(dst) = inst.def() {
                    if let Some(s) = secret.get_mut(dst as usize) {
                        if result && !*s {
                            *s = true;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (t, &is) in secret.iter().enumerate() {
            if is {
                if let Some(s) = self.secret[*id].get_mut(t) {
                    *s = true;
                }
            }
        }
        Ok(returns)
    }
}

#[cfg(test)]
mod tests;
