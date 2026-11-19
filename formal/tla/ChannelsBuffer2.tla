---------------------------- MODULE ChannelsBuffer2 ----------------------------
(* Channels with a buffer of 2 on channel 2 (BufferSize = 2 in             *)
(* ChannelsBuffer2.cfg). Channels.cfg, with a buffer of 1, never holds two *)
(* buffered messages, so the order inside a buffer is checked here. It     *)
(* must pass, like Channels.                                               *)
EXTENDS Channels
=============================================================================
