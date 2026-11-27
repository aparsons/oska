---- MODULE StoreLogNoChecksum ----
(* StoreLog with VerifyChecksum = FALSE (in StoreLogNoChecksum.cfg). It is broken on  *)
(* purpose: open accepts a torn record. *)
(* TLC must find ChainValid violated. The formal step fails if it does not.       *)
EXTENDS StoreLog
=============================================================================
