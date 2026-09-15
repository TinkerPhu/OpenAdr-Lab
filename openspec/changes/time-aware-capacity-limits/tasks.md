## 0. Decisions and inventory

- [ ] 0.1 Confirm the design's open questions (missing duration, untimed events) with the user
- [ ] 0.2 Re-grep for every place that resolves interval timing or looks up a limit at a time (design Context tables); add any new finding to this change or to `docs/reference/TECHNICAL_DEBTS.md`

## 1. Shared interval-timing resolver (`openadr-interval-timing`)

- [ ] 1.1 Tests first: User Guide Example 8.10.1-1 (2 × 30 min), a 48-interval day, own period overriding with the next interval following it, single interval with event-level period, `0001-01-01` start, `P9999Y` duration, missing duration, no timing at all
- [ ] 1.2 `controller/event_timing.rs`: the resolver (one function, open-ended end represented once)
- [ ] 1.3 `collect_interval_groups` uses it (looping and priority resolution unchanged); update `test_parse_capacity_schedule_does_not_guess_for_multi_interval_events` to the spec's contiguous rule (explain the expectation change)
- [ ] 1.4 Alert, SIMPLE, dispatch and charge-state parsers use it; delete their private timing code; per-parser test for a multi-interval event without per-interval periods
- [ ] 1.5 `reporter::event_is_active` uses it; test for an event with only an event-level period in the past
- [ ] 1.6 Full `cargo test`, fmt, clippy on Node2

## 2. Shared capacity lookup and "in force now" (`time-aware-capacity-limits`)

- [ ] 2.1 Tests first: instant inside/outside, span over two limits, import and export, no limit
- [ ] 2.2 The lookup in `entities::capacity` (one function, span with instant as degenerate case)
- [ ] 2.3 `parse_capacity_state` limit fields from the schedule at `now`; `test_parse_capacity_state_strictest_wins` becomes a priority test (explain); future-limit and interval-start tests
- [ ] 2.4 GB-47 limit pass (`capacity_import_limit_at_kw`) and history sampler (`accumulator.rs` lookup) call the shared lookup; delete their own

## 3. Planner slot caps

- [ ] 3.1 Tests first (`milp_planner` inputs): a limit announced in advance caps only its slots; the seed's two-interval ev-charge-pause event; a 20-min limit inside a coarse slot caps the slot; SIMPLE level 1 on a capped slot; alert still 0
- [ ] 3.2 Schedule into `SolveRequest` (`tasks/planning/cycle.rs`, `services/planning`, `controller/solver_port.rs`)
- [ ] 3.3 `build_milp_inputs`: per-slot `cont_imp`/`cont_exp` from the lookup, physical limit and allowance
- [ ] 3.4 Full `cargo test`, fmt, clippy, file-size audit, architecture greps on Node2

## 4. Use case and BDD

- [ ] 4.1 BDD: a VTN limit announced 30 min ahead → `/plan` caps only the overlapping slots, `/capacity` shows no limit yet, the limit appears in `/capacity` once it starts
- [ ] 4.2 BDD: a spec-form DOE event (event-level period, contiguous intervals) appears in `/capacity/schedule` with per-interval values
- [ ] 4.3 E2E + resilience on Node2 (detached); UI unit tests unchanged and green

## 5. Merge, deploy, docs

- [ ] 5.1 Rebase + ff-merge, deploy Node1 + Node2; check ven-2 against the seed-style event (live, then removed)
- [ ] 5.2 Docs: `VEN_ARCHITECTURE.md` (event timing, capacity schedule as single source), use-case doc (limit announced in advance), BACKLOG GB-48 → done (options B′ and C stay listed), TECHNICAL_DEBTS for the tariff "value at t" copies, project journal; delete this change
