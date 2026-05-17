# Requirement Gaps — DOCUMENTATION.md vs. Reference Docs

Comparison of `DOCUMENTATION.md` against all files in `docs/architecture/` and `docs/REQUIREMENTS.md`.  
Date: 2026-05-16

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
- No domain glossary (Utility, DSO, TSO, Aggregator, Prosumer, VEN, VTN, BFF) — REQUIREMENTS §2.1 is the authoritative source; DOCUMENTATION.md assumes readers already know these terms
- Sign convention not formally stated — REQUIREMENTS §2.5 defines positive = import; DOCUMENTATION.md uses the convention correctly but never defines it
- Domain entity definitions absent — `EnergyPacket`, `OadrEventSnapshot`, `FlexibilityEnvelope`, `UserRequest`, `AssetLedger`, and all enumerations (`AssetType`, `PowerAdjustability`, `PlanTrigger`, `UserRequestMode`, `CompletionPolicy`, `StaleRatePolicy`) are defined in REQUIREMENTS §3.2.1 but not in DOCUMENTATION.md
- Grid as virtual site boundary not explained — REQUIREMENTS §3.3 defines the power balance model; DOCUMENTATION.md says "aggregates all asset powers" without the formal equation
- Functional requirements (FR-OA-01…FR-OA-08, FR-ASSET-01…FR-ASSET-05, FR-SIM-01…FR-SIM-10) entirely absent — DOCUMENTATION.md describes features but does not cross-reference the FR codes that motivated them

**CONFLICTS:** None — DOCUMENTATION.md is consistent with REQUIREMENTS conventions.

**COVERED:** OpenADR 3.0 overview; tariff vs. rate distinction; VEN/VTN topology.

---

## `docs/architecture/VEN_ARCHITECTURE.md`

**GAPS:**
- Time-series alignment problem not documented — VEN_ARCHITECTURE §5 defines how tariff boundaries, per-interval capacity flattening, and last-write-wins event merging work; DOCUMENTATION.md has no equivalent section
- `TimeSeries<T>` abstraction and `Interpolation` enum (`Step` / `Linear`) not mentioned — these govern how tariff and capacity data are queried across slot boundaries
- Slot classification (`FIRM` vs `FLEXIBLE`, near-horizon boundary, early firm-up on flat rates) missing — relevant for plan stability guarantees
- Design Decisions D-01…D-07 not referenced — architectural rationale (why snapshot-and-release, why two-phase MIP, why per-asset ports) is not traceable to decisions in DOCUMENTATION.md
- Report generation alignment problem (§5.2–5.3) described in more detail than DOCUMENTATION.md §2.6 — obligation-based structure and interval boundary handling missing
- `CalcCache` struct and scoring strategy not mentioned

**CONFLICTS:**
- DOCUMENTATION.md §2.2 states Phase 2 optimises "friction" without initially clarifying Phase 1 cost is frozen. This was corrected in the latest edits (the "Independence of objectives" paragraph now states `c_star` is a hard constraint for Phase 2). No remaining conflict.

**COVERED:** Component overview; HEMS controller; asset abstraction; API contract; two-phase MIP structure; locking protocol.

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
- Two-layer heater control architecture (Layer A: physical heat-flow forecast; Layer B: MILP planning) not separated this explicitly in DOCUMENTATION.md
- Relay schema constraints and switching penalty structure (delta schema, two-relay on/off, 20% penalty for `0↔6` transitions) not in DOCUMENTATION.md
- Time discretisation rationale (5-minute step, 288 slots, 24-hour horizon) stated in DOCUMENTATION.md as defaults but not justified
- `min_run_slots` / `min_off_slots` parameters mentioned in DOCUMENTATION.md §2.4 conclusion but not yet in the profile YAML reference (§5) or heater MILP description
- Stale rate policy (`HEURISTIC_FORECAST` default) not mentioned in DOCUMENTATION.md

**CONFLICTS:** None.

**COVERED:** Heater thermal ODE; multi-tier control (off/mid/full); Phase 2 friction for switching.

---

## `docs/architecture/archive/ven_backend_review.md`

Review dated 2026-05-03. The following is the current verified status of each finding against live code.

### Architecture findings

