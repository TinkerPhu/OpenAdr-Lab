## Why

`DOCUMENTATION.md` is the primary entry point for understanding the system, but a systematic gap analysis (Requirement-Gaps.md) identified several missing sections that leave readers unable to interpret power values, understand core algorithmic decisions, or trace features back to requirements. A TODO item in the document header also requests a dedicated MILP formulation chapter that has never been written.

## What Changes

- **Add §0 Glossary** — domain term definitions (VEN, VTN, BFF, DSO, etc.) and the sign convention (positive = import from grid, negative = export) sourced from `docs/REQUIREMENTS.md §2`.
- **Add §2.11 Time-Series Architecture** — document `TimeSeries<T>`, the `Interpolation` enum (`Step` / `Linear`), tariff boundary alignment, per-interval capacity flattening, and slot classification (`FIRM` / `FLEXIBLE`), sourced from `docs/architecture/VEN_ARCHITECTURE.md §5`.
- **Clarify packet role** — add a paragraph in §2 or §4 explicitly stating that energy packets are intent/reporting metadata, not MILP scheduling variables; MILP decision variables drive the schedule.
- **Add FR code cross-references** — inline `(FR-OA-01)` / `(FR-SIM-03)` anchors in §2 feature sections to make compliance traceability possible, sourced from `docs/REQUIREMENTS.md §4`.
- **Expand §5 config reference** — add `min_run_slots` and `min_off_slots` to the heater profile YAML reference table and the heater MILP description in §2.4.
- **Add reference links** — link to the canonical architecture docs at the end of each major §2 and §4 section so readers can follow-up without searching.
- **Add §2.12 MILP Formulation** — deliver the chapter requested by the TODO on line 34: variable definitions, objective function (Phase 1 cost minimisation, Phase 2 friction), constraint families, and the independence-of-objectives constraint (`c_star`).

## Capabilities

### New Capabilities

- `documentation-reference-coverage`: Glossary §0, sign convention, FR code references, packet role clarification, time-series alignment §2.11, `min_run_slots`/`min_off_slots` in §5, and reference links at the end of each section.
- `documentation-milp-formulation`: Dedicated §2.12 with the complete MILP formulation — decision variables, objective phases, constraint families, and the two-phase independence mechanism.

### Modified Capabilities

<!-- No spec-level behavior changes — this is a documentation-only change. -->

## Impact

- **File changed**: `DOCUMENTATION.md` only — no code, API, or runtime behavior changes.
- **Services affected**: None.
- **openleadr-rs change required**: No.
- **Non-goals**: Writing new architecture docs (the reference docs already exist); changing any code to match new documentation; adding documentation for not-yet-implemented features (e.g., full 17-signal OpenADR taxonomy, VTN internal architecture).
