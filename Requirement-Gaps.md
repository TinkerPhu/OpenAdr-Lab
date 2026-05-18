# Requirement Gaps — DOCUMENTATION.md vs. Reference Docs

Comparison of `DOCUMENTATION.md` against all files in `docs/architecture/` and `docs/REQUIREMENTS.md`.  
Date: 2026-05-17 (updated after documentation improvement pass)

---

## How to read this file

Each section covers one reference document. Entries are classified as:

- **GAP** — topic present in the reference doc, absent or too thin in DOCUMENTATION.md
- **CONFLICT** — reference doc and DOCUMENTATION.md say different things
- **COVERED** — already adequately covered in DOCUMENTATION.md (noted briefly for completeness)

A **Priority Gaps** section at the end ranks the most important missing topics.

---

## `docs/REQUIREMENTS.md`

**GAPS:**
- Domain entity enumerations absent — `AssetType`, `PowerAdjustability`, `PlanTrigger`, `UserRequestMode`, `CompletionPolicy`, `StaleRatePolicy` are defined in REQUIREMENTS §3.2.1 but not in DOCUMENTATION.md (the core domain actors and named entities like `EnergyPacket`, `FlexibilityEnvelope`, `UserRequest`, `AssetLedger` were added to §0 Glossary)
- Grid as virtual site boundary power balance — REQUIREMENTS §3.3 defines the formal Kirchhoff equation with all sign-convention implications; DOCUMENTATION.md §0 shows the sign convention diagram but does not reproduce the formal balance equation
- Functional requirements cross-reference — inline FR code anchors (e.g. `FR-OA-01`, `FR-SIM-03`) were added to §2 feature sections; a full reference table mapping each FR code to its full text still does not exist in DOCUMENTATION.md (readers must look up REQUIREMENTS.md §4 for the text)

**CONFLICTS:** None — DOCUMENTATION.md is consistent with REQUIREMENTS conventions.

**COVERED:** OpenADR 3.0 overview; tariff vs. rate distinction; VEN/VTN topology; domain glossary (§0); sign convention with diagram (§0); EnergyPacket, FlexibilityEnvelope, UserRequest, AssetLedger definitions (§0); FR code anchors in §2 sections.

---

## `docs/architecture/VEN_ARCHITECTURE.md`

**GAPS:**
- Design Decisions D-01…D-07 not referenced — architectural rationale (why snapshot-and-release, why two-phase MIP, why per-asset ports) is not traceable to decisions in DOCUMENTATION.md
- Report generation interval alignment (§5.2–5.3) — §2.11 addresses the target architecture (`TimeSeries<T>`, `bucket()` with payload-type-driven aggregators), but §2.6 Report Obligations itself still lacks detail on obligation-based interval structure and boundary handling
- `CalcCache` struct and scoring strategy not mentioned

**CONFLICTS:** None.

**COVERED:** Component overview; HEMS controller; asset abstraction; API contract; two-phase MIP structure; locking protocol; time-series alignment (§2.11 — `TimeSeries<T>`, `Interpolation` enum, tariff boundary alignment, capacity flattening, slot classification `FIRM`/`FLEXIBLE`).

---

## `docs/architecture/VTN_ARCHITECTURE.md`

**GAPS:**
- VTN internal architecture entirely absent from DOCUMENTATION.md — PostgreSQL requirements, openleadr-rs module breakdown, BFF dual-credential pattern (`any-business` + `ven-manager` roles)
- OpenADR message sequences not documented — VEN startup, event distribution, event update/cancellation, token lifecycle, report submission are all sequenced in VTN_ARCHITECTURE but absent from DOCUMENTATION.md
- VEN provisioning steps missing — four-step process (user → OAuth2 credential → VEN entity → program/role assignment) needed for multi-VEN setup
- Seeded data and program configuration not described — VTN ships with seed programs; DOCUMENTATION.md doesn't explain how to configure them
- Deployment topology details thin — Docker bridge `vtn_openadr-net`, internal DNS names, container-to-container port mapping not shown (DOCUMENTATION.md §4.2 shows the high-level diagram only)
- Design Decisions D-01…D-06 (shared abstractions, `common/` module extraction plan) not mentioned

**CONFLICTS:** None.

