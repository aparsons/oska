# prelude

The prelude: what every module sees without `use`.

A second paragraph, which the index leaves out.

`oskac doc prelude` prints this page offline.

## Eq for Option<T>

**Kind:** impl · **Authority:** pure

```oska
impl<T> Eq for Option<T>
```

Hostile: <script>alert(1)</script>, [a link](javascript:alert(1)), ``a `tick` span``, "quotes", a back\slash, Ab< / a slash,	and a tab.

\# Not a heading

```
an open fence <b>bold</b>
## nor this
```

## List.push

**Kind:** method · **Authority:** pure

```oska
pub fn push(var self, item: T)
```

Add `item` at the end.

## String.to_lowercase

**Kind:** method · **Authority:** pure

```oska
pub fn to_lowercase(self) -> String
```

Every letter in lower case: `"ÀB".to_lowercase()` is `"àb"`.
