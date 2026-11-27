---- MODULE StoreLogAckEarly ----
(* StoreLog with AckAfterSync = FALSE (in StoreLogAckEarly.cfg). It is broken on  *)
(* purpose: a write is acknowledged before its sync. *)
(* TLC must find AckedSurvive violated. The formal step fails if it does not.       *)
EXTENDS StoreLog
=============================================================================
