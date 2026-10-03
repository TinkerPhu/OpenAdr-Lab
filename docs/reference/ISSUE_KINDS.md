# Issue kinds, weighted

The leading number is the kind's **weight**: higher = more important. Equal weights tie and
fall back to Severity. Change a number to change priorities; nothing else needs editing.
`scripts/audit_debt_gate.py` reads the `N. \`kind\`` lines and sorts findings by weight
descending, then by Severity (S1 first).

Each open issue in `TECHNICAL_DEBTS.md` / `BACKLOG.md` carries: **Kind** (below),
**Severity** (S1 wrong output/data/wire or production failure; S2 untrustworthy instruments or
reliability; S3 slows work or misleads readers; S4 cosmetic/future-proofing), **Cost**
(Small / Medium / Large — under ~1 hour is fixed, never filed), and **Why open**
(`needs-decision` | `needs-evidence` | `too-big`, naming the first step).

10. `bug` — the system does the wrong thing
8. `architecture` — ring/port violations, asset-competence breaches
8. `duplication` — one concept implemented more than once
7. `wire-contract` — what we emit or accept on the wire does not declare or honour its meaning
7. `ui-transparency` — state or capability with no visible UI surface
6. `feature-gap` — capability the system does not have yet (the GB-* rows)
5. `performance` — solve time, request volume, render cost
5. `test-infra` — flaky, slow or untrustworthy tests and test tooling
5. `docs` — documentation that is missing, stale or wrong
4. `style` — naming, unit suffixes, lint, dead code
3. `security-deps` — vulnerable dependency, open broker, credential handling
2. `file-size` — files over the 500/200 production-line caps
