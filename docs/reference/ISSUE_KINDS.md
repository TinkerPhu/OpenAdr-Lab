# Issue kinds, ranked

Order = priority: **top is highest**. Reorder the list to change priorities; nothing else needs
editing. `scripts/audit_debt_gate.py` reads this file (numbered lines starting with a backticked
kind) and sorts findings by it, then by Severity (S1 first).

Each open issue in `TECHNICAL_DEBTS.md` / `BACKLOG.md` carries: **Kind** (below),
**Severity** (S1 wrong output/data/wire or production failure; S2 untrustworthy instruments or
reliability; S3 slows work or misleads readers; S4 cosmetic/future-proofing), **Cost**
(Small / Medium / Large — under ~1 hour is fixed, never filed), and **Why open**
(`needs-decision` | `needs-evidence` | `too-big`, naming the first step).

1. `bug` — the system does the wrong thing
2. `wire-contract` — what we emit or accept on the wire does not declare or honour its meaning
3. `security-deps` — vulnerable dependency, open broker, credential handling
4. `test-infra` — flaky, slow or untrustworthy tests and test tooling
5. `architecture` — ring/port violations, asset-competence breaches
6. `duplication` — one concept implemented more than once
7. `ui-transparency` — state or capability with no visible UI surface
8. `performance` — solve time, request volume, render cost
9. `file-size` — files over the 500/200 production-line caps
10. `feature-gap` — capability the system does not have yet (the GB-* rows)
11. `style` — naming, unit suffixes, lint, dead code
12. `docs` — documentation that is missing, stale or wrong
