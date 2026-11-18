# Logic prototype

Use this branch for a question about logic or state. The result is one self-contained HTML file. A person who is not a developer can open it with a double-click.

## Steps

1. **Write the question at the top.** Put the question in the page title and in a visible heading.
   Done when: the question is the first thing on the page.

2. **Write the logic as one pure module.** Put it in one `<script>` block. Choose the form that fits the question: a reducer, a state machine, pure functions, or a small class. The module has no DOM access. So you can move it to the real code later.
   Done when: the module uses no `document`, `window`, or event objects.

3. **Build a thin shell around the logic.** The page code only reads state from the module and calls its actions. It holds no rules.
   Done when: each rule in the question lives in the module, not in the page code.

4. **Lay out the page.** Include these parts:
   - the title and the question;
   - a state panel with labelled fields, not raw JSON, that highlights what changed after each action;
   - one button for each action, for free play;
   - guided scenarios in tabs.
   Done when: each part is present and each button changes the state panel.

5. **Write the scenarios.** Give each scenario a plain description and the ordered list of buttons to press. Each scenario starts from a reset state. Include at least these scenarios:
   - a happy path;
   - a tricky edge case;
   - an attempt at something that must not be allowed.
   Done when: the three scenarios run from a reset state and show the expected result.

## Style

- Use domain words from `GLOSSARY.md` for labels.
- Use a clean, calm design: one font stack, plenty of space, one accent colour.
- Use system fonts. Do not load web fonts.

## Anti-patterns

- Tests. The prototype is thrown away.
- A real database.
- General code for cases that the question does not ask about.
- Logic mixed with page code.
- Frameworks or build tools.
- Shipping the page to production.

## Example skeleton

```html
<!doctype html>
<meta charset="utf-8">
<title>PROTOTYPE: Can a member renew a reserved loan?</title>
<style>/* inline styles only */</style>
<h1>PROTOTYPE: Can a member renew a reserved loan?</h1>
<section id="state"></section>
<section id="actions"></section>
<section id="scenarios"></section>
<script>
// Logic module: pure, no DOM.
const initial = () => ({ loans: {}, reservations: {} });
function apply(state, action) { /* return a new state or an error */ }

// Shell: renders state, wires buttons.
</script>
```
