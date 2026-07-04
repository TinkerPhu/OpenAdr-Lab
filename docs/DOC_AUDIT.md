# Documentation Audit — 2026-07-03

> All recommendations executed on 2026-07-03. This file documents what was done and why.

---

## Proposed Hierarchy

```
docs/
├── REQUIREMENTS.md              ← authoritative domain model (tier 0)
├── BACKLOG.md                   ← open requirements gap tracker
├── BACKLOG_OpenADR_Cert.md      ← certification readiness tracker
├── SECURITY.md                  ← security posture doc
│
├── architecture/                ← "how it is built"
│   ├── VEN_ARCHITECTURE.md      ← primary VEN reference
│   ├── VTN_ARCHITECTURE.md      ← primary VTN reference
│   ├── INTERFACES.md            ← REST route table (VEN + BFF)
│   ├── ven_milp_planner.md      ← MILP planner design (current)
│   ├── ven_asset_interface_spec.md  ← Asset trait / Rust signatures
│   ├── asset_simulation.md      ← per-asset physics reference
│   ├── heater_tank_milp_planning_model.md  ← MILP math formulation
│   ├── module_dependency_graph_post_refactoring.md  ← Mermaid graph (needs refresh)
│   └── archive/                 ← superseded snapshots
│
├── guidelines/                  ← "how we work"
│   ├── TESTING.md
│   ├── REACT_GUIDELINES.md
│   ├── AI-SW-Development.md
│   ├── speckit-cheatsheet.md
│   └── superpowers_analysis.md
│
├── reference/                   ← "look things up"
│   ├── KEY_LEARNINGS.md
│   ├── TECHNICAL_DEBTS.md
│   ├── GLOSSARY.md
│   ├── FAQ.md
│   └── SESSION_START.md
│
├── plans/                       ← active / postponed work
│   ├── refactoring_backlog.md
│   ├── deviation-control-suggestions.md  (needs update)
│   ├── milp_storage_planning.md
│   ├── milp_storage_planning_impl.md
│   ├── milp_planner_config.md
│   ├── postponed/
│   └── archive/
│
├── use-cases/                   ← lab manuals
│   ├── SYSTEM-USE-CASES.md
│   ├── SYSTEM-USE-CASE-MANUAL.md
│   └── HEMS-USE-CASE-OBSERVATION-MANUAL.md  (needs update)
│
└── history/
    └── project_journal.md
```

---

## Active Docs — Per-File Assessment

### Tier 0 — Authoritative References

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `REQUIREMENTS.md` | fresh | ✓ none | large | **keep** — designated authoritative domain model |
| `SECURITY.md` | fresh | ✓ none | small | **keep** — accurate, appropriately scoped to lab |
| `BACKLOG.md` | aging | likely-outdated | large | **update** — item status needs a pass; some may already be implemented |
| `BACKLOG_OpenADR_Cert.md` | aging | likely-outdated | medium | **update** — dated 2026-03-22; ~4 months of VEN changes since audit |

### Architecture Docs

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `architecture/VEN_ARCHITECTURE.md` | aging | likely-outdated | large | **update** — §2.3 still describes the old greedy planner; MILP replaced it |
| `architecture/VTN_ARCHITECTURE.md` | fresh | ✓ none | medium | **keep** |
| `architecture/INTERFACES.md` | fresh | ✓ none | medium | **keep** |
| `architecture/ven_milp_planner.md` | fresh | ✓ none | medium | **keep** — authoritative MILP reference for current branch |
| `architecture/ven_asset_interface_spec.md` | fresh | ✓ none | large | **keep** |
| `architecture/asset_simulation.md` | fresh | ✓ none | large | **keep** |
| `architecture/heater_tank_milp_planning_model.md` | fresh | ✓ none | large | **keep** — math model is implemented and BDD-tested |
| `architecture/module_dependency_graph_post_refactoring.md` | aging | likely-outdated | small | **update** — graph may show resolved violations; new ones may have appeared |
| `architecture/packet_explanation.md` | fresh | ✓ none | tiny | **merge-into: VEN_ARCHITECTURE.md** — short design note, wrong granularity for a standalone file |
| `architecture/system_design.md` | dead | wrong-if-used | large | **DROP** — first line self-declares "ARCHIVED — Superseded by VTN_ARCHITECTURE.md"; still in active folder |
| `architecture/Domain_definitions.md` | dead | wrong-if-used | small | **DROP** — first line self-declares "ARCHIVED — Superseded by REQUIREMENTS.md" |
| `architecture/concept_vtn_ven_demand_response_simulation.md` | stale | wrong-if-used | large | **archive** — pre-implementation concept; reactor FSM and Python operator described no longer exist |
| `architecture/simulators_and_reactors.md` | dead | wrong-if-used | small | **DROP** — informal brain-dump about the removed reactor; no content not captured elsewhere |
| `architecture/testing_landscape.md` | stale | likely-outdated | large | **DROP** — raw terminal output of a specific test run; not a design document |

### Guidelines

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `guidelines/TESTING.md` | aging | likely-outdated | medium | **update** — CI/CD section claims GitHub Actions is configured; CLAUDE.md says no CI pipeline exists yet |
| `guidelines/REACT_GUIDELINES.md` | fresh | ✓ none | medium | **keep** |
| `guidelines/AI-SW-Development.md` | fresh | ✓ none | medium | **keep** |
| `guidelines/speckit-cheatsheet.md` | fresh | ✓ none | tiny | **keep** |
| `guidelines/superpowers_analysis.md` | fresh | ✓ none | tiny | **keep** — decision record for agentic framework evaluation |

