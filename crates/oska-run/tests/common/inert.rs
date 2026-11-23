// SPDX-License-Identifier: Apache-2.0
//! What a browser would make of a page the site generator wrote: the
//! check that nothing in it could run, and a reader for its start tags.

use std::collections::BTreeMap;

/// Every element the site may emit, with the attributes it may carry.
/// Anything else in a page, an `<iframe>` or an `onerror=`, means text
/// got out of its escaping.
pub const ELEMENTS: &[(&str, &[&str])] = &[
    ("html", &["lang"]),
    ("head", &[]),
    (
        "meta",
        &["charset", "name", "property", "content", "http-equiv"],
    ),
    ("title", &[]),
    ("link", &["rel", "href", "type"]),
    ("script", &["type", "src"]),
    ("aside", &["aria-label"]),
    ("body", &[]),
    ("header", &[]),
    ("nav", &["aria-label"]),
    ("main", &["id"]),
    ("footer", &[]),
    ("a", &["class", "href", "aria-current"]),
    ("h1", &["id"]),
    ("h2", &["id"]),
    ("h3", &["id"]),
    ("h4", &["id"]),
    ("h5", &["id"]),
    ("h6", &["id"]),
    ("p", &[]),
    ("ul", &[]),
    ("ol", &["start"]),
    ("li", &[]),
    ("pre", &["id"]),
    ("section", &["class", "aria-labelledby"]),
    ("button", &["type", "data-copy", "hidden"]),
    ("code", &[]),
    ("blockquote", &[]),
    ("table", &[]),
    ("thead", &[]),
    ("tbody", &[]),
    ("tr", &[]),
    ("th", &[]),
    ("td", &[]),
    ("strong", &[]),
    ("em", &[]),
    ("img", &["src", "alt"]),
];

/// `text` with its character references decoded, as a browser decodes an
/// attribute value: the named ones the escaper writes, and numeric ones.
pub fn decode_references(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `url`, an attribute value as the browser decodes it, can only
/// navigate or load: a relative URL, a fragment, or `https:`, `http:`, or
/// `mailto:`. Browsers drop tabs and line breaks anywhere in a URL and
/// ignore the case of its scheme, so they are dropped and ignored here.
pub fn is_inert_url(url: &str) -> bool {
    let url: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase();
    let scheme_end = url.find(':');
    let path_start = url.find(['/', '?', '#']);
    match (scheme_end, path_start) {
        (None, _) => true,
        (Some(colon), Some(slash)) if slash < colon => true,
        _ => ["https:", "http:", "mailto:"]
            .iter()
            .any(|ok| url.starts_with(ok)),
    }
}

/// Assert that `page`, an HTML page the generator wrote, holds nothing a
/// browser would run: only the elements and attributes in [`ELEMENTS`],
/// every attribute double-quoted, every URL inert, no comment, and the
/// site's CSP before anything it governs.
pub fn assert_inert(name: &str, page: &str) {
    let rest = page
        .strip_prefix("<!DOCTYPE html>\n")
        .unwrap_or_else(|| panic!("{name} should start with a doctype:\n{page}"));
    let mut rest = rest;
    let mut seen_csp = false;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest
            .find('>')
            .unwrap_or_else(|| panic!("{name}: a `<` that never closes:\n{page}"));
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        let tag = tag.strip_prefix('/').unwrap_or(tag);
        let element: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '!' || *c == '-')
            .collect();
        let allowed = ELEMENTS
            .iter()
            .find(|(e, _)| *e == element)
            .unwrap_or_else(|| panic!("{name}: element `<{tag}>` is not allowed:\n{page}"))
            .1;
        let mut attributes = tag[element.len()..].trim_start();
        while !attributes.is_empty() {
            let eq = attributes
                .find("=\"")
                .unwrap_or_else(|| panic!("{name}: `<{tag}>`: an unquoted attribute"));
            let attribute = &attributes[..eq];
            assert!(
                allowed.contains(&attribute),
                "{name}: `{attribute}` on `<{element}>` is not allowed: `<{tag}>`"
            );
            let value_and_rest = &attributes[eq + 2..];
            let end = value_and_rest
                .find('"')
                .unwrap_or_else(|| panic!("{name}: `<{tag}>`: an attribute that never ends"));
            let value = decode_references(&value_and_rest[..end]);
            if attribute == "href" || attribute == "src" {
                assert!(
                    is_inert_url(&value),
                    "{name}: `{attribute}=\"{value}\"` could run: `<{tag}>`"
                );
            }
            if attribute == "http-equiv" {
                assert_eq!(value, "Content-Security-Policy", "{name}");
                seen_csp = true;
            }
            if element == "link" || element == "img" {
                assert!(seen_csp, "{name}: `<{tag}>` comes before the CSP");
            }
            attributes = value_and_rest[end + 1..].trim_start();
        }
    }
    assert!(seen_csp, "{name} has no CSP");
}

pub fn assert_site_inert(site: &BTreeMap<String, String>) {
    for (name, text) in site {
        if name.ends_with(".html") {
            assert_inert(name, text);
        }
    }
}

/// One start tag of a page: its element and its attributes, decoded.
pub struct Tag {
    pub element: String,
    pub attributes: Vec<(String, String)>,
}

impl Tag {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The start tags of `page`, in order. Pages have passed
/// [`assert_inert`], so every attribute is double-quoted.
pub fn tags_of(page: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut rest = page;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest.find('>').expect("a tag that closes");
        let inside = &rest[..close];
        rest = &rest[close + 1..];
        if inside.starts_with('/') || inside.starts_with('!') {
            continue;
        }
        let element: String = inside
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let mut attributes = Vec::new();
        let mut text = inside[element.len()..].trim_start();
        while let Some(eq) = text.find("=\"") {
            let value_and_rest = &text[eq + 2..];
            let end = value_and_rest.find('"').expect("an attribute that ends");
            attributes.push((
                text[..eq].to_string(),
                decode_references(&value_and_rest[..end]),
            ));
            text = value_and_rest[end + 1..].trim_start();
        }
        tags.push(Tag {
            element,
            attributes,
        });
    }
    tags
}

/// The URL a tag links to or loads, if it does.
pub fn url_of(tag: &Tag) -> Option<&str> {
    match tag.element.as_str() {
        "a" | "link" => tag.attribute("href"),
        "img" | "script" => tag.attribute("src"),
        _ => None,
    }
}
