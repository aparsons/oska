# Compliance reports are self-assessments, and only findings lower the determination

## Decision

Oska publishes its reports against outside standards, such as SLSA, the NIST SSDF or ISO/IEC 5230, as **self-assessments**: the Oska project judges itself and cites a pinned commit or release for each claim. A report never calls itself an audit, and it says that no third party performed or certified it.

Each gap a report finds is rated High, Medium or Low by likelihood times impact. A High or Medium gap is a **finding**; a Low gap is an **observation**. Only findings lower the **determination**, the headline verdict of Conforms, Partially conforms or Does not conform. An observation does not lower it, even when it records a requirement the project does not meet. The determination is given beside the standard's own measure, such as an SLSA Build level or a Scorecard score.

Two standards have no pass criteria of their own and are handled differently. NIST CSF 2.0 is judged by its Implementation Tiers, one per Function, and its determination follows the lowest Tier. ISO/IEC 25010 gets observations only and no determination.

## Why

The project assesses itself, so audit wording would claim an independence the reports do not have, and readers of ISO/IEC 5230, ISO/IEC 18974 and the SSDF read "audit" as a third party's attestation.

Rating gaps by risk keeps the determination about what matters to a user of Oska. A Low-rated unmet requirement, such as a missing file a standard asks for, still appears in the report, with its recommendation, but it does not turn a sound project into a "Partially conforms".

## Considered options

- **Audit wording with a disclaimer.** Rejected: the headline would still read as an independent audit.
- **A finding is any unmet requirement; an observation is advice.** Rejected by the maintainer in favour of the risk rating. Its strength was that a reader could check every determination against the standard's clauses alone.
- **Every unmet requirement blocks Conforms, whatever its rating.** Rejected for the same reason: the determination would follow the count of clauses, not the risk.

## Consequences

A report that says Conforms can list unmet requirements among its observations. The report template and the FAQ must say so, so that a reader does not take Conforms to mean that every clause is met.
