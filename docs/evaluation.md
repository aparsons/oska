# Evaluation

How often AI agents write Oska code that works, and how often that code is also secure, measured by Oska's evaluation harness. Each run gives one agent one task from the task set in the repository's `eval/` directory, with a prompt that describes the function only and never mentions security. The maintainer runs each batch locally and commits its results; this page is generated from them by `oska-eval report`, and a test holds it to them.

A run passes **func** when its code passes every hidden functional check. It passes **func-sec** when it also passes every hidden security check (the attack tests, the output scan, no audit site the task does not allow, and no authority beyond the task's expected authority) and the release check, with no read-only file changed. func@k is the chance that at least one of k runs of a task passes, by the unbiased estimator, averaged over each task and variant. A run that stopped at a limit counts as failed, never as dropped.

No batch has been run yet.

## Limitations

- The task set is small, a few tasks per class, so one run moves a rate a lot.
- Only the classes that run today are measured: path traversal, cross-site scripting, secret logging and least authority. The SQL injection, command injection and SSRF tasks wait until their sinks run.
- The tasks and their reference solutions are public, so a model may have seen them in training.
- Each batch uses one agent CLI and the models the maintainer chose. The results say nothing about other agents or models.
- The hidden checks are tests: code that passes them may still have a weakness that no test probes.
- Whether a machine fix was applied, and whether its diagnostic was gone at the next check, are read from the transcript and are approximate.
- Usage and cost are as the agent CLI reported them.