| Finding | Status | Evidence |
|---------|--------|----------|
| **AB-01** `loops.rs` God Module (63 KB, 1077 lines) | ✅ Resolved | Split into `tasks/` with separate files: `planning.rs`, `poll_events.rs`, `poll_programs.rs`, `poll_reports.rs`, `obligation.rs`, `state_persist.rs`, `sim_tick/` |
| **AB-02** `milp_planner.rs` monolith (142 KB) | ✅ Resolved | Split into `controller/milp_planner/`: `solver_phase1.rs`, `solver_phase2.rs`, `inputs.rs`, `results.rs`, `types.rs`, `asset_port.rs`, `envelopes.rs` |
| **AB-03** Physics tick embeds absorber/escalation logic | ⚠️ Partial | `sim_tick/tick.rs` is 197 lines (within the 200-line task limit). Phases 3 and 6 (absorber, escalation) still inline rather than explicit controller calls, but within architectural bounds |
| **AB-04** `AppState` catch-all (90+ methods) | ⚠️ Partial | `state.rs` is 745 lines; still large. A `services/` layer now exists but `state.rs` retains many accessors |
| **AB-05** Routes reach into state directly | ✅ Resolved | `routes/hems.rs` imports `services::hems::EvSessionService` and `services::user_request::UserRequestService`; routes call services |
| **AB-06** `vtn.rs` returns untyped `serde_json::Value` | ✅ Resolved | Public methods (`fetch_events`, `fetch_programs`) return typed structs. Internal HTTP transport uses `Value`, satisfying the CLAUDE.md invariant ("must be empty or internal only") |

### Quality findings

| Finding | Status | Evidence |
|---------|--------|----------|
| **5.1** No panic recovery in background loops | 🔴 Open | No supervisor/restart wrapper in `main.rs`; a panic kills the task silently |
| **5.2** Profile validation absent on startup | 🔴 Open | No `Profile::validate()` found; invalid profiles cause silent wrong behaviour or runtime panics |
| **5.3** Non-atomic state persistence | ✅ Resolved | `simulator/persist.rs`: `write(&tmp_path)` → `rename(&tmp_path, &path)` |
| **5.4** `thiserror` declared but unused; `anyhow` everywhere | 🔴 Open | `thiserror = "1"` in `Cargo.toml`; no `DomainError` enum found anywhere |
| **5.5** No unit tests on control path | ✅ Substantially resolved | `dispatcher.rs` has 27 tests; `absorber.rs` has extensive departure-guard and dead-band tests; `services/planning.rs` has acceptance-gate tests |
| **5.6** EV departure guard not implemented | ✅ Resolved | Fully implemented in `absorber.rs` with 4 named test cases |

### Magic numbers — remaining open items

| Location | Value | Config path? |
|----------|-------|-------------|
| `tasks/planning.rs:32` | `5 s` initial planning delay | No — hardcoded `from_secs(5)` |
| `tasks/obligation.rs:22` | `5 s` obligation check interval | No — hardcoded `from_secs(5)` |
| `milp_planner/solver_phase1.rs:149` | `60 s` HiGHS solver timeout | No — hardcoded `with_time_limit(60.0)` |
| `vtn.rs` | `60 s` OAuth token expiry margin | No — hardcoded `elapsed + 60` |

`persist_every_s` (15 s) is now configurable via profile YAML — resolved.

### Code duplication — open items

- **CD-01** Per-asset config accessor pattern (`ev_config`, `heater_config`, etc.) — 4 identical `find_map` methods in `profile.rs`, still open
- **CD-02** 401-retry inline in 3 HTTP methods in `vtn.rs` — still open
- **CD-03** Energy counter accumulation per asset — not re-verified; marked unknown
- **CD-04** Setpoint clamping duplicated in dispatcher and absorber — not re-verified; marked unknown

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

**CONFLICTS:**
- Reactor design proposed here was superseded by the decision to remove the reactor (spec kit 001). DOCUMENTATION.md correctly reflects current state; this doc is partially obsolete.

**COVERED:** Asset physics simulation.

---

## `docs/architecture/packet_explanation.md`

**GAPS:**
- Packet semantic shift not documented — packets were originally the scheduling unit; they are now demoted to an intent/reporting metadata layer. DOCUMENTATION.md references packets but does not explain this demotion
- `FlexibilityEnvelope` as the primary per-packet output (not scheduling driver) not clarified

