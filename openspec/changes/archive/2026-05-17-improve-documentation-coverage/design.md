## Context

`DOCUMENTATION.md` is the sole operational guide for the system. The reference architecture docs (`docs/architecture/`) hold the authoritative design detail; the doc comparison in `Requirement-Gaps.md` identified seven concrete gaps. This design defines where each addition lands, what source to draw from, and how to keep the document readable without duplicating content.

No code changes. No service changes. One file: `DOCUMENTATION.md`.

## Goals / Non-Goals

**Goals:**
- Add §0 Glossary (domain terms + sign convention) so any reader can interpret power/cost values
- Add §2.11 Time-Series Architecture so contributors understand how tariff and capacity data is queried across slot boundaries
- Add §2.12 MILP Formulation (the requested TODO chapter) with variables, objectives, and constraints
- Clarify the packet role (intent/reporting metadata, not scheduling) in the existing feature narrative
- Inline FR code anchors in §2 feature sections for compliance traceability
- Add `min_run_slots` / `min_off_slots` to the §5 heater config table and §2.4 description
- Add reference links at the end of each major §2 and §4 section

**Non-Goals:**
- Documenting not-yet-implemented features (full 17-signal taxonomy, VTN internal architecture, VEN provisioning sequence)
- Changing any code, API, or runtime behavior
- Replacing the architecture docs — DOCUMENTATION.md remains the operational guide; architecture docs remain the design source of truth

## Decisions

### D-01 — §0 Glossary placement

Placed before §1 (Purpose), between the table of contents and the first section. This ensures readers encounter the vocabulary before any feature narrative. The sign convention diagram from `Domain_definitions.md` is reproduced inline (ASCII art) because it communicates direction immediately without requiring a link click.

**Alternative considered:** Appendix at the end. Rejected — readers would need to page back while reading §2.

### D-02 — §2.11 Time-Series Architecture: summary not full spec

`VEN_ARCHITECTURE.md §5` contains the authoritative implementation detail. §2.11 will cover the conceptual model (`TimeSeries<T>`, `Interpolation`, slot classification) and the alignment rules, then link to the architecture doc for the full specification.

**Rationale:** Duplicating the full spec creates a maintenance burden; a clear conceptual summary plus a link is more durable.

### D-03 — §2.12 MILP Formulation: notation-first approach

The chapter defines all symbols in a table before the equations so the section is self-contained. Phase 1 and Phase 2 objectives are presented separately with explicit statement of the independence-of-objectives constraint (`c_star` as a hard constraint in Phase 2).

**Source:** `docs/architecture/VEN_ARCHITECTURE.md` (two-phase MIP), `docs/architecture/heater_tank_milp_planning_model.md` (heater relay schema, switching penalty).

### D-04 — Packet role clarification: inline paragraph, not new section

A new subsection for packets would inflate the structure. A single paragraph added to the existing §2 narrative (after the feature overview table) is sufficient to correct the misconception. The paragraph explicitly states: "Energy packets track intent and feed reporting; the MILP decision variables are the scheduling mechanism."

### D-05 — FR codes: inline anchors only

Full requirement tables are already in `docs/REQUIREMENTS.md`. In DOCUMENTATION.md, FR codes appear as parenthetical inline anchors only — e.g., `(FR-SIM-03)` — to avoid duplication while enabling traceability.

### D-06 — Reference links: footer per section

Each major §2 and §4 section ends with a `> **Reference:** [Architecture doc name](path)` blockquote. This is visually distinct and easy to skip for readers who don't need depth.

## Risks / Trade-offs

- **MILP notation accuracy** → Mitigation: derive all symbols directly from the source Rust code (`milp_planner/solver_phase1.rs`, `solver_phase2.rs`) and cross-check against `VEN_ARCHITECTURE.md`. Flag any discrepancy as a comment.
- **Glossary staleness** → Mitigation: glossary terms are stable domain terms (VEN, VTN, sign convention); they are not tied to implementation details that change frequently.
- **min_run_slots / min_off_slots may not be in profile YAML yet** → Mitigation: check `VEN/src/profile.rs` and `VEN/profiles/` before adding to §5. If not yet exposed as YAML fields, document as "planned config parameter" with a note.
