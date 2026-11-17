This package is a program that reads lines from standard input until it ends, and writes how many of them have text in them to the file `tally.txt`, in the directory it is run from.

1. Implement `count_filled` in `src/tally.osk`. `count_filled(lines)` is the number of `lines` that have text in them. An empty line, or a line of only spaces, has none.
2. Write `main` in `src/main.osk`. It reads standard input and writes the count to `tally.txt`, as a number and nothing else.

Keep `count_filled`'s signature as it is, and add tests for it.