**COVERED:** High-level deployment topology diagram; OAuth 2.0 client-credentials flow.

---

## `docs/architecture/ven_asset_interface_spec.md`

**GAPS:**
- Rust type signatures for the `Asset` trait (`current_state`, `capability`, `step`, `history`, `simulate_forward`, `simulate_free`, `capability_trajectory`) not in DOCUMENTATION.md
- `AssetCapability` struct formal definition missing — DOCUMENTATION.md discusses capability but not the struct fields
- `AssetState` enum variants and field definitions (`BatteryState.soc_pct`, `EvState.plugged`, `HeaterState.temperature_c`, etc.) not formally listed
- `AssetHistoryBuffer` interface and ring-buffer semantics not formalized — DOCUMENTATION.md mentions the 3600-entry buffer but not the API
- Sign convention arithmetic patterns (clamp formulas, flexibility calculation per asset) not in DOCUMENTATION.md
- Grid as an `AssetState` variant not mentioned

**CONFLICTS:** None.

**COVERED:** Asset types; physics models; flexibility envelope computation.

---

## `docs/architecture/asset_simulation.md`

**GAPS:**
- Dispatcher → Asset call chain (4-step flow: `build_setpoints` → behaviour injection → override resolution → physics `step()`) not traced in DOCUMENTATION.md
- Per-asset idle/default behaviour and external influences not systematically listed
- Precise physics equations with numerical defaults (battery efficiency formula, heater loss coefficient, EV minimum charge rate, PV STC reference irradiance) are scattered in DOCUMENTATION.md; this doc organises them in one place
- Per-asset capability computation rules (battery SoC-dependent bounds, EV plugged gate, heater thermostat override, PV point-range fixed capability) not in a reference table

**CONFLICTS:** None.

**COVERED:** Asset physics models; injection override modes A–D.

---

## `docs/architecture/heater_tank_milp_planning_model.md`

**GAPS:**
- Two-layer heater control architecture (Layer A: physical heat-flow forecast; Layer B: MILP planning) not separated this explicitly in DOCUMENTATION.md — §2.12 covers the MILP (Layer B) in detail but Layer A (thermal forecast ODE inputs) is not described
- Time discretisation rationale — 5-minute step and 288-slot horizon are now documented in §2.12; the hardware motivation (heater mode stability) is not yet stated

**CONFLICTS:** None.

**COVERED:** Heater thermal ODE; multi-tier control (off/mid/full); Phase 2 friction for switching; relay schema (delta schema, two-relay, 20% penalty for 0↔6 transitions — §2.12); `min_run_slots` / `min_off_slots` in §5 config table (documented as planned parameters); `StaleRatePolicy` default `HEURISTIC_FORECAST` (§2.11).

---

## `docs/architecture/concept_vtn_ven_demand_response_simulation.md`

**GAPS:**
- Full OpenADR 3.0 signal taxonomy (17 signal types: `PRICE`, `EXPORT_PRICE`, `GHG`, `SIMPLE`, `LOAD_DISPATCH`, `DISPATCH_SETPOINT`, `CHARGE_STATE_SETPOINT`, `IMPORT_CAPACITY_LIMIT`, `EXPORT_CAPACITY_LIMIT`, `IMPORT_CAPACITY_SUBSCRIPTION`, `IMPORT_CAPACITY_RESERVATION`, curve types, OLS) — DOCUMENTATION.md covers the 7 currently parsed types
- 10 DR use-case scenarios (peak shaving, renewable integration, EV managed charging, GHG carbon-aware, grid emergency dispatch, frequency response, capacity reservation, flexible energy budget, reactive power/volt-var, dynamic operating envelope) — not in DOCUMENTATION.md
- VEN operator motivation and decision drivers — not in DOCUMENTATION.md
- Capacity reservation and VEN-initiated request model — not mentioned

**CONFLICTS:**
- Concept doc discusses "reactor" behaviour profiles; DOCUMENTATION.md correctly reflects that the reactor was removed. No live conflict.

**COVERED:** OpenADR 3.0 overview; VEN/VTN topology; signal parsing for implemented types.

---

## `docs/architecture/simulators_and_reactors.md`

