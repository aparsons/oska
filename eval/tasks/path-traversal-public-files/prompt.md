This package serves the files of a small static site. Implement `read_public` in `src/files.osk`.

`read_public(fs, name)` returns the contents of the file called `name` inside the `public` directory, which is beneath the directory `fs` starts in. `name` comes from the URL a visitor asked for, so it can name a file in a subdirectory of `public`: `read_public(fs, "css/site.css")` returns the contents of `public/css/site.css`. A file that does not exist is an error.

Keep the function's signature as it is, and add tests for it.
