//! The `std.html` runtime: escaped text, trusted markup, and rendering.

mod common;

use common::{assert_runs, run_on};
use oska_run::FakeHost;

/// A program that prints `expr`, an `html.Html`, rendered.
fn printing(expr: &str) -> String {
    format!(
        "use std.html\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println({expr}.render())?\n    \
         Ok(())\n}}\n"
    )
}

#[test]
fn text_is_escaped_for_html() {
    assert_runs(
        &printing("html.text(\"<script>alert('x & \\\"y\\\"')</script>\")"),
        &[],
        "&lt;script&gt;alert(&#39;x &amp; &quot;y&quot;&#39;)&lt;/script&gt;\n",
    );
}

#[test]
fn trusted_markup_is_inserted_unescaped_around_escaped_text() {
    assert_runs(
        &printing(
            "html.Html.raw(html.TrustedHtml.new(\"<p class=\\\"x\\\">\"))\
             .concat(html.text(\"a < b\"))\
             .concat(html.Html.raw(html.TrustedHtml.new(\"</p>\")))",
        ),
        &[],
        "<p class=\"x\">a &lt; b</p>\n",
    );
}

#[test]
fn untrusted_input_comes_out_escaped() {
    let src = "use std.html\n\
               fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {\n    \
               let line = input.read_line()?.unwrap_or(\"\")\n    \
               io.println(html.text(line).render())?\n    \
               Ok(())\n}\n";
    let host = FakeHost::new().with_stdin("<img src=x onerror=alert(1)>\n");
    let outcome = run_on(host, src, &[]);
    assert_eq!(outcome.diagnostics, Vec::new());
    assert_eq!(outcome.stdout, "&lt;img src=x onerror=alert(1)&gt;\n");
}

#[test]
fn a_program_declaring_one_of_the_modules_names_still_reaches_it() {
    let src = "use std.html\n\
               fn text(n: Int) -> Int {\n    n\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               let _ = text(1)\n    \
               io.println(html.text(\"&\").render())?\n    \
               Ok(())\n}\n";
    assert_runs(src, &[], "&amp;\n");
}
