# How these reports are written

Every report in the [Compliance section](index.md) follows this template. The Oska project writes each one as a **self-assessment**: the project judges itself against one outside standard and cites a pinned commit or release for each claim. Be clear that no third party performed or certified it, and do not rate it as if one had.

## The six sections

Each report has these sections, in this order.

### Executive summary

The determination, the standard's own measure, and the findings in a few lines. It names the release and the commit the report judges, so that a reader can tell when the report is out of date.

### Scope

What the report covers and what it leaves out: the release and commit, the parts of the project judged, and the version of the standard.

### Determination

The headline verdict, Conforms, Partially conforms or Does not conform, given beside the standard's own measure, such as an SLSA Build level, a Scorecard score or an OSPS level. The rule is below.

### Findings with recommendations

Each **finding** is a gap rated High or Medium, with a recommendation and a link to the evidence at a pinned commit or release.

### Observations with recommendations

Each **observation** is a gap rated Low, or advice where nothing is unmet, with a recommendation and a link to the evidence.

### Conclusions

What the project plans to do about the findings, and what a reader should check for themselves.

## The rating scale

Every gap is rated by likelihood times impact.

| Rating | Meaning |
|---|---|
| High | Likely to happen and costly when it does. |
| Medium | Either likely and minor, or unlikely and costly. |
| Low | Unlikely and minor. |

A High or Medium gap is a finding. A Low gap is an observation.

## The determination rule

Only findings lower the determination. The rule, from [ADR 0017](../adr/0017-compliance-reports-are-self-assessments.md):

- **Conforms**: there is no finding.
- **Partially conforms**: there is at least one finding, and the standard is met in part.
- **Does not conform**: the findings show that the standard is not met.

An observation never lowers the determination, even when it records a requirement the project does not meet. So Conforms can sit beside a Low-rated unmet requirement, such as a missing file the standard asks for: the report still lists it, with its recommendation. Do not read Conforms as "every clause is met".

Two standards are handled differently. NIST CSF 2.0 is judged by its Implementation Tiers, one per Function, and its determination follows the lowest Tier. ISO/IEC 25010 defines no pass criteria, so its report has observations only and no determination.

## Terms and authorship

The reports use the project's glossary terms: self-assessment, finding, observation and determination. The author of every report is the Oska project.
