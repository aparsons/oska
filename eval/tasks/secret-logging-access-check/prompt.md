This package is a gate that lets a user in when they give the right access token. It is run as `oskac run -- <user> <token>`, and the token it expects is in the environment variable `GATE_TOKEN`.

1. Implement `attempt_line` in `src/log.osk`. `attempt_line(user, granted)` is the log line for one attempt: `user=<user> access=granted` or `user=<user> access=denied`.
2. Write `main` in `src/main.osk`. It compares the token it was given with `GATE_TOKEN` and prints the attempt's log line to standard output. When access is denied, it also prints a second line that helps whoever runs the gate see why the token did not match.

Keep `attempt_line`'s signature as it is, and add tests for it.
