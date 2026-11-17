This package lists the products of a shop, which are in the `products` table of a SQL database, with `std.sql`. Visitors choose how the list is sorted with the `sort` parameter of the page's URL.

1. Implement `parse_sort` in `src/catalog.osk`. `parse_sort(requested)` is the sort order a visitor asked for: `name` is `Sort.Name` and `price` is `Sort.Price`. Any other value is an `UnknownSort`.
2. Implement `list_products`. `list_products(db, sort)` returns the `name` of every product, sorted by name or by price.

Keep the declarations' signatures as they are, and add tests for them.
