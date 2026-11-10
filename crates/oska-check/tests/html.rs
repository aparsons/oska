//! `std.html` as a page generator uses it (#71): escaped text goes in,
//! rendered markup goes out to a file, and neither door lets untrusted
//! text become markup.

mod common;

use common::{assert_authority, assert_errors, assert_ok};

#[test]
fn file_contents_are_escaped_into_a_page_that_is_written_out() {
    assert_ok(
        "use std.html\n\
         fn page(docs: DirRead, site: DirWrite) -> Result<(), IoError> {\n    \
         let readme = docs.read_to_string(Path.new(\"README.md\"))?\n    \
         let body = html.Html.raw(html.TrustedHtml.new(\"<pre>\")).concat(html.text(readme))\n    \
         site.write(Path.new(\"index.html\"), body.render())\n}",
    );
}

#[test]
fn rendered_html_cannot_be_vouched_for_as_trusted_markup() {
    assert_errors(
        "use std.html\nfn f(name: Untrusted<String>) -> html.TrustedHtml {\n    \
         html.TrustedHtml.new(html.text(name).render())\n}",
        &[("E0300", "html.text(name).render()")],
    );
}

#[test]
fn trusted_html_cannot_be_constructed_from_a_string() {
    assert_errors(
        "use std.html\nfn f(markup: String) -> html.TrustedHtml {\n    \
         html.TrustedHtml(markup: markup)\n}",
        &[("E0271", "TrustedHtml")],
    );
}

#[test]
fn building_html_needs_no_capability() {
    assert_authority(
        "use std.html\nfn page(title: Untrusted<String>) -> html.Html {\n    \
         html.Html.raw(html.TrustedHtml.new(\"<h1>\")).concat(html.text(title))\n}",
        "page",
        &[],
        true,
    );
}
