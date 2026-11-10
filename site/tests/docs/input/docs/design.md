# Oska design notes

Decisions are in [`adr/`](adr/), terms in [`GLOSSARY.md`](../GLOSSARY.md), and the
background in [the research](research/2026-11-report.md).

## Principles

1. **Secure by default.** The safe path is the shortest.
2. **No ambient authority.** See [ADR 0002](adr/0002-capabilities.md#decision).
3. **Legible.** A `snake_case_name` stays as it is, and so does `2 * 3 * 4`.

## Capabilities

- `Fs` attenuates:
  - to `FsRead` or `FsWrite`
  - to one directory, with `restrict_to`
- **Confinement.** A directory capability resolves every path:
  1. An empty path is refused.
  2. The path is canonicalized.

  **Residual risk.** A race remains, as [Untrusted data](#untrusted-data-and-sinks) explains.
- A code block in an item:

  ```oska
  fn main(fs: FsRead) {
      let docs = fs.restrict_to("docs")
  }
  ```

3. A list that starts at three
4. keeps its number.

## Untrusted data and sinks

| Sink | Data position | Structural position |
| --- | --- | --- |
| SQL (`std.sql`) | Parameters | The query, an `sql"..."` literal |
| Closures | `\|x\| x` | `` `|` `` in code |

Text is *untrusted* when it comes from outside, and __trusted__ text is
written in the program. An escaped \*star\* is not emphasis, and
``a ` b`` is code.

### Sinks

Headings at every level get an anchor.

#### Fourth

##### Fifth

###### Sixth