**CONFLICTS:**
- DOCUMENTATION.md implies energy packets participate in scheduling. This doc clarifies they do not — the MILP variables are the scheduling mechanism; packets track intent and feed reporting. Minor semantic conflict.

**COVERED:** Packet lifecycle states.

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
- Energy flow direction diagram with sign conventions — useful visual missing from DOCUMENTATION.md
- Time & clock management (NTP, ISO-8601 duration parsing) — not mentioned in DOCUMENTATION.md
- Security model details (OAuth scope, TLS optional) — DOCUMENTATION.md has minimal security content

**CONFLICTS:** None (archived reference material).

---

## Priority Gaps

Ranked by impact on a new reader's ability to understand, contribute to, or operate the system.

### Tier 1 — Critical for Correctness and Onboarding

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 1 | **Formal domain glossary and sign convention** | REQUIREMENTS.md §2 | Readers cannot interpret power values, costs, or API fields without the sign convention. Foundational for everything else. |
| 2 | **Time-series alignment architecture** | VEN_ARCHITECTURE.md §5 | Governs how tariff and capacity data is queried across slot boundaries in the MILP. Misunderstanding this leads to incorrect cost calculations and broken report intervals. |
| 3 | **Packet model semantic shift** | packet_explanation.md | DOCUMENTATION.md implies packets are scheduling units; they are not. Contributors building new features may build against the wrong abstraction. |

### Tier 2 — Important for Implementation Completeness

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 5 | **Functional requirements (FR-* codes)** | REQUIREMENTS.md §4 | No traceability between features and requirements. Cannot verify compliance or scope changes against a baseline. |
| 6 | **VEN provisioning sequence** | VTN_ARCHITECTURE.md §5 | Anyone setting up a new VEN instance has no documented procedure. |
| 7 | **Full OpenADR 3.0 signal taxonomy** | concept doc | Implementation only handles 7 of 17+ signal types. The gap between spec and implementation is invisible without a full taxonomy. |
| 8 | **Asset trait Rust type signatures** | ven_asset_interface_spec.md | Contributors adding new asset types have no formal interface contract to implement against. |
| 9 | **`min_run_slots` / `min_off_slots` in profile YAML** | heater_tank_milp_planning_model.md | Recently added to the conclusion in DOCUMENTATION.md §2.4 but not yet in the §5 config reference or the MILP description. |

### Tier 3 — Operational and Quality

| # | Gap | Source doc | Why it matters |
|---|-----|-----------|----------------|
| 10 | **Deployment topology detail** | VTN_ARCHITECTURE.md §6 | Docker bridge names, internal DNS, port mapping table needed for network troubleshooting. |
| 11 | **Hardcoded magic numbers** | ven_backend_review.md §3.3 | Values like 5 s, 15 s, 60 s are configurable or could be; their locations are not centralised. |
| 12 | **Test counts and run-time** | testing_landscape.md | DOCUMENTATION.md §7 describes what is tested but not how much or how long. |
| 13 | **Thermal model layer separation** | heater_tank_milp_planning_model.md | Layer A (forecast ODE) vs Layer B (MILP planning variables) distinction aids contributors adding new thermal assets (AC, floor heating). |
| 14 | **VTN internal architecture** | VTN_ARCHITECTURE.md | Entirely absent; matters when extending the VTN side or debugging event/report flows. |

---

## Recommended Actions

1. **Add a §0 Glossary** to DOCUMENTATION.md sourced from REQUIREMENTS.md §2 — domain terms and sign convention (5 min, high value).
2. **Add a §2.11 Time-Series Architecture** section explaining `TimeSeries<T>`, `Interpolation`, tariff alignment, and capacity slot flattening — sourced from VEN_ARCHITECTURE.md §5.
3. **Add a §4.8 Known Technical Debt** section listing the AB-0x findings with current status (resolved / open).
4. **Clarify packet role** in §2 or §4 — one paragraph stating packets are intent/reporting metadata, not MILP scheduling variables.
5. **Add a reference table to FR codes** in §2 feature sections — just `(FR-SIM-03)` inline anchors are enough to make the document traceable.
6. **Expand §5 config reference** to include `min_run_slots` / `min_off_slots` for the heater profile once those parameters are implemented.
7. **Link to reference docs** at the end of each major section rather than duplicating content — DOCUMENTATION.md is the operational guide; the architecture docs remain the design source of truth.
