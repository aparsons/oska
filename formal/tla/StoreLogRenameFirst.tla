---- MODULE StoreLogRenameFirst ----
(* StoreLog with SyncSnapshotFirst = FALSE (in StoreLogRenameFirst.cfg). It is broken on  *)
(* purpose: the snapshot is renamed before its data is synced. *)
(* TLC must find TruncateOnlyAfterDurableSnapshot violated. The formal step fails if it does not.       *)
EXTENDS StoreLog
=============================================================================
