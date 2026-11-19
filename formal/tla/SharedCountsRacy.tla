--------------------------- MODULE SharedCountsRacy ---------------------------
(* SharedCounts with a count update that is not atomic: a load, then a     *)
(* separate store (Atomic = FALSE in SharedCountsRacy.cfg). It is broken on *)
(* purpose. TLC must find a value freed while a task still holds it, and   *)
(* the formal step fails if it does not (the expect-violation line in the  *)
(* .cfg), so a model that cannot fail is caught.                           *)
EXTENDS SharedCounts
=============================================================================