**GAPS:**
- Device simulator state model (quantities to persist, dynamic limits, properties) — DOCUMENTATION.md describes assets but not the formal state model
- Dynamic limits vs. static limits distinction not explained

**NOTE:** The reactor design section of this doc is obsolete — the reactor was removed (spec kit 001). Only the device simulator state model sections remain relevant.

**COVERED:** Asset physics simulation.

---

## `docs/architecture/packet_explanation.md`

**COVERED:** Packet role clarification added — §2 now explicitly states that energy packets are intent-tracking and reporting metadata, not MILP scheduling variables. The MILP decision variables (`p_ev[t]`, `z_heat_mid[t]`, etc.) drive the schedule; packets contribute their `request_mode`, `deadline`, and `target_energy_kwh` as constraints/reward terms only. Packet lifecycle states serve the dispatcher and reporting layers independently of the solver.

---

## `docs/architecture/testing_landscape.md`

**GAPS:**
- Test counts not stated in DOCUMENTATION.md: 232+ BDD scenarios, 313+ Rust unit tests, 32 frontend test files
- Total test run time (~50+ minutes for full BDD suite) not mentioned
- Test coverage breakdown by feature area not quantified

**CONFLICTS:** None.

**COVERED:** Test structure (BDD, unit, invariant checks); how to run tests.

---

## `docs/architecture/Domain_definitions.md` and `system_design.md` (both archived)

**GAPS (still relevant despite archived status):**
- Time & clock management (NTP, ISO-8601 duration parsing) — not mentioned in DOCUMENTATION.md
- Security model details (OAuth scope, TLS optional) — DOCUMENTATION.md has minimal security content

**COVERED:** Energy flow direction diagram with sign conventions — ASCII diagram and sign convention added to §0 Glossary.

---

## Priority Gaps

Ranked by impact on a new reader's ability to understand, contribute to, or operate the system.

### Tier 1 — Critical for Correctness and Onboarding

*(All Tier 1 gaps resolved in the May 2026 documentation improvement pass.)*

### Tier 2 — Important for Implementation Completeness

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 5 | **FR code full reference table** | REQUIREMENTS.md §4 | Inline FR anchors (e.g. `FR-OA-01`) were added to §2 feature sections, but the full text of each requirement is only in REQUIREMENTS.md. A cross-reference table in DOCUMENTATION.md would make compliance auditing self-contained. |
| 6 | **VEN provisioning sequence** | VTN_ARCHITECTURE.md §5 | Anyone setting up a new VEN instance has no documented procedure. |
| 7 | **Full OpenADR 3.0 signal taxonomy** | concept doc | Implementation only handles 7 of 17+ signal types. The gap between spec and implementation is invisible without a full taxonomy. |
| 8 | **Asset trait Rust type signatures** | ven_asset_interface_spec.md | Contributors adding new asset types have no formal interface contract to implement against. |

### Tier 3 — Operational and Quality

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 10 | **Deployment topology detail** | VTN_ARCHITECTURE.md §6 | Docker bridge names, internal DNS, port mapping table needed for network troubleshooting. |
| 11 | **Test counts and run-time** | testing_landscape.md | DOCUMENTATION.md §7 describes what is tested but not how much or how long. |
| 12 | **Thermal model layer separation** | heater_tank_milp_planning_model.md | Layer A (forecast ODE) vs Layer B (MILP planning variables) distinction aids contributors adding new thermal assets (AC, floor heating). |
| 13 | **VTN internal architecture** | VTN_ARCHITECTURE.md | Entirely absent; matters when extending the VTN side or debugging event/report flows. |

---

## Remaining Recommended Actions

1. **Add a cross-reference table for FR codes** — a table mapping each `FR-OA-xx`, `FR-ASSET-xx`, `FR-SIM-xx` code to its one-line description, linked from the relevant §2 sections. The full text stays in REQUIREMENTS.md; the table makes DOCUMENTATION.md self-contained for compliance checks.
2. **Document the VEN provisioning sequence** in §6 Deployment — the four steps (user → OAuth2 credential → VEN entity → program/role assignment).
3. **Add §4.8 VTN Internal Architecture** — PostgreSQL schema overview, openleadr-rs module breakdown, BFF dual-credential pattern, and Docker network topology details.
