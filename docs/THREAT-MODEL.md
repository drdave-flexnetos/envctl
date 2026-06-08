# Threat Model

**Status:** DEFERRED — Part of secrets Phase 7 (envctl MERGE). The complete threat model is developed in `docs/secrets/` and will be merged into this file when the secrets stack integrates into the main envctl workspace.

## Deferred By: secrets Phase 7

The full threat model is authored as part of the secrets stack's architecture and will be consolidated here during Phase 7 merge. See:
- `docs/secrets/ARCHITECTURE.md` — secrets stack threat model (HS-S*, FS-S* items)
- `docs/secrets/audits/AUDIT-server-mode.md` — server-mode audit with cross-referenced threat items
- `docs/ops/02-envctl-component.md` — component-level threat references (THREAT-MODEL §72, §8, §75)

## Current References (resolved by docs/secrets/)

| Reference | Resolved In |
|-----------|-------------|
| THREAT-MODEL §72 (PARTUUID downgrade) | `docs/secrets/ARCHITECTURE.md` FS-S9 |
| THREAT-MODEL §8 (mlockall hardening) | `docs/secrets/ARCHITECTURE.md` FS-S4 |
| THREAT-MODEL §75 (keyslot on volatile fs) | `docs/secrets/ARCHITECTURE.md` FS-S10 |

The full model is the source of truth for all FS-S* and HS-S* security items in the secrets stack.
