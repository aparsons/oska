# Oska checker report

## Diagnostic codes

| Code | Meaning | Fix |
| --- | --- | --- |
| `E0200` | Unknown name "x" with a \\ in it, as in `Strng`. See [the tour](#tour). | Replace the name with the closest one. |
| `E0201` | A trait <script>alert(1)</script> where a type is expected. Then more. | None. |
| `E0140` | An or-pattern such as `A \| B`. | None: write one arm each. |

### Examples

#### `E0200`

```oska error=E0200
fn f(x: Strng) {}
```

```oska fixed=E0200
fn f(x: String) {}
```

#### `E0201`

```oska error=E0201
fn show(x: Display) {
    let note = "<b>\"quoted\"</b>"
}

```

```text
not an example
| `E0999` | Not a code row. | None. |
```
