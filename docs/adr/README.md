# Architecture decision records

Each file here is one decision, named `NNNN-<decision>.md`. Read the glossary first (`GLOSSARY.md`), then the records for the area you change (`docs/agents/domain.md`).

## Numbers have gaps

A number is reserved when a spec plans its record, and the file lands only when its pull request merges. Parallel branches can therefore merge out of order. A gap in the list does not mean that a record was lost or deleted.

As of 2026-11-28 these numbers are reserved and not yet merged:

| Numbers | Held by |
| --- | --- |
| 0031 to 0033 | Set, Deque and sorted collections (#798) |
| 0035 | Core library (#660) |
| 0036 | Let's Encrypt and ACME (#824) |
| 0037 to 0039 | Web services (#826) |

The next free number is 0041. When a spec reserves numbers, add its row here. Delete the row when its records merge.
