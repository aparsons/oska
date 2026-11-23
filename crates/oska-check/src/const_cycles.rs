// SPDX-License-Identifier: Apache-2.0
//! Constants that depend on themselves (#278). A constant lowers to a
//! function of no parameters that each use calls (#83), so a constant
//! whose initializer names itself, directly or through other constants,
//! would call itself until the stack overflowed. Reported at the
//! initializer, so no checked program is built for it.
//!
//! The edges come from the resolutions body checking recorded: a name
//! that resolved to a constant, inside a constant's initializer. A local
//! that shadows a constant resolved to the local, so it is no edge. A
//! package's modules are one joined module here, so a cycle across modules
//! (which needs an import cycle, `E0207`) is found too, and each constant
//! names the others as its module writes them.

use std::collections::{HashMap, VecDeque};

use oska_syntax::ast::{ItemKind, Module};
use oska_syntax::{Diagnostic, Span};

use crate::codes;
use crate::program::lower::{Resolution, Resolutions};

/// Report every constant of `module` that is on a cycle of constants,
/// naming the cycle from that constant.
pub fn module(module: &Module, resolutions: &Resolutions, diagnostics: &mut Vec<Diagnostic>) {
    let consts: Vec<_> = module
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Const(c) => Some(c),
            _ => None,
        })
        .collect();
    // Every use of a constant, by where it is, found once: each
    // constant's initializer then reads its own range of them.
    let mut named: Vec<(Span, &str)> = resolutions
        .iter()
        .filter_map(|(span, resolution)| match resolution {
            Resolution::Const(name) => Some((*span, name.as_str())),
            _ => None,
        })
        .collect();
    named.sort_unstable_by_key(|(span, _)| span.start);
    // Each constant's uses of constants, in source order, once each.
    let mut uses: HashMap<&str, Vec<&str>> = HashMap::new();
    for c in &consts {
        let inside = c.value.span;
        let first = named.partition_point(|(span, _)| span.start < inside.start);
        let mut edges: Vec<&str> = Vec::new();
        for &(span, name) in &named[first..] {
            if span.start >= inside.end {
                break;
            }
            if span.end <= inside.end && !edges.contains(&name) {
                edges.push(name);
            }
        }
        uses.insert(c.name.name.as_str(), edges);
    }
    for c in &consts {
        let name = c.name.name.as_str();
        if let Some(cycle) = shortest_cycle(name, &uses) {
            diagnostics.push(Diagnostic::new(
                codes::CONST_CYCLE,
                format!(
                    "constant `{name}` depends on itself: {}",
                    cycle.join(" -> ")
                ),
                c.value.span,
            ));
        }
    }
}

/// The shortest way from `start` back to itself through `uses`, as the
/// names along it, `start` first and last.
fn shortest_cycle<'a>(
    start: &'a str,
    uses: &HashMap<&'a str, Vec<&'a str>>,
) -> Option<Vec<&'a str>> {
    let mut came_from: HashMap<&str, &str> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        for &next in uses.get(at).map(Vec::as_slice).unwrap_or_default() {
            if next == start {
                let mut path = vec![start, at];
                let mut back = at;
                while back != start {
                    back = came_from[back];
                    path.push(back);
                }
                path.reverse();
                return Some(path);
            }
            if !came_from.contains_key(next) && uses.contains_key(next) {
                came_from.insert(next, at);
                queue.push_back(next);
            }
        }
    }
    None
}
