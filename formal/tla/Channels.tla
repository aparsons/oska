------------------------------- MODULE Channels -------------------------------
(* `Sched` channels and `select` on parallel tasks (ADR 0019, issue #433). *)
(* parallel-tasks.md beside it is the design this model checks, in its     *)
(* section "Channels and select".                                          *)
(*                                                                         *)
(* The scheduler is ScopedTasks': a fixed pool of workers runs ready      *)
(* tasks, and a task that waits parks, giving its worker back. The task    *)
(* tree is fixed to keep TLC fast: `main` opens one scope and spawns the   *)
(* other tasks into it at the start, and its last step ends the scope,     *)
(* which waits for them. ScopedTasks checks spawn, join and scopes in      *)
(* general; this model adds the waits that channels bring.                 *)
(*                                                                         *)
(* Every program up to the bound is in the state space: every task holds   *)
(* every channel, and a running task may at any step send on, receive      *)
(* from or close any channel, `select` over any two or more channels, or   *)
(* end. Channel c has capacity c - 1, so channel 1 is a rendezvous and     *)
(* channel 2 buffers one message. Each message sent is a new id, so TLC    *)
(* can tell where every message is.                                        *)
(*                                                                         *)
(* A channel operation is one atomic step, as under the channel's lock. A  *)
(* `select` is two steps: it scans its arms and picks a ready one, then it *)
(* takes the message. With LockArms it holds the locks of all its arms     *)
(* from the scan to the take, as an implementation that locks them in one  *)
(* fixed order does. Without it (ChannelsRacySelect) another task can      *)
(* empty the picked arm between the two, and TLC must find that. With      *)
(* CloseKeepsBuffer, close keeps the buffered messages for receivers;      *)
(* without it (ChannelsLossyClose) close drops them, and TLC must find a   *)
(* lost message.                                                           *)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
  MaxTasks,     \* task ids 0 .. MaxTasks-1; task 0 is `main`
  NumChannels,  \* channel ids 1 .. NumChannels; channel c holds c - 1
  MaxMsgs,      \* the most sends in one run; message ids 1 .. MaxMsgs
  Workers,      \* the scheduler's workers (OS threads)
  LockArms,     \* TRUE: a select holds its arms' locks from scan to take
  CloseKeepsBuffer  \* TRUE: close keeps buffered messages for receivers

Main == 0
Tasks == 0 .. MaxTasks - 1
Chans == 1 .. NumChannels
Msgs == 1 .. MaxMsgs
None == 0 - 1  \* no task, channel or message
Capacity(c) == c - 1

\* The states of a task. A task parked in "recv", "send" or "select" waits
\* on the channels in on[t]; `main` parked in "closing" waits for the
\* other tasks to end.
Parked == {"recv", "send", "select", "closing"}
States == {"ready", "running", "done"} \cup Parked

VARIABLES
  st,        \* st[t]: the task's state
  on,        \* on[t]: the channels a parked task waits on, or a select's arms
  hold,      \* hold[t]: the message a task parked in send holds
  pick,      \* pick[t]: the arm a select has picked and not yet taken from
  ended,     \* ended[t]: the channels t was told are closed and drained
  buf,       \* buf[c]: c's buffered messages, oldest first
  closed,    \* closed[c]: c is closed
  lock,      \* lock[c]: the task whose select holds c's lock
  sent,      \* the messages sent so far (ids 1 .. sent)
  chanOf,    \* chanOf[m]: the channel m was sent on
  closedAt,  \* closedAt[c]: how many messages were sent when c closed
  got,       \* got[m]: how many times a task received m
  returned,  \* returned[m]: m's send failed, and the sender kept m
  deadlock   \* the runtime found every task parked, and stopped the program

vars == <<st, on, hold, pick, ended, buf, closed, lock, sent, chanOf,
          closedAt, got, returned, deadlock>>

TypeOK ==
  /\ st \in [Tasks -> States]
  /\ on \in [Tasks -> SUBSET Chans]
  /\ hold \in [Tasks -> Msgs \cup {None}]
  /\ pick \in [Tasks -> Chans \cup {None}]
  /\ ended \in [Tasks -> SUBSET Chans]
  /\ \A c \in Chans : buf[c] \in Seq(Msgs) /\ Len(buf[c]) <= Capacity(c)
  /\ closed \in [Chans -> BOOLEAN]
  /\ lock \in [Chans -> Tasks \cup {None}]
  /\ sent \in 0 .. MaxMsgs
  /\ chanOf \in [Msgs -> Chans \cup {None}]
  /\ closedAt \in [Chans -> 0 .. MaxMsgs \cup {None}]
  /\ got \in [Msgs -> 0 .. MaxTasks]
  /\ returned \in [Msgs -> BOOLEAN]
  /\ deadlock \in BOOLEAN

Init ==
  /\ st = [t \in Tasks |-> "ready"]
  /\ on = [t \in Tasks |-> {}]
  /\ hold = [t \in Tasks |-> None]
  /\ pick = [t \in Tasks |-> None]
  /\ ended = [t \in Tasks |-> {}]
  /\ buf = [c \in Chans |-> <<>>]
  /\ closed = [c \in Chans |-> FALSE]
  /\ lock = [c \in Chans |-> None]
  /\ sent = 0
  /\ chanOf = [m \in Msgs |-> None]
  /\ closedAt = [c \in Chans |-> None]
  /\ got = [m \in Msgs |-> 0]
  /\ returned = [m \in Msgs |-> FALSE]
  /\ deadlock = FALSE

Running(t) == st[t] = "running"
\* Running, and not in the middle of a select.
Idle(t) == Running(t) /\ pick[t] = None
Busy == Cardinality({t \in Tasks : Running(t)})
Free(c) == lock[c] = None
SendersOn(c) == {u \in Tasks : st[u] = "send" /\ on[u] = {c}}
\* Receivers parked on c: in a receive, or in a select with an arm on c.
ReceiversOn(c) == {u \in Tasks : st[u] \in {"recv", "select"} /\ c \in on[u]}
\* The sender that parked first holds the oldest message.
Oldest(c) == CHOOSE u \in SendersOn(c) :
               \A v \in SendersOn(c) : hold[u] <= hold[v]
\* A receive on c takes a message at once.
HasMsg(c) == Len(buf[c]) > 0 \/ SendersOn(c) # {}
\* A receive on c returns None: c is closed and has no message left.
Drained(c) == closed[c] /\ ~HasMsg(c)

\* --- the scheduler -----------------------------------------------------------

\* A free worker takes a ready task.
Schedule(t) ==
  /\ Busy < Workers
  /\ st[t] = "ready"
  /\ st' = [st EXCEPT ![t] = "running"]
  /\ UNCHANGED <<on, hold, pick, ended, buf, closed, lock, sent, chanOf,
                 closedAt, got, returned, deadlock>>

\* --- channel operations -----------------------------------------------------

\* Running task t takes c's oldest message: the head of the buffer, else
\* (a rendezvous) the message of the oldest parked sender. That sender's
\* message moves into the room the take made, and the sender wakes: its
\* send returns Ok.
TakeMsg(t, c) ==
  LET s == IF SendersOn(c) = {} THEN None ELSE Oldest(c)
      m == IF Len(buf[c]) > 0 THEN Head(buf[c]) ELSE hold[s]
  IN /\ got' = [got EXCEPT ![m] = @ + 1]
     /\ buf' = [buf EXCEPT ![c] =
          IF Len(@) = 0 THEN @
          ELSE IF s = None THEN Tail(@) ELSE Append(Tail(@), hold[s])]
     /\ st' = [u \in Tasks |-> IF u = s THEN "ready" ELSE st[u]]
     /\ on' = [u \in Tasks |-> IF u = s THEN {} ELSE on[u]]
     /\ hold' = [u \in Tasks |-> IF u = s THEN None ELSE hold[u]]

\* `tx.send(v)`. On a closed channel it returns Err(v): the sender keeps
\* the message. Else it hands the message to a parked receiver, which
\* wakes with it, or buffers it if there is room, or parks until a
\* receiver takes it or the channel closes.
Send(t, c) ==
  /\ Idle(t)
  /\ Free(c)
  /\ sent < MaxMsgs
  /\ LET m == sent + 1 IN
       /\ sent' = m
       /\ chanOf' = [chanOf EXCEPT ![m] = c]
       /\ CASE closed[c] ->
                 /\ returned' = [returned EXCEPT ![m] = TRUE]
                 /\ UNCHANGED <<st, on, hold, buf, got>>
            [] ReceiversOn(c) # {} ->
                 \E u \in ReceiversOn(c) :
                   /\ got' = [got EXCEPT ![m] = 1]
                   /\ st' = [st EXCEPT ![u] = "ready"]
                   /\ on' = [on EXCEPT ![u] = {}]
                   /\ UNCHANGED <<hold, buf, returned>>
            [] Len(buf[c]) < Capacity(c) ->
                 /\ buf' = [buf EXCEPT ![c] = Append(@, m)]
                 /\ UNCHANGED <<st, on, hold, got, returned>>
            [] OTHER ->
                 /\ st' = [st EXCEPT ![t] = "send"]
                 /\ on' = [on EXCEPT ![t] = {c}]
                 /\ hold' = [hold EXCEPT ![t] = m]
                 /\ UNCHANGED <<buf, got, returned>>
  /\ UNCHANGED <<pick, ended, closed, lock, closedAt, deadlock>>

\* `rx.recv()`: Some(the oldest message), else None when the channel is
\* closed and drained, else park until a message comes or it closes.
Recv(t, c) ==
  /\ Idle(t)
  /\ Free(c)
  /\ CASE HasMsg(c) ->
            /\ TakeMsg(t, c)
            /\ UNCHANGED <<ended>>
       [] closed[c] ->
            /\ ended' = [ended EXCEPT ![t] = @ \cup {c}]
            /\ UNCHANGED <<st, on, hold, buf, got>>
       [] OTHER ->
            /\ st' = [st EXCEPT ![t] = "recv"]
            /\ on' = [on EXCEPT ![t] = {c}]
            /\ UNCHANGED <<hold, buf, got, ended>>
  /\ UNCHANGED <<pick, closed, lock, sent, chanOf, closedAt, returned,
                 deadlock>>

\* `tx.close()`. Closing a closed channel does nothing. Parked senders
\* wake, and their sends return Err(v). Buffered messages stay for
\* receivers (unless not CloseKeepsBuffer). A parked receive wakes with
\* None; a parked select wakes with "all closed" once every arm is drained.
Close(t, c) ==
  /\ Idle(t)
  /\ Free(c)
  /\ ~closed[c]
  /\ LET senders == SendersOn(c)
         buf2 == [buf EXCEPT ![c] = IF CloseKeepsBuffer THEN @ ELSE <<>>]
         closed2 == [closed EXCEPT ![c] = TRUE]
         \* c has no parked sender once they wake.
         Drained2(a) == closed2[a] /\ Len(buf2[a]) = 0
                          /\ (a = c \/ SendersOn(a) = {})
         wake == senders \cup
                   {u \in ReceiversOn(c) : \A a \in on[u] : Drained2(a)}
     IN /\ closed' = closed2
        /\ buf' = buf2
        /\ closedAt' = [closedAt EXCEPT ![c] = sent]
        /\ returned' = [m \in Msgs |->
             returned[m] \/ \E u \in senders : hold[u] = m]
        /\ ended' = [u \in Tasks |->
             IF u \in wake /\ u \notin senders THEN ended[u] \cup on[u]
             ELSE ended[u]]
        /\ st' = [u \in Tasks |-> IF u \in wake THEN "ready" ELSE st[u]]
        /\ on' = [u \in Tasks |-> IF u \in wake THEN {} ELSE on[u]]
        /\ hold' = [u \in Tasks |-> IF u \in senders THEN None ELSE hold[u]]
  /\ UNCHANGED <<pick, lock, sent, chanOf, got, deadlock>>

\* `select` over arms A, first step: scan the arms. With LockArms it takes
\* all their locks first. It picks an arm with a message; if none has one
\* it returns "all closed" when every arm is drained, else parks on all
\* the arms until a message comes or the last arm drains.
Scan(t, A) ==
  /\ Idle(t)
  /\ Cardinality(A) >= 2
  /\ \A c \in A : Free(c)
  /\ IF \E c \in A : HasMsg(c)
       THEN \E c \in A :
              /\ HasMsg(c)
              /\ pick' = [pick EXCEPT ![t] = c]
              /\ lock' = [a \in Chans |->
                   IF LockArms /\ a \in A THEN t ELSE lock[a]]
              /\ UNCHANGED <<st, on, ended>>
       ELSE /\ IF \A c \in A : Drained(c)
                 THEN /\ ended' = [ended EXCEPT ![t] = @ \cup A]
                      /\ UNCHANGED <<st, on>>
                 ELSE /\ st' = [st EXCEPT ![t] = "select"]
                      /\ on' = [on EXCEPT ![t] = A]
                      /\ UNCHANGED ended
            /\ UNCHANGED <<pick, lock>>
  /\ UNCHANGED <<hold, buf, closed, sent, chanOf, closedAt, got, returned,
                 deadlock>>

