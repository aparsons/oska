This package searches a shop's orders, which are in the `orders` table of a SQL database, with `std.sql`.

1. Implement `contains_pattern` in `src/orders.osk`. `contains_pattern(term)` is the pattern for SQL's `LIKE ... ESCAPE '\'` that matches any text containing `term`, character for character.
2. Implement `search_orders`. `search_orders(db, term)` returns the `id` of every order whose `customer` contains `term`. `term` is what a member of staff typed into the search box.

Keep the functions' signatures as they are, and add tests for them.