### Reference

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `reference/KEY_LEARNINGS.md` | fresh | ✓ none | large | **keep** — living reference; high value for new sessions |
| `reference/TECHNICAL_DEBTS.md` | fresh | ✓ none | tiny | **keep** — only 3 open items; linked from CLAUDE.md |
| `reference/GLOSSARY.md` | fresh | ✓ none | medium | **keep** |
| `reference/FAQ.md` | fresh | ✓ none | large | **keep** |
| `reference/SESSION_START.md` | fresh | ✓ none | small | **keep** |
| `reference/spec_kit_startup_message.md` | stale | — | tiny | **DROP** — tool stdout captured by accident; zero reference value |

### Plans (active / postponed)

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `plans/refactoring_backlog.md` | fresh | ✓ none | small | **keep** — 2 open items (R-03, R-08) referenced by TECHNICAL_DEBTS.md |
| `plans/deviation-control-suggestions.md` | aging | likely-outdated | large | **update** — design is pre-spec; `absorber.rs` is partially built but doc still reads as a proposal; add implementation-status section |
| `plans/milp_storage_planning.md` | fresh | ✓ none | medium | **keep** — active design document |
| `plans/milp_storage_planning_impl.md` | fresh | ✓ none | medium | **keep** — active implementation plan |
| `plans/milp_planner_config.md` | fresh | ✓ none | medium | **keep** — parameter tuning reference |
| `plans/context-assessment-linger-time.md` | stale | likely-outdated | medium | **archive** — pre-implementation analysis for `min_state_linger_s`; feature not yet built; reactor references obsolete |
| `plans/3-tier-milp-slot-alignment.md` | aging | likely-outdated | large | **archive** — Part A is implemented and merged; Part B details now live in `ven_milp_planner.md` |
| `plans/postponed/ven-runtime-override-capability-plan.md` | stale | wrong-if-used | small | **archive** — references the removed reactor; needs full rewrite before revisiting |

### Use Cases

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `use-cases/SYSTEM-USE-CASES.md` | fresh | ✓ none | small | **keep** |
| `use-cases/SYSTEM-USE-CASE-MANUAL.md` | fresh | ✓ none | medium | **keep** |
| `use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` | aging | likely-outdated | medium | **update** — references "FSM state" Trace page from the removed reactor; UI page names have changed |

### History

| File | Staleness | Accuracy | Size | Recommendation |
|------|-----------|----------|------|----------------|
| `history/project_journal.md` | aging | — | large | **keep** — required by workflow; historical record is intentional; recent MILP entries may be missing |

---

## Archive Docs — Assessment

All files in `plans/archive/` and `architecture/archive/` are already correctly filed as historical record. No action needed — they exist for context, not as living references. Notable entries:

| File | Notes |
|------|-------|
| `plans/archive/ven_backend_architecture_refactoring*.md` | Origin of the hexagonal structure now in production |
| `plans/archive/milp_planner_transition.md` | Documents greedy → MILP decision |
| `plans/archive/phase6/7/phase_a*.md` | Completed refactoring phases |
| `plans/archive/spec_kit_001/002/004` | Completed speckit feature implementations |
| `plans/archive/BDD_*.md`, `COMPLETION_STATUS_017.md` | BDD wiring history |
| `architecture/archive/ven_backend_components.md` | Pre-refactoring component diagrams |

---

## Priority Action List

### P1 — Drop (self-archived or noise, zero value)

| File | Why |
|------|-----|
| `architecture/system_design.md` | Self-declares ARCHIVED in line 1; still in active folder |
| `architecture/Domain_definitions.md` | Self-declares ARCHIVED in line 1; still in active folder |
| `architecture/simulators_and_reactors.md` | Brain-dump about removed reactor; content captured elsewhere |
| `architecture/testing_landscape.md` | Raw terminal output paste, not a document |
| `reference/spec_kit_startup_message.md` | Accidental tool stdout capture |

### P2 — Move to archive/ (historically interesting but misleading as active)

| File | Why |
|------|-----|
| `architecture/concept_vtn_ven_demand_response_simulation.md` | Reactor FSM + Python operator described no longer exist |
| `plans/context-assessment-linger-time.md` | Pre-impl analysis for unbuilt feature with reactor refs |
| `plans/3-tier-milp-slot-alignment.md` | Part A done; Part B now documented in `ven_milp_planner.md` |
| `plans/postponed/ven-runtime-override-capability-plan.md` | Reactor-based design, needs full rewrite before use |

### P3 — Update (accurate skeleton, stale sections)

| File | What to fix |
|------|-------------|
| `architecture/VEN_ARCHITECTURE.md` | §2.3 still describes greedy planner — replace with redirect to `ven_milp_planner.md` |
| `architecture/module_dependency_graph_post_refactoring.md` | Re-run violation check against current source; update Mermaid graph |
| `guidelines/TESTING.md` | CI/CD section claims GitHub Actions is live — contradicts CLAUDE.md |
| `use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` | Remove FSM/reactor references; update UI page names |
| `BACKLOG.md` | Mark items resolved vs. open based on current code |
| `BACKLOG_OpenADR_Cert.md` | Re-audit fulfilment percentages against current VEN |
| `plans/deviation-control-suggestions.md` | Add implementation-status section for `absorber.rs` |

### P4 — Merge

| File | Into |
|------|------|
| `architecture/packet_explanation.md` | `architecture/VEN_ARCHITECTURE.md` (short design note, wrong granularity) |
