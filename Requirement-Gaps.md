# Requirement Gaps — DOCUMENTATION.md vs. Reference Docs

Comparison of `DOCUMENTATION.md` against all files in `docs/architecture/` and `docs/REQUIREMENTS.md`.  
Date: 2026-05-17 (updated after documentation improvement pass)

---

## How to read this file

Each section lists only open **GAP** items — topics present in the reference doc but absent or too thin in DOCUMENTATION.md, or **CONFLICT** items where the two sources disagree.

A **Priority Gaps** section at the end ranks the most important missing topics.

---

## `docs/REQUIREMENTS.md`

- Domain entity enumerations absent — `AssetType`, `PowerAdjustability`, `PlanTrigger`, `UserRequestMode`, `CompletionPolicy`, `StaleRatePolicy` are defined in REQUIREMENTS §3.2.1 but not in DOCUMENTATION.md
- Grid as virtual site boundary power balance — REQUIREMENTS §3.3 defines the formal Kirchhoff equation with all sign-convention implications; DOCUMENTATION.md §0 shows the sign convention diagram but does not reproduce the formal balance equation

---

## `docs/architecture/VEN_ARCHITECTURE.md`

- Report generation interval alignment (§5.2–5.3) — §2.11 addresses the target architecture, but §2.6 Report Obligations still lacks detail on obligation-based interval structure and boundary handling
- `CalcCache` struct and scoring strategy not mentioned

---

## `docs/architecture/VTN_ARCHITECTURE.md`

- Seeded data and program configuration not described — VTN ships with seed programs; DOCUMENTATION.md doesn't explain how to configure them
- Design Decisions D-01…D-06 (shared abstractions, `common/` module extraction plan) not mentioned

---

## `docs/architecture/ven_asset_interface_spec.md`

- Rust type signatures for the `Asset` trait (`current_state`, `capability`, `step`, `history`, `simulate_forward`, `simulate_free`, `capability_trajectory`) not in DOCUMENTATION.md
- `AssetCapability` struct formal definition missing — DOCUMENTATION.md discusses capability but not the struct fields
- `AssetState` enum variants and field definitions (`BatteryState.soc_pct`, `EvState.plugged`, `HeaterState.temperature_c`, etc.) not formally listed

---

## `docs/architecture/asset_simulation.md`

- Per-asset idle/default behaviour and external influences not systematically listed
- Precise physics equations with numerical defaults (battery efficiency formula, heater loss coefficient, EV minimum charge rate, PV STC reference irradiance) are scattered in DOCUMENTATION.md; this doc organises them in one place
- Per-asset capability computation rules (battery SoC-dependent bounds, EV plugged gate, heater thermostat override, PV point-range fixed capability) not in a reference table

---

## `docs/architecture/heater_tank_milp_planning_model.md`

- Two-layer heater control architecture (Layer A: physical heat-flow forecast; Layer B: MILP planning) not separated this explicitly in DOCUMENTATION.md — §2.12 covers Layer B in detail but Layer A (thermal forecast ODE inputs) is not described
- Time discretisation rationale — the hardware motivation for the 5-minute step and 288-slot horizon (heater mode stability) is not yet stated

---

## `docs/architecture/concept_vtn_ven_demand_response_simulation.md`

- 10 DR use-case scenarios (peak shaving, renewable integration, EV managed charging, GHG carbon-aware, grid emergency dispatch, frequency response, capacity reservation, flexible energy budget, reactive power/volt-var, dynamic operating envelope) — not in DOCUMENTATION.md

---

## `docs/architecture/testing_landscape.md`

- Test counts not stated in DOCUMENTATION.md: 232+ BDD scenarios, 313+ Rust unit tests, 32 frontend test files
- Total test run time (~50+ minutes for full BDD suite) not mentioned
- Test coverage breakdown by feature area not quantified

---

## `docs/architecture/Domain_definitions.md` and `system_design.md` (both archived)

- Time & clock management (NTP, ISO-8601 duration parsing) — not mentioned in DOCUMENTATION.md
- Security model details (OAuth scope, TLS optional) — DOCUMENTATION.md has minimal security content

---

## Priority Gaps

Ranked by impact on a new reader's ability to understand, contribute to, or operate the system.

### Tier 2 — Important for Implementation Completeness

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 5 | **FR code full reference table** | REQUIREMENTS.md §4 | Inline FR anchors (e.g. `FR-OA-01`) were added to §2 feature sections, but the full text of each requirement is only in REQUIREMENTS.md. A cross-reference table in DOCUMENTATION.md would make compliance auditing self-contained. |
| 8 | **Asset trait Rust type signatures** | ven_asset_interface_spec.md | Contributors adding new asset types have no formal interface contract to implement against. |

### Tier 3 — Operational and Quality

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 11 | **Test counts and run-time** | testing_landscape.md | DOCUMENTATION.md §7 describes what is tested but not how much or how long. |
| 12 | **Thermal model layer separation** | heater_tank_milp_planning_model.md | Layer A (forecast ODE) vs Layer B (MILP planning variables) distinction aids contributors adding new thermal assets (AC, floor heating). |

---

## Remaining Recommended Actions

*(All recommended actions have been completed.)*
