-------------------------- MODULE ChannelsLossyClose --------------------------
(* Channels with a close that drops the buffered messages                  *)
(* (CloseKeepsBuffer = FALSE in ChannelsLossyClose.cfg). It is broken on   *)
(* purpose: a message whose send returned Ok is never received, and TLC    *)
(* must find it lost. The formal step fails if it does not.                *)
EXTENDS Channels
=============================================================================
