# lock Agent Ergonomics Scorecard - Pass 1

## Summary

- Mode: full
- Surfaces inventoried: 6 focused surfaces
- Recommendations applied: 4 / 4
- Intent corpus: 4 canonical first-try intents, 0 silent failures after patch
- Version prepared: 0.5.0

## Scores

| Dimension | Before | After | Evidence |
|---|---:|---:|---|
| Self-documentation | 650 | 900 | `lock capabilities --json`, `lock robot-docs guide`, `lock --describe` |
| Output parseability | 780 | 920 | single-object JSON for triage and capabilities |
| Error pedagogy | 520 | 900 | `lock doctor --fix` names exact alternatives |
| Intent inference | 590 | 860 | top-level first-try commands no longer fall through to input parsing |
| Installability | 720 | 850 | release workflow formula generation fails on missing checksums |
| Regression resistance | 680 | 860 | Rust tests plus audit regression scripts |

## Residual Risk

The core lock creation and verification paths were intentionally left unchanged. Future passes should consider generalized typo recovery for common flag misspellings if that becomes a recurring support issue.
