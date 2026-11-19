//! The fenced code blocks of the catalog docs (`docs/checker.md`,
//! `docs/runtime.md`), as `oskac explain` and the catalog tests read them.
//! A block opens with ```` ``` ```` or `~~~` and closes only with a line
//! of its own fence and nothing else, so a block fenced with `~~~` may
//! hold a ```` ``` ```` fence, as `E0001`'s example does. The site
//! generator (`site/build.osk`) reads the docs the same way.

/// What one line of a doc is to its fenced blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceLine<'a> {
    /// The line opens a block; the text after the fence is its info
    /// string, such as `oska error=E0001`.
    Opens(&'a str),
    /// The line closes the open block.
    Closes,
    /// Any other line: a block's body while one is open, else text.
    Other,
}

/// The fence of the block that is open, while a doc is read line by line.
#[derive(Debug, Default)]
pub struct Fences {
    open: Option<&'static str>,
}

impl Fences {
    /// What `line`, the next line of the doc, is.
    pub fn line<'a>(&mut self, line: &'a str) -> FenceLine<'a> {
        match self.open {
            Some(fence) => {
                if line
                    .strip_prefix(fence)
                    .is_some_and(|rest| rest.trim().is_empty())
                {
                    self.open = None;
                    FenceLine::Closes
                } else {
                    FenceLine::Other
                }
            }
            None => {
                for fence in ["~~~", "```"] {
                    if let Some(info) = line.strip_prefix(fence) {
                        self.open = Some(fence);
                        return FenceLine::Opens(info);
                    }
                }
                FenceLine::Other
            }
        }
    }
}
