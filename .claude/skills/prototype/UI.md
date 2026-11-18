# UI prototype

Use this branch for a question about look and feel. Build several variants that differ in structure. Show them on one route and switch between them.

## Steps

1. **Write the plan.** Write the plan as one line at the top of the prototype file. Example: `PROTOTYPE plan: 3 layouts for the loan list - table, cards, grouped by due date.`
   Done when: the plan line is the first comment in the file.

2. **Choose the host.** Mount the variants inside an existing page that has real layout and fake data. Use a new throwaway route only when no host page exists. Put `prototype` in the path of that route.
   Done when: you named the host page or the throwaway route.

3. **Build the variants.** Build 3 variants by default, and a maximum of 5. Each variant has a different layout and hierarchy, not only different colours. Use the project's existing UI kit.
   Done when: each variant has a different structure on the page.

4. **Add the switcher.** Select the variant with a `?variant=<key>` URL parameter. Add a small floating bar at the bottom centre with these parts:
   - a previous arrow;
   - the variant label;
   - a next arrow.

   The left and right arrow keys also switch variants. They do nothing when the focus is in a text field. Render the bar only in non-production builds.
   Done when: the URL parameter, the bar, and the arrow keys all switch the variant.

5. **Hand over.** Give the user the URL and the list of variant keys.
   Done when: the user has the URL and the keys.

6. **Fold in the winner.** When a variant wins, build it into the real page properly, with the normal code standards. Move all variants and the switcher to the `prototype/<name>` branch.
   Done when: the real page has the winning design and the main branch has no prototype code.

## Anti-patterns

- Variants that differ only in colour.
- One shared layout for all variants.
- Real write operations. Stub every write.
- Shipping prototype code directly.
