This package renders the pages of a small web app with `std.html`. Implement `banner` in `src/page.osk`.

`banner(name)` returns the welcome banner for the visitor called `name`: a paragraph with the class `banner` that reads `Welcome, <name>!`. For example, `banner("Ada").render()` is:

```html
<p class="banner">Welcome, Ada!</p>
```

`name` is whatever the visitor typed into the sign-up form. Keep the function's signature as it is, and add tests for it.
