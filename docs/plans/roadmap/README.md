# Roadmap Implementation Plans — Index

> **Date:** 2026-07-06
> **Parent:** `docs/plans/strategic_roadmap.md` (topic clusters, user stories, priority queue)
> One plan file per phase. Each phase ends with a runnable demonstration, not just merged code.

| Phase | Plan | Goal | Exit demonstration |
|-------|------|------|--------------------|
| 5 | [phase-5-forecast-and-baseline.md](phase-5-forecast-and-baseline.md) | Heuristics from history, external feeds, baselines | Heuristic forecast beats last-known on a held-out week |
| 6 | [phase-6-fidelity-and-cert.md](phase-6-fidelity-and-cert.md) | Planner fidelity, transport modernisation, hygiene | Cert-readiness re-audit; slot costs match real bills |

Phases 0–4 (quick wins, data foundation, fleet enablement, control-method lab,
comfort & personas) are fully implemented; their plan documents were removed.
Current-state documentation lives in `wiki/components/` (`history-store.md`,
`fleet-tooling.md`, `experiment-harness.md`, `heuristics-pipeline.md`,
`notifications.md`), with resolved backlog items dropped from `docs/BACKLOG.md`
and open remainders (the S-1…S-6 experiment run, persona re-run) tracked in
`docs/plans/strategic_roadmap.md` §3.1.

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

Phases 0–4 shipped in order (0 → 1 → 2 → 3 → 4), with Phase 1's history store running
in "collect" mode from early on so Phase 5's heuristics (BL-14) had multi-week real
history once they started. Phase 5 builds on that accumulated history; Phase 6 is a
grab-bag whose WPs are independent and can be worked in any order.
