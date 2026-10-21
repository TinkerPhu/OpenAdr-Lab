## 1. Vocabulary and asset declarations (test first at every step)

- [x] 1.1 `NonPlanningDispatch::StartBy(DateTime<Utc>)`; `non_planning_dispatch(&self, deadline)` returns it for
      BY_DEADLINE and `StartNow` for ASAP; rewrite `non_planning_dispatch_starts_the_cost_aware_modes...` in
      `design_vocabulary.rs` (its purpose changes: say so in the commit message)
- [x] 1.2 `Asset::full_effort_time_to_target_s() -> Option<f64>` on the trait, carried on `AssetSnapshot`; EV (own
      efficiency, effective target, through `entities/units.rs`) and shiftable load (`duration_min`); heater `None`
- [x] 1.3 `Asset::dispatch_without_request() -> Option<NonPlanningDispatch>` carried on `AssetSnapshot`; EV `StartNow`
      while it can charge and its switch is on, else `None`; every other asset `None`

## 2. The decision

- [x] 2.1 `opportunistic_requests` passes each request's deadline (session `departure_time`, heater `ready_by`, load
      `latest_end`) into `non_planning_dispatch`
- [x] 2.2 `request_regime(sim, base, requests, now)`: `StartNow` as today; `StartBy(d)` waits (takes part in the pass)
      while `slack > DEADLINE_MARGIN_S` and starts at full effort once `slack <= DEADLINE_MARGIN_S`, with
      `None` time to target served as `StartNow`; no request reads `dispatch_without_request`; delete the
      `SurplusCharge && !switch` arm
- [x] 2.3 Unit tests in `controller/arbiter/tests/opportunistic_tests.rs` for each requirement scenario: ASAP at
      once; BY_DEADLINE waits then runs; surplus first moves the start later; stays started as time passes;
      deadline that cannot be met; asset without a declaration; implicit request, switch off, not plugged in
- [x] 2.4 `slot_decision` and `tasks/sim_tick/setpoints.rs` pass `now`; forecast test: a waiting EV is planned
      idle, then at full power to its target
- [x] 2.5 `OpportunisticOutcome.waiting`; regenerate the ts-rs types

## 3. Surfaces

- [x] 3.1 VEN UI: the opportunistic row of the arbiter diagnostics shows each waiting asset and its start; the EV card
      switch label says what "on" means under this objective; component tests
- [x] 3.2 BDD in `tests/features/ven_opportunistic_objective.feature`: replace "A BY_DEADLINE EV request charges at
      once, without look-ahead" with "An ASAP EV request charges at once" and "A BY_DEADLINE EV request waits, then
      charges"; add "A plugged-in EV with no request charges"; steps in the existing step file; run
      `python ci/audits/audit_step_definitions.py`
- [x] 3.3 Debt gate and architecture audit (`audit_ven_architecture.py`: no raw asset reads, no inline conversion)

## 4. Docs, issues, release

- [x] 4.1 Wave into `docs/architecture/VEN_ARCHITECTURE.md` (the opportunistic section: modes, margin, why additive,
      the tariff-shape caveat), UC-19 in `HEMS-USE-CASE-OBSERVATION-MANUAL.md`, `SYSTEM-USE-CASES.md` section 14
- [x] 4.2 Journal entry (the 2026-10-21 fleet run as the evidence: 0 kWh charged, 36 % less planned import, 0.111 to
      0.172 EUR/kWh) and a KEY_LEARNINGS entry: a baseline must serve the same demand as what it is compared with
- [x] 4.3 File issues with all four fields: service-level KPI (SoC at departure, unserved energy) on the Fleet plan
      card, needs-decision; heater latest start, too-big, first step is a time-to-target on the heater
- [ ] 4.4 After merge and deploy to Node1 and Node2: repeat the fleet run of `opportunistic-objective` 10.3 and read
      the card with the KPI issue in mind; record in the journal; then delete this change and
      `opportunistic-objective`