\* `select`, second step: take the picked arm's message and release the
\* locks. Only without LockArms can the arm have lost its message since
\* the scan (SelectChoosesReady is then violated); the take gets nothing.
SelectTake(t) ==
  /\ Running(t)
  /\ pick[t] # None
  /\ LET c == pick[t] IN
       IF HasMsg(c)
         THEN TakeMsg(t, c)
         ELSE UNCHANGED <<st, on, hold, buf, got>>
  /\ pick' = [pick EXCEPT ![t] = None]
  /\ lock' = [a \in Chans |-> IF lock[a] = t THEN None ELSE lock[a]]
  /\ UNCHANGED <<ended, closed, sent, chanOf, closedAt, returned, deadlock>>

\* A spawned task returns.
Return(t) ==
  /\ Idle(t)
  /\ t # Main
  /\ st' = [st EXCEPT ![t] = "done"]
  /\ UNCHANGED <<on, hold, pick, ended, buf, closed, lock, sent, chanOf,
                 closedAt, got, returned, deadlock>>

\* `main`'s scope block ends, then `main` returns: at once if every other
\* task has ended, else it parks until they have.
OthersDone == \A u \in Tasks \ {Main} : st[u] = "done"
EndMain ==
  /\ Idle(Main)
  /\ st' = [st EXCEPT ![Main] = IF OthersDone THEN "done" ELSE "closing"]
  /\ UNCHANGED <<on, hold, pick, ended, buf, closed, lock, sent, chanOf,
                 closedAt, got, returned, deadlock>>
