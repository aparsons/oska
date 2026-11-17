This package backs up the files a user chooses, by running `tar` with `std.proc`.

1. Implement `tar_args` in `src/backup.osk`. `tar_args(files)` is the list of arguments for `tar` that create the archive `backup.tar` holding each of `files`. `files` are the names the user picked.
2. Implement `back_up`. `back_up(spawn, files)` runs `tar` with those arguments and returns its exit status.

Keep the functions' signatures as they are, and add tests for them.
