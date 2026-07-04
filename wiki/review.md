# Review Queue

Human-in-the-loop items found during sync/ingest/lint: contradictions, uncertain claims,
coverage gaps. Claude appends items instead of guessing; the human resolves or delegates.

Format: `- [ ] YYYY-MM-DD — <description> (found during <workflow>; pages: page-slug, other-slug)`

- [x] 2026-07-04 — `.claude/CLAUDE.md` §ven-architecture cites `docs/plans/ven_backend_architecture_refactoring.md` as its reference, but that file no longer exists in the working tree. Update or remove the reference. (found during seed sync; pages: ven-hexagonal-architecture) — **RESOLVED 2026-07-04**: reference now points to `docs/architecture/VEN_ARCHITECTURE.md` + module dependency graph.
- [x] 2026-07-04 — `docs/REQUIREMENTS.md` §2.3 still defines the Planner as using "a greedy algorithm"; superseded by the two-phase MILP. Glossary needs updating. (found during seed sync; pages: milp-over-greedy, milp-planner) — **RESOLVED 2026-07-04**: glossary entry now names the two-phase MILP solver (HiGHS) on the 3-tier grid.
- [x] 2026-07-04 — purpose.md asks for use cases *implied by the OpenADR 3 spec* to be systematically derived and gap-checked against the code base. (found during seed sync; pages: system-use-cases, vision-and-roadmap) — **RESOLVED 2026-07-04**: first pass written as [[openadr-spec-use-cases]] from User Guide §5–§7 + cert backlog; deepen later via `/wiki-ingest docs/openadr_3_1_specs/`.
- [ ] 2026-07-04 — `docs/REQUIREMENTS.md` §2.3 "Energy Packet" glossary entry describes packet-based scheduling as current, but the code removed its last remnants (PacketSeed config, `packet_id` fields) on 2026-07-04 — device sessions carry the lifecycle now; only comment vocabulary and `PacketTransition` trace events remain. Decide: rewrite the glossary entry as historical, or rename residual code vocabulary. (found during review-item fixes; pages: hems-planning)