WakeMain ==
  /\ st[Main] = "closing"
  /\ OthersDone
  /\ st' = [st EXCEPT ![Main] = "done"]
  /\ UNCHANGED <<on, hold, pick, ended, buf, closed, lock, sent, chanOf,
                 closedAt, got, returned, deadlock>>

\* Every task is parked or has ended, and `main` has not: no task can
\* ever move again. The runtime sees this when it parks the last task, and
\* stops the program with a deadlock panic.
AllParked ==
  /\ st[Main] # "done"
  /\ \A t \in Tasks : st[t] \in Parked \cup {"done"}
  /\ ~(st[Main] = "closing" /\ OthersDone)
DetectDeadlock ==
  /\ ~deadlock
  /\ AllParked
  /\ deadlock' = TRUE
  /\ UNCHANGED <<st, on, hold, pick, ended, buf, closed, lock, sent, chanOf,
                 closedAt, got, returned>>

\* The program has ended: `main` returned, or the runtime reported a
\* deadlock. Nothing more happens.
Ended == (st[Main] = "done" \/ deadlock) /\ UNCHANGED vars

TaskStep(t) ==
  \/ \E c \in Chans : Send(t, c) \/ Recv(t, c) \/ Close(t, c)
  \/ \E A \in SUBSET Chans : Scan(t, A)
  \/ SelectTake(t)
  \/ Return(t)
  \/ (t = Main /\ EndMain)

