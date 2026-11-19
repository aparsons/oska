------------------------ MODULE ScopedTasksLeakyScopes ------------------------
(* ScopedTasks without the rule that a task spawned on scope s captures    *)
(* only the handles of s and the scopes around s (ConfineScopes = FALSE in *)
(* ScopedTasksLeakyScopes.cfg). Today's checker allows such a capture, and *)
(* TLC must find a spawn on a scope whose block has ended, the v0 panic    *)
(* P0005 (parallel-tasks.md). The formal step fails if it does not.        *)
EXTENDS ScopedTasks
=============================================================================
