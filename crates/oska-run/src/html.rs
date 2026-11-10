//! The `std.html` runtime: escaped text and trusted markup.
//!
//! An `Html` and a `TrustedHtml` are each a struct holding one string,
//! their markup. Only [`escape`] turns text into markup; `TrustedHtml.new`
//! takes text the checker has proved trusted, and `Html.raw` passes it on
//! unescaped. Rendering hands back the markup as it is.

use std::rc::Rc;

use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::Intrinsic::Plain;
use crate::intrinsics::Table;
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

/// A struct of type `ty` holding `markup`.
fn markup(ty: &str, markup: &str) -> Value {
    Value::Struct {
        ty: Rc::from(ty),
        fields: Rc::new(vec![Value::str(markup)]),
    }
}

/// The markup an `Html` or `TrustedHtml` holds.
fn markup_of(value: &Value) -> &str {
    value.fields()[0].as_str()
}

/// `text(text)`: the text, escaped.
fn text(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(markup(HTML, &escape(args[0].as_str())))
}

/// `TrustedHtml.new(markup)`: the markup, vouched for by its author.
fn trusted_html_new(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(markup(TRUSTED_HTML, args[0].as_str()))
}

/// `Html.raw(markup)`: trusted markup, unescaped.
fn html_raw(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(markup(HTML, markup_of(&args[0])))
}

/// `Html.concat(self, other)`: `self`'s markup, then `other`'s.
fn html_concat(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let joined = format!("{}{}", markup_of(&args[0]), markup_of(&args[1]));
    Ok(markup(HTML, &joined))
}

/// `Html.render(self)`: the markup, ready to send.
fn html_render(_: &mut dyn Host, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    Ok(args[0].fields()[0].clone())
}
