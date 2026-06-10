# lock Agent Ergonomics Playbook

## Applied Focus

This pass targeted the commands an agent naturally tries first when it needs to discover a CLI contract:

- `lock --robot-triage`
- `lock capabilities --json`
- `lock robot-docs guide`
- `lock doctor --fix`

The lock artifact path, verify behavior, witness append semantics, and refusal envelopes were intentionally left unchanged.

## Residual Work

Generalized typo recovery for arbitrary flags and command names is deferred to a follow-up bead. That needs a shared pattern across the spine tools rather than a one-off handler here.