Next ==
  \/ \E t \in Tasks : Schedule(t) \/ TaskStep(t)
  \/ WakeMain
  \/ DetectDeadlock
  \/ Ended

\* Each task keeps taking steps while it can, `main` wakes once the other
\* tasks have ended, the runtime reports a deadlock once there is one, and
\* a ready task gets a worker (strong fairness, as in ScopedTasks).
Fairness ==
  /\ \A t \in Tasks : WF_vars(TaskStep(t)) /\ SF_vars(Schedule(t))
  /\ WF_vars(WakeMain)
  /\ WF_vars(DetectDeadlock)

Spec == Init /\ [][Next]_vars /\ Fairness

\* --- properties --------------------------------------------------------------

Sent == 1 .. sent
\* How many places hold m: a buffer slot, a parked sender, a receiver
\* that got it, or the sender it was returned to.
Copies(m) ==
  Cardinality({<<c, i>> \in Chans \X (1 .. MaxMsgs) :
                 i <= Len(buf[c]) /\ buf[c][i] = m})
    + Cardinality({t \in Tasks : st[t] = "send" /\ hold[t] = m})
    + got[m]
    + (IF returned[m] THEN 1 ELSE 0)

\* No message is lost: every message sent is still on its way (buffered,
\* or held by its parked sender), was received, or was returned to its
\* sender by a failed send.
NoLostMessage == \A m \in Sent : Copies(m) >= 1

