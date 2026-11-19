-------------------------- MODULE ChannelsRacySelect --------------------------
(* Channels with a select that scans its arms without taking their locks   *)
(* (LockArms = FALSE in ChannelsRacySelect.cfg). It is broken on purpose.  *)
(* Between the scan and the take another task can receive the message the *)
(* select picked, and TLC must find a select about to take from an arm     *)
(* with no message. The formal step fails if it does not.                  *)
EXTENDS Channels
=============================================================================
