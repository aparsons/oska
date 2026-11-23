// SPDX-License-Identifier: Apache-2.0
//! The `std.html` runtime: escaped text and trusted markup.
//!
//! An `Html` and a `TrustedHtml` are each a struct holding one string,
//! their markup. Only [`escape`] turns text into markup; `TrustedHtml.new`
//! takes text the checker has proved trusted, and `Html.raw` passes it on
//! unescaped. Rendering hands back the markup as it is.

use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::Plain;
use crate::intrinsics::{Args, Table};
use crate::memory;
use crate::panics::Panic;
use crate::value::Value;

/// The module these intrinsics implement.
pub const MODULE: &str = "std.html";

/// Every `std.html` intrinsic, by the name the checked program calls it
/// by when the program declares none of the module's names itself.
pub const INTRINSICS: Table = &[
    ("Html.concat", Plain(html_concat)),
    ("Html.raw", Plain(html_raw)),
    ("Html.render", Plain(html_render)),
    ("TrustedHtml.new", Plain(trusted_html_new)),
    ("text", Plain(text)),
];

const HTML: &str = "Html";
const TRUSTED_HTML: &str = "TrustedHtml";

/// `text`, escaped for HTML: safe in an element's content and in a
/// quoted attribute value.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A struct of type `ty` holding the markup `make` builds, `len` bytes of
/// it, made at `span`. Under `--max-memory` it is charged before it is
/// built.
fn markup(ty: &str, len: usize, span: Span, make: impl FnOnce() -> String) -> Result<Value, Panic> {
    let markup = memory::text_of_len(len, span, make)?;
    memory::structure(ty, vec![markup], span)
}

/// The length of `text` escaped ([`escape`]).
fn escaped_len(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '&' => 5,
            '<' | '>' => 4,
            '"' => 6,
            '\'' => 5,
            c => c.len_utf8(),
        })
        .sum()
}

/// The markup an `Html` or `TrustedHtml` holds.
fn markup_of(value: &Value) -> &str {
    value.fields()[0].as_str()
}

/// `text(text)`: the text, escaped.
fn text(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let text = args[0].as_str();
    let len = if memory::metered() {
        escaped_len(text)
    } else {
        0
    };
    markup(HTML, len, span, || escape(text))
}

/// `TrustedHtml.new(markup)`: the markup, vouched for by its author.
fn trusted_html_new(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    if !memory::metered() {
        // The markup is the text itself, shared rather than copied (#572).
        return memory::structure(TRUSTED_HTML, vec![args.swap_remove(0)], span);
    }
    let text = args[0].as_str();
    markup(TRUSTED_HTML, text.len(), span, || text.to_string())
}

/// `Html.raw(markup)`: trusted markup, unescaped.
fn html_raw(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    if !memory::metered() {
        // The trusted markup's text, shared rather than copied (#572).
        let markup = args[0].fields()[0].clone();
        return memory::structure(HTML, vec![markup], span);
    }
    let text = markup_of(&args[0]);
    markup(HTML, text.len(), span, || text.to_string())
}

/// `Html.concat(self, other)`: `self`'s markup, then `other`'s.
fn html_concat(_: &mut dyn Host, args: &mut Args, span: Span) -> Result<Value, Panic> {
    let (first, second) = (markup_of(&args[0]), markup_of(&args[1]));
    let len = first.len() + second.len();
    markup(HTML, len, span, || {
        // Made at its length, not grown as `format!` would grow it.
        let mut both = String::with_capacity(len);
        both.push_str(first);
        both.push_str(second);
        both
    })
}

/// `Html.render(self)`: the markup, ready to send.
fn html_render(_: &mut dyn Host, args: &mut Args, _: Span) -> Result<Value, Panic> {
    Ok(args[0].fields()[0].clone())
}
