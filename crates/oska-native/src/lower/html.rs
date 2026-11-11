//! `std.html`: `text`, `TrustedHtml.new` and `Html.concat`.
//!
//! An `Html` and a `TrustedHtml` are each a struct of one `String`, so an
//! object `[count][markup]`. `Html.raw` passes its `TrustedHtml` on as it
//! is (same layout) and `Html.render` reads the field: both are in
//! `intrinsics.rs`'s table. The rest build a new object, from text that
//! `std.rt`'s `html_escape` and `html_concat` make.

use oska_check::program::Expr;
use oska_syntax::Diagnostic;

use super::Body;
use crate::intrinsics;
use crate::ir::Temp;
use crate::layout;
use crate::NOWHERE;

impl Body<'_, '_> {
    /// The `std.html` intrinsic `name` applied to `values`, for `expr`.
    /// `None` for any other intrinsic.
    pub(super) fn html_intrinsic(
        &mut self,
        name: &str,
        values: &[Temp],
        expr: &Expr,
    ) -> Result<Option<Temp>, Diagnostic> {
        let markup = match (intrinsics::unqualified(name), values) {
            ("text", &[text]) => {
                let escaped = self.call_runtime("html_escape", vec![text], NOWHERE)?;
                self.consume(text)?;
                escaped
            }
            ("TrustedHtml.new", &[markup]) => {
                // The new object takes over the reference.
                self.owned.remove(&markup);
                markup
            }
            ("Html.concat", &[first, second]) => {
                let a = self.load(first, layout::struct_field(0));
                let b = self.load(second, layout::struct_field(0));
                let joined = self.call_runtime("html_concat", vec![a, b], NOWHERE)?;
                self.consume(first)?;
                self.consume(second)?;
                joined
            }
            _ => return Ok(None),
        };
        let object = self.alloc(layout::struct_size(1))?;
        self.store(object, layout::struct_field(0), markup);
        self.own(object, &expr.ty);
        Ok(Some(object))
    }
}
