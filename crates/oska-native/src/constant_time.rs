//! The constant-time guarantee of native executables (#225, ADR 0012):
//! no conditional branch, and no memory address, depends on a secret.
//!
//! The checker refuses a secret in a condition, an index, a division or a
//! shift amount (`E0353` to `E0356`). This is the backend's half: after
//! lowering and optimization, just before code is selected, [`verify`]
//! follows every secret through the code and fails the build with an
//! internal error (`E0390`) if one reaches
//!
//! - the condition of a branch ([`Inst::Branch`], [`Inst::BranchImm`],
//!   [`Inst::JumpIfZero`]), or checked arithmetic, which branches on
//!   overflow ([`Inst::Arith`], [`Inst::ArithImm`]);
//! - the address of a load or store, or of an indirect call.
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
//! `rt_mem_conceal` marks (`bytes_read`, `bytes_constant_time_eq`,
//! `bytes_select`, the constant-time text comparison). A `Secret<Bytes>`'s
//! object address and length are public; only its bytes are secret.
//!
//! Every value an instruction computes from a secret is secret. A call
//! with secret arguments is followed into the callee, with those
//! parameters secret (each callee once for each set of secret
//! parameters), and its result is secret when the callee returns a value
//! computed from one of them. A mark made inside the callee stays there:
//! `bytes_read` gives `Bytes.at` of public bytes a public byte, and a
//! secret's operation stores its result in a new `Secret`, whose read is
//! marked again. A value stored to memory and loaded back is not followed:
//! the reads that matter are the marked ones.
//!
//! Each function is followed as a whole, not path by path: a temporary
//! is secret if any instruction writes a secret to it.

use std::collections::{HashMap, HashSet};

use oska_syntax::Diagnostic;

use crate::ir::{FnId, Function, Inst, Temp};
use crate::{codes, NOWHERE};

/// Where a secret would decide control flow or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    /// The function it is in.
    pub function: String,
    /// What the secret would decide, as in "the condition of a branch".
    pub what: &'static str,
}

/// Fail with an internal error (`E0390`) if a secret value decides a
/// conditional branch or a memory address anywhere in `functions`.
pub fn verify(functions: &[Function]) -> Result<(), Diagnostic> {
    secrets(functions).map(|_| ()).map_err(|leak| {
        Diagnostic::new(
            codes::UNSUPPORTED,
            format!(
                "internal error: the native backend would make {} depend on a secret value, \
                 in `{}`; this is a bug in `oskac`, not in the program, so the build stops \
                 rather than write an executable that is not constant-time (`oskac run \
                 --interpret` runs the program)",
                leak.what, leak.function
            ),
            NOWHERE,
        )
    })
}

/// For each function of `functions`, which of its temporaries ever hold
/// a secret; or the first place a secret decides a branch or an address.
pub fn secrets(functions: &[Function]) -> Result<Vec<Vec<bool>>, Leak> {
    let mut analysis = Analysis {
        functions,
        summaries: HashMap::new(),
        running: HashSet::new(),
        secret: functions
            .iter()
            .map(|f| vec![false; f.temps as usize])
            .collect(),
    };
    for (id, function) in functions.iter().enumerate() {
        if function
            .insts
            .iter()
            .any(|inst| matches!(inst, Inst::Secret { .. }))
        {
            analysis.function(id, vec![false; function.params as usize])?;
        }
    }
    Ok(analysis.secret)
}

/// How secret a temporary is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Taint {
    /// Computed from a secret parameter: a result made from it is secret
    /// to the caller.
    param: bool,
    /// Computed from any secret, a parameter or a mark.
    any: bool,
}

impl Taint {
    fn join(self, other: Taint) -> Taint {
        Taint {
            param: self.param || other.param,
            any: self.any || other.any,
        }
    }
}

