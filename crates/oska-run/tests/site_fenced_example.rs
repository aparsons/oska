// SPDX-License-Identifier: Apache-2.0
//! A catalog example that holds a Markdown fence (ticket #449).
//!
//! `E0001`'s example is a module wrapped in a ```` ``` ```` fence, so the
//! catalog fences that example with `~~~`. The generator reads it whole,
//! and writes it on the code's page in a `~~~` fence of its own, so that
//! the inner fence stays code.

mod common;

use std::collections::BTreeMap;

use common::site::{self, MINIMAL_REPO};

const CHECKER: &str = "# Checker

## Syntax diagnostic codes

| Code | Meaning | Fix | Applicability |
| --- | --- | --- | --- |
| `E0001` | A character that starts no token. | Remove the fence. | machine |

#### `E0001`

~~~oska error=E0001
```oska
fn main() {}
```
~~~

```oska fixed=E0001
fn main() {}
```
";

fn fenced_site() -> BTreeMap<String, String> {
    site::site_of(MINIMAL_REPO, &[("docs/checker.md", CHECKER)])
}

#[test]
fn codes_json_holds_an_example_fenced_with_tildes_whole() {
    let site = fenced_site();
    let codes = &site["codes.json"];
    assert!(
        codes.contains(
            "\"example\":\"```oska\\nfn main() {}\\n```\\n\",\"fix\":\"fn main() {}\\n\""
        ),
        "{codes}"
    );
}

#[test]
fn a_code_page_fences_an_example_that_holds_a_fence_with_tildes() {
    let site = fenced_site();
    let page = &site["codes/E0001.md"];
    assert!(
        page.contains("~~~oska\n```oska\nfn main() {}\n```\n~~~\n"),
        "{page}"
    );
    assert!(
        page.contains("```oska\nfn main() {}\n```\n\nListed"),
        "{page}"
    );
    let html = &site["codes/E0001.html"];
    assert!(
        html.contains("<pre><code>```oska\nfn main() {}\n```\n</code></pre>"),
        "{html}"
    );
}
