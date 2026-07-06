# Roadmap Implementation Plans — Index

> **Date:** 2026-07-06
> **Parent:** `docs/plans/strategic_roadmap.md` (topic clusters, user stories, priority queue)
> One plan file per phase. Each phase ends with a runnable demonstration, not just merged code.

| Phase | Plan | Goal | Exit demonstration |
|-------|------|------|--------------------|
| 0 | [phase-0-quick-wins.md](phase-0-quick-wins.md) | Small, high-value fixes | BL-02/BL-12 merged, uniform VEN naming, zero warnings |
| 1 | [phase-1-data-foundation.md](phase-1-data-foundation.md) | Persistent history in VEN (SQLite) + VTN (Postgres recorder) | "Show me yesterday" works in VEN UI after container restart |
| 2 | [phase-2-fleet-enablement.md](phase-2-fleet-enablement.md) | N independent VENs, one command, stable VTN | `./fleet.sh up 10` → 10 healthy VENs on Pi4 |
| 3 | [phase-3-control-method-lab.md](phase-3-control-method-lab.md) | All VTN control knobs honoured + experiment harness | Experiment report: tariff vs. limit vs. event day |
| 4 | [phase-4-comfort-and-personas.md](phase-4-comfort-and-personas.md) | Resident intent, comfort curves, notifications | Same experiments with 3 personas → measurably different fleet response |
| 5 | [phase-5-forecast-and-baseline.md](phase-5-forecast-and-baseline.md) | Heuristics from history, external feeds, baselines | Heuristic forecast beats last-known on a held-out week |
| 6 | [phase-6-fidelity-and-cert.md](phase-6-fidelity-and-cert.md) | Planner fidelity, transport modernisation, hygiene | Cert-readiness re-audit; slot costs match real bills |

## Conventions common to all phases (do not repeat per plan)

- **Workflow per work package (WP):** propose via `/openspec-propose` → branch
  `NNN-<slug>` (openspec ID) → test-first (write failing test, then implement) →
  all four suites green → PR to `main`, rebase + fast-forward merge.
- **Gates before merge:** `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo audit`, `scripts/audit_file_sizes.py` (tasks/ ≤ 200, VEN/src/ ≤ 500
  production lines), E2E green on Pi4.
- **Test pyramid (VEN Rust):** Domain → Use-case → Adapter-contract → Integration;
  mocks in `VEN/src/services/test_support/`; naming `test_<function>_<scenario>`.
- **Architecture:** hexagonal ring rules from `.claude/CLAUDE.md` (ven-architecture);
  new external integrations get a **port trait + mock**, never a concrete import from
  an inner ring. Every time-dependent module takes an **injectable clock**.
- **Naming:** physical quantities carry unit suffixes (`power_kw`, `soc_pct`,
  `tariff_eur_per_kwh`).
- **Builds:** `wsl cargo …` locally; full HiGHS runs via Pi4 docker.
- **Bookkeeping at phase end:** update `docs/BACKLOG.md` (mark items resolved),
  `docs/history/project_journal.md` entry, `docs/reference/KEY_LEARNINGS.md` if any,
  `/wiki-sync`, and re-check `docs/reference/TECHNICAL_DEBTS.md` for items touched.
- **Effort tags:** S ≤ ½ day · M ≈ 1–2 days · L ≈ 3–5 days · XL > 1 week.

## Cross-phase dependencies

```
P0 ──► P1 ──► P2 ──► P3 ──► P4
        │                    │
        └───(history data accumulates ≥ 4 weeks)───► P5 ──► P6
```

Phase 1's history store must run in "collect" mode as early as possible — Phase 5's
heuristics (BL-14) need multi-week real history. Phases 3 and 4 can overlap with that
accumulation window. Phase 6 is a grab-bag; its WPs are independent and can be
interleaved anywhere after Phase 3 if priorities shift.
