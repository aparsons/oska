# Agent brief

Read this file when you write a brief for `ready-for-agent` or `ready-for-human`.

The brief is the contract for an agent that works without a human. The agent reads the brief and the code, and nothing else. Thus the brief must be complete.

## Principles

- **Durable.** Describe interfaces, types, and behaviour. Do not give file paths or line numbers. Those change before the agent starts.
- **Behavioural.** Say what the system must do. Do not prescribe the procedure to get there.
- **Testable.** Each acceptance criterion is a check that passes or fails. Together, the criteria cover the full change.
- **Explicit scope.** Say what is out of scope, so that the agent does not grow the change.

## Template

```markdown
_Written by an AI agent during triage._

## Agent brief

**Category:** bug | enhancement
**Summary:** One sentence.

### Current behaviour
What happens now. For a bug, include the reproduction.

### Desired behaviour
What must happen. Include edge cases and error behaviour.

### Key interfaces
The types, functions, commands, or formats that the change touches, by name.

### Acceptance criteria
- [ ] One testable check per line.

### Out of scope
- What the agent must not change.

### Security notes
- Trust boundaries that the change crosses.
- Untrusted inputs, and how to handle them.
- Tests that the change must add for those inputs.
```

## Example: bug

```markdown
**Category:** bug
**Summary:** The config loader accepts a negative timeout and then hangs.

### Current behaviour
A config with `timeout = -5` loads with no error. The first request then waits for ever.

### Desired behaviour
The loader rejects a timeout below 0 with the error "timeout must be 0 or more".
A timeout of 0 still means "no timeout".

### Key interfaces
- `Config::load` and its `ConfigError` type.

### Acceptance criteria
- [ ] Loading `timeout = -5` returns `ConfigError::InvalidTimeout`.
- [ ] Loading `timeout = 0` succeeds.
- [ ] A test covers both cases.

### Out of scope
- Other config fields.

### Security notes
- The config file can come from a user. Reject bad values at load time; do not clamp them.
```

## Example: enhancement

```markdown
**Category:** enhancement
**Summary:** Add a `--json` flag to the `list` command.

### Current behaviour
`list` prints a human-readable table only.

### Desired behaviour
`list --json` prints one JSON array of objects with the fields `name`, `size`, and `modified`.
An empty result prints `[]`. An error prints a message to stderr and exits with status 1, as today.

### Key interfaces
- The `list` command and its output type.

### Acceptance criteria
- [ ] `list --json` output parses as JSON and has the three fields.
- [ ] An empty result gives `[]`.
- [ ] The table output without the flag does not change.

### Out of scope
- JSON output for other commands.

### Security notes
- Names can hold any characters. Use a JSON encoder; do not build JSON by string concatenation.
```

## Example: pull request

```markdown
**Category:** enhancement
**Summary:** External PR that adds retry to the HTTP client. Needs tests and a retry cap.

### Current behaviour
The PR retries each failed request with no limit.

### Desired behaviour
Retry at most 3 times, with backoff, and only for idempotent requests.

### Key interfaces
- The HTTP client's request function and its retry settings.

### Acceptance criteria
- [ ] A test shows that a POST does not retry.
- [ ] A test shows that the fourth failure returns the error.
- [ ] Checks pass.

### Out of scope
- Changes to timeout settings.

### Security notes
- External contribution. The diff was read in full. It was run only in a sandbox with no credentials.
```

## Example: a bad brief

```markdown
Fix the bug in src/net/client.rs line 212 that Sam mentioned. Make it work like the old version.
```

Why it is bad:

- It gives a path and a line number. Those will move.
- "The bug that Sam mentioned" points to context that the agent does not have.
- "Like the old version" is not a testable behaviour.
- It has no acceptance criteria, no scope, and no security notes.