\* No message is duplicated: it is in one place only, so at most one
\* receive gets it, and only once.
NoDuplicateMessage == \A m \in Sent : Copies(m) <= 1

\* A channel is first in, first out: its buffer is oldest first, and its
\* parked senders hold newer messages than the buffer. A receive takes the
\* head, so each sender's messages arrive in the order it sent them.
FifoOrder ==
  \A c \in Chans :
    /\ \A i, j \in 1 .. Len(buf[c]) : i < j => buf[c][i] < buf[c][j]
    /\ \A i \in 1 .. Len(buf[c]) : \A u \in SendersOn(c) : buf[c][i] < hold[u]

\* A closed channel takes no message: every send that started after the
\* close returned its message, and no sender stays parked on it.
ClosedRefusesSends ==
  \A c \in Chans : closed[c] =>
    /\ SendersOn(c) = {}
    /\ \A m \in Sent : chanOf[m] = c /\ m > closedAt[c] => returned[m]

\* A receive returns None, and a select "all closed", only for a channel
\* that is closed and drained: the messages buffered before the close are
\* received first.
EndOnlyWhenDrained == \A t \in Tasks : \A c \in ended[t] : Drained(c)

\* A select takes only from a ready arm: the arm it picked still has a
\* message when it takes.
SelectChoosesReady == \A t \in Tasks : pick[t] # None => HasMsg(pick[t])

\* No lost wake-up: a parked task waits for something that has not
\* happened yet. A receive's channel has no message and is open; a send's
\* channel is open and full with no receiver parked; a select has no arm
\* with a message and some arm not drained. (The end of `main`'s scope
\* wakes in a step of its own, as in ScopedTasks.)
ParkedOnlyWhileBlocked ==
  \A t \in Tasks :
    /\ st[t] = "recv" => \A c \in on[t] : ~HasMsg(c) /\ ~closed[c]
    /\ st[t] = "send" =>
         \A c \in on[t] : ~closed[c] /\ Len(buf[c]) = Capacity(c)
                          /\ ReceiversOn(c) = {}
    /\ st[t] = "select" =>
         /\ \A c \in on[t] : ~HasMsg(c)
         /\ \E c \in on[t] : ~closed[c]

\* At most one running task per worker.
WithinWorkers == Busy <= Workers

\* Every deadlock involves a channel: some task is parked on a send, a
\* receive or a select. Joins and scopes alone never deadlock
\* (ScopedTasks); channels are how a program makes a deadlock.
DeadlockNeedsAChannel ==
  deadlock => \E t \in Tasks : st[t] \in {"recv", "send", "select"}

\* A deadlock is reported only when it is real: every task is parked
\* (with ParkedOnlyWhileBlocked, none can move again).
DeadlockIsReal == deadlock => AllParked

\* No program hangs unseen: it ends, or the runtime reports its deadlock.
EndsOrReportsDeadlock == <>(st[Main] = "done" \/ deadlock)
=============================================================================
