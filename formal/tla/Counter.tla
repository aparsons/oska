------------------------------ MODULE Counter ------------------------------
(* A trivial model, so the formal step has a model to check before the     *)
(* parallel-tasks design (ADR 0019) is modelled here. A counter steps      *)
(* through 0, 1 and 2 and wraps around; TypeOK holds in every state.       *)
EXTENDS Naturals

VARIABLE n

Init == n = 0

Next == n' = (n + 1) % 3

Spec == Init /\ [][Next]_n

TypeOK == n \in 0..2
=============================================================================
