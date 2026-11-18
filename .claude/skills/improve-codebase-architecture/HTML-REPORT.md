# HTML report

The report is one self-contained HTML file. It has no remote scripts, styles, or fonts. All diagrams are inline SVG or plain HTML with inline CSS.

## Structure

1. **Header.** Show the repo name, the date, the scope, and a legend. The legend explains the strength badges, the dependency category tags, and the diagram colours.
2. **Candidate cards.** One card for each candidate. The card parts are in `SKILL.md`. Order the cards from strongest to weakest.
3. **Top recommendation.** One short section at the end. Name the candidate and give the reason in one or two sentences.

## Diagram patterns

Pick the pattern that shows the friction most clearly. Put the before diagram and the after diagram side by side, at the same scale.

- **Call or dependency graph.** Boxes for modules, arrows for calls. Before: many boxes and crossing arrows. After: one box that holds them.
- **Boxes and arrows in inline SVG.** Use `<rect>`, `<text>`, and `<line>` with an arrow marker. Keep labels short.
- **Layered cross-section.** Horizontal bands for layers. Show where a concept cuts through many bands before, and one band after.
- **Interface and implementation size bars.** Two bars for each module: interface width on top, implementation below. A deep module has a short top bar and a long bottom bar.
- **Call tree collapse.** Before: a deep tree of small calls. After: the tree folds into one module with one entry point.

## Style

- Use a calm editorial style: a readable serif or system font, wide margins, and plenty of white space.
- Use one accent colour for headings and badges.
- Use red only for leaks, such as a detail that crosses a seam.
- Use amber only for warnings, such as an ADR conflict.
- Use system fonts only. Do not load web fonts.

## Tone

- Write plain, short sentences.
- Use only the glossary words from `codebase-design` and from `GLOSSARY.md`.
- Say what the reader gains, not how clever the change is.

## Escaping

Escape every piece of text that comes from the code before you put it in the HTML. Replace these characters:

| Character | Entity |
|---|---|
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |
| `'` | `&#39;` |

Escape text inside SVG `<text>` elements the same way.

## Skeleton

```html
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>Architecture review: oska, crates/oska-run, 2026-11-18</title>
<style>
  body { font: 16px/1.6 Georgia, serif; max-width: 60rem; margin: 3rem auto; color: #222; }
  .card { border: 1px solid #ddd; padding: 1.5rem; margin: 2rem 0; }
  .badge { color: #fff; background: #1f5f8b; padding: 0 .5rem; }
  .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  .leak { stroke: #b00020; } .warn { background: #fff4d6; }
</style>
<header><!-- repo, date, scope, legend --></header>
<main><!-- one <section class="card"> per candidate --></main>
<footer><!-- top recommendation --></footer>
</html>
```