struct Analysis<'a> {
    functions: &'a [Function],
    /// Whether a function returns a value computed from its secret
    /// parameters, for each set of them followed so far.
    summaries: HashMap<(FnId, Vec<bool>), bool>,
    /// The functions being followed, for recursion.
    running: HashSet<(FnId, Vec<bool>)>,
    /// [`secrets`]'s answer, so far.
    secret: Vec<Vec<bool>>,
}

impl Analysis<'_> {
    /// Follow the secrets of function `id`, whose parameters `params`
    /// says are secret: whether it returns one computed from them.
    fn function(&mut self, id: FnId, params: Vec<bool>) -> Result<bool, Leak> {
        let key = (id, params);
        if let Some(&returns) = self.summaries.get(&key) {
            return Ok(returns);
        }
        // A recursive call: its result is taken as public this time
        // round; the outer call follows every instruction anyway.
        if !self.running.insert(key.clone()) {
            return Ok(false);
        }
        let functions = self.functions;
        let function = &functions[id];
        let mut taint = vec![Taint::default(); function.temps as usize];
        for (param, &secret) in key.1.iter().enumerate() {
            if let Some(t) = taint.get_mut(param) {
                *t = Taint {
                    param: secret,
                    any: secret,
                };
            }
        }
        let get = |taint: &[Taint], t: Temp| taint.get(t as usize).copied().unwrap_or_default();
        let leak = |what| Leak {
            function: function.name.clone(),
            what,
        };
        let mut returns = false;
        let mut last_call = Taint::default();
        loop {
            let mut changed = false;
            for inst in &function.insts {
                let uses = inst
                    .uses()
                    .into_iter()
                    .fold(Taint::default(), |acc, t| acc.join(get(&taint, t)));
                let result = match inst {
                    Inst::Secret { src, .. } => Taint {
                        param: get(&taint, *src).param,
                        any: true,
                    },
                    Inst::Branch { .. } | Inst::BranchImm { .. } | Inst::JumpIfZero { .. }
                        if uses.any =>
                    {
                        return Err(leak("the condition of a branch"));
                    }
                    Inst::Arith { .. } | Inst::ArithImm { .. } if uses.any => {
                        return Err(leak("checked arithmetic, which branches on overflow,"));
                    }
                    Inst::Load { base, .. } | Inst::Store { base, .. }
                        if get(&taint, *base).any =>
                    {
                        return Err(leak("the address of a load or store"));
                    }
                    Inst::CallIndirect { target, .. } if get(&taint, *target).any => {
                        return Err(leak("the address of a call"));
                    }
                    Inst::CallOnStack { stack, .. } if get(&taint, *stack).any => {
                        return Err(leak("the address of the stack"));
                    }
                    Inst::Call { callee, args, .. } => {
                        let params: Vec<bool> = args.iter().map(|&a| get(&taint, a).any).collect();
                        let result = if params.contains(&true) {
                            let returned = self.function(*callee, params)?;
                            Taint {
                                param: returned && uses.param,
                                any: returned,
                            }
                        } else {
                            Taint::default()
                        };
                        last_call = result;
                        result
                    }
                    // The second result of the call just before.
                    Inst::Second { .. } => last_call,
                    // A load's value is not followed through memory.
                    Inst::Load { .. } => Taint::default(),
                    Inst::Return(_) | Inst::ReturnPair { .. } => {
                        returns |= uses.param;
                        Taint::default()
                    }
                    _ => uses,
                };
                if let Some(dst) = inst.def() {
                    if let Some(t) = taint.get_mut(dst as usize) {
                        let joined = t.join(result);
                        if joined != *t {
                            *t = joined;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (t, taint) in taint.iter().enumerate() {
            if taint.any {
                if let Some(s) = self.secret[id].get_mut(t) {
                    *s = true;
                }
            }
        }
        self.running.remove(&key);
        self.summaries.insert(key, returns);
        Ok(returns)
    }
}

#[cfg(test)]
mod tests;
