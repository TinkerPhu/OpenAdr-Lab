# Tasks: VEN Timeline UI

**Input**: Design documents from `/specs/005-ven-timeline-ui/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅, quickstart.md ✅

**Organization**: Tasks grouped by user story. US1 (backend endpoints) is the load-bearing MVP — all UI stories depend on it being complete.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story label (US1–US6)

---

## Phase 1: Setup

**Purpose**: Write the new BDD feature file before any implementation (BDD-first per constitution).

- [ ] T001 Write failing BDD feature file tests/features/ven_timeline.feature from contracts/bdd-scenarios.md (all scenarios: per-asset, grid, all-assets, extended window, 404, future point enrichment)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Rust module scaffolding and HTTP route stubs that all user stories build on.

**⚠️ CRITICAL**: No user story work begins until this phase is complete.

- [ ] T002 Add `TimeWindow { hours_back: f64, hours_forward: f64 }` struct and `build_asset_timeline` function stub returning `Option<Vec<AssetTimelinePoint>>` in VEN/src/controller/timeline.rs (replace single-comment stub)
- [ ] T003 [P] Expose `pub mod timeline;` in VEN/src/controller/mod.rs so the module is accessible from main.rs
- [ ] T004 Add `TimelineParams { hours_back: Option<f64>, hours_forward: Option<f64> }` query struct and register `GET /timeline/all` (before `GET /timeline/:asset_id`) + `GET /timeline/:asset_id` stub handlers in VEN/src/main.rs (handlers return empty `[]` / `{}` until US1 implementation)

**Checkpoint**: `cargo build` succeeds; both routes respond 200 with empty responses.

---

## Phase 3: User Story 1 — Backend Timeline Endpoints (Priority: P1) 🎯 MVP

**Goal**: `GET /timeline/{asset_id}` and `GET /timeline/all` return merged past+future timeline data. `GET /timeline/grid` returns tariff + net power. Unknown asset_id → 404.

**Independent Test**: `curl http://ven:8080/timeline/ev?hours_back=1&hours_forward=1` returns a non-empty sorted array with `values.power_kw` in each point. `GET /timeline/all` contains keys for all assets + "grid". `GET /timeline/unknown_xyz` returns 404.

- [ ] T005 [US1] Implement past-history section of `build_asset_timeline` in VEN/src/controller/timeline.rs: look up `history.get(asset_id)`, call `buf.to_timeline(Some((now - hours_back_duration, now)))`, collect past `AssetTimelinePoint` rows
- [ ] T006 [US1] Implement future-plan section of `build_asset_timeline` in VEN/src/controller/timeline.rs: iterate `plan.firm_slots + plan.flexible_slots` within `[now, now + hours_forward_duration)`, find allocation for `asset_id`, compute `cost_rate_eur_h = power_kw * slot.import_price_eur_kwh`, `co2_rate_g_h = power_kw * slot.co2_g_kwh`, emit one `AssetTimelinePoint` per slot start
- [ ] T007 [US1] Implement grid special case in `build_asset_timeline` in VEN/src/controller/timeline.rs: when `asset_id == "grid"`, past = `history["grid"]`; future = net_import_kw/net_export_kw from plan slots plus tariff keys (`import_price_eur_kwh`, `export_price_eur_kwh`, `import_limit_kw`, `export_limit_kw`) from `TariffSnapshot` list; return `None` only when asset_id is unrecognised (not "grid" and not in history keys)
- [ ] T008 [US1] Replace stub `get_timeline` handler in VEN/src/main.rs: acquire sim + state locks, call `build_asset_timeline`, return `Json(points)` on Some or `StatusCode::NOT_FOUND` with JSON error on None
- [ ] T009 [US1] Replace stub `get_timeline_all` handler in VEN/src/main.rs: call `build_asset_timeline` for each key in `sim.assets` plus `"grid"`, collect into `HashMap<String, Vec<AssetTimelinePoint>>`, return `Json(map)`
- [ ] T010 [US1] Write cargo unit tests for `build_asset_timeline` in VEN/src/controller/timeline.rs: past-only window (`hours_forward=0`), future-only window (`hours_back=0`), merged window with both sections, grid asset with tariff data, unknown asset_id returns None, result sorted ascending by ts
- [ ] T011 [US1] Write BDD step definitions in tests/features/steps/ven_timeline_steps.py for all scenarios in ven_timeline.feature (GET /timeline/ev, GET /timeline/grid, GET /timeline/all, extended window, 404, future point enrichment checks)
- [ ] T012 [US1] Deploy to Pi4-Server and verify all ven_timeline.feature BDD scenarios pass: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_timeline.feature`

**Checkpoint**: All 7 ven_timeline.feature scenarios pass. `GET /timeline/ev` returns past + future merged and sorted. `GET /timeline/grid` has `import_price_eur_kwh` in future values. `GET /timeline/unknown_xyz` returns 404.

---

## Phase 4: User Story 2 — Asset Cell Diagram Fixes (Priority: P2)

**Goal**: All asset cell timeline charts (incl. battery, base_load) show continuous past data from timeline endpoint. PV shows actual generated power. X-axis is ±1h with NOW line always visible.

**Independent Test**: Open controller V2 UI — every asset cell has a chart with no empty past section. PV values are 0 at night. NOW reference line visible on all charts.

- [ ] T013 [P] [US2] Replace `AssetTimePoint` with `AssetTimelinePoint { ts: number; values: Record<string, number> }` in VEN/ui/src/components/controller/types.ts; remove `isPast` field; remove `TariffTimePoint.isPast` and `StackedAreaPoint` if they reference `isPast`
- [ ] T014 [P] [US2] Add `useTimeline(assetId: string, hoursBack: number, hoursForward: number)` hook to VEN/ui/src/api/hooks.ts (calls GET /timeline/{assetId}?hours_back=&hours_forward=, staleTime 10000, queryKey ["timeline", assetId, hoursBack, hoursForward])
- [ ] T015 [US2] Update `AssetTimelineChart` in VEN/ui/src/components/controller/charts/AssetTimelineChart.tsx: change props type to `data: AssetTimelinePoint[]`; access chart series as `values.power_kw`, `values.cost_rate_eur_h`, `values.co2_rate_g_h`; use recharts accessor function `(pt) => pt.values["power_kw"] ?? null` (not dot-notation dataKey since values is a nested map); fix X-axis domain to `[nowMs - hoursBack*3_600_000, nowMs + hoursForward*3_600_000]` defaulting to ±1h; pass `hoursBack` and `hoursForward` as props (defaults 1)
- [ ] T016 [US2] Wire `useTimeline` in AssetCell (VEN/ui/src/components/controller/AssetCell.tsx or equivalent): replace `buildAssetTimeline(...)` call with `const { data: timeline = [] } = useTimeline(assetId, hoursBack, hoursForward)`; pass `timeline` to `AssetTimelineChart`; add `nowMs` prop computed at page level
- [ ] T017 [US2] Update BDD tests in tests/features/controller/02_asset_cells.feature: add scenario "Battery asset cell shows past power data" and "Base load asset cell shows past power data"; update PV scenario to assert actual power values (not setpoint); confirm NOW reference line scenario still passes
- [ ] T018 [US2] Write step definitions for new 02_asset_cells scenarios in tests/features/steps/controller_steps.py (battery timeline chart visible, base_load timeline chart visible)
- [ ] T019 [US2] Deploy to Pi4-Server and run full BDD suite to confirm US2 scenarios pass and no regressions: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/controller/`

**Checkpoint**: Battery and base_load asset cells show timeline data. PV chart shows physics-model values. NOW line visible. All existing controller scenarios still pass.

---

## Phase 5: User Story 3 — Per-Cell Extended Time Window Toggle (Priority: P3)

**Goal**: Each supported asset cell has an icon toggle button. Activating it switches to the asset-specific extended time window; deactivating returns to ±1h.

**Independent Test**: Click the extended window toggle on the EV cell — chart expands to show 24h forward. Click again — returns to ±1h. Tariff cell toggle → `hours_back=0, hours_forward=24`. Heater/PV/BaseLoad have no toggle.

- [ ] T020 [US3] Add `EXTENDED_WINDOWS` lookup table and `extended` state toggle to AssetCell in VEN/ui/src/components/controller/AssetCell.tsx: `useState<boolean>(false)`; compute `hoursBack`/`hoursForward` from lookup; render icon toggle button with `data-testid="asset-cell-{assetId}-extend-btn"` only when the asset has an entry in `EXTENDED_WINDOWS`; pass computed window params to `useTimeline`
- [ ] T021 [US3] Add extended window support to `GridTariffCell` in VEN/ui/src/components/controller/GridTariffCell.tsx: replace `buildTariffTimeline(...)` call with `useTimeline("grid", hoursBack, hoursForward)`; add toggle icon for `hours_back=0, hours_forward=24` extended mode
- [ ] T022 [US3] Add BDD scenario "Per-cell extended window toggle expands EV horizon" to tests/features/controller/02_asset_cells.feature; add scenario "Tariff cell extended window shows no past"
- [ ] T023 [US3] Write step definitions for extended window scenarios in tests/features/steps/controller_steps.py; deploy and verify: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/controller/02_asset_cells.feature`

**Checkpoint**: Extended window toggle works per-cell. EV → 24h forward. Tariff → no past, 24h forward. Heater/PV/BaseLoad have no toggle icon.

---

## Phase 6: User Story 4 — Schema-Driven Simulation Controls (Priority: P4)

**Goal**: `AssetRightSection` renders controls from `GET /sim/schema` descriptors. `DynamicControl` dispatches Slider/Switch/NumberInput. No hardcoded per-asset-type conditionals remain.

**Independent Test**: `GET /sim/schema` returns descriptors; EV cell right section shows correct controls (plugged switch + SoC slider) generated from schema without any `if assetId === "ev"` conditionals in the UI code.

- [ ] T024 [P] [US4] Add `useSimSchema()` hook to VEN/ui/src/api/hooks.ts (calls GET /sim/schema, staleTime Infinity, queryKey ["sim", "schema"], returns `Record<string, ControlDescriptor[]>`)
- [ ] T025 [P] [US4] Create `DynamicControl` component in VEN/ui/src/components/controller/DynamicControl.tsx: props `{ descriptor: ControlDescriptor; value: number | boolean | null; onChange: (key: string, val: number | boolean) => void }`; dispatch to MUI Switch (kind=="Switch"), MUI Slider (kind=="Slider", min/max/step from descriptor), MUI TextField type="number" (kind=="NumberInput"); add `data-testid="ctrl-{descriptor.key}"`
- [ ] T026 [US4] Rewrite `AssetRightSection` in VEN/ui/src/components/controller/AssetRightSection.tsx to use `useSimSchema()`: `const { data: schema } = useSimSchema(); const controls = schema?.[assetId] ?? [];`; replace all hardcoded per-asset-type conditionals with `controls.map(d => <DynamicControl .../>)`; keep existing `simOverrides` + `onOverrideChange` props interface unchanged
- [ ] T027 [US4] Deploy to Pi4-Server and verify EV cell right section renders schema-driven controls: no `if assetId === "ev"` conditionals remain in AssetRightSection.tsx; `data-testid="ctrl-ev-plugged"` and `data-testid="ctrl-ev-soc"` still present (now from schema)

**Checkpoint**: AssetRightSection has zero hardcoded asset-type conditionals. All existing simulation control BDD scenarios (`controller/03_simulation_controls.feature`) still pass.

---

## Phase 7: User Story 5 — GridAccumulatedCell Stacked Area from Backend (Priority: P5)

**Goal**: `GridAccumulatedCell` renders stacked area from `useAllTimelines` instead of `buildStackedAreaData`. Per-asset `power_kw` series stack positive above and negative below the X-axis.

**Independent Test**: GridAccumulatedCell renders a stacked area chart; data comes from `GET /timeline/all`; `buildStackedAreaData` is no longer called anywhere.

- [ ] T028 [US5] Add `useAllTimelines(hoursBack: number, hoursForward: number)` hook to VEN/ui/src/api/hooks.ts (calls GET /timeline/all?hours_back=&hours_forward=, staleTime 10000, queryKey ["timeline", "all", hoursBack, hoursForward], returns `Record<string, AssetTimelinePoint[]>`)
- [ ] T029 [US5] Rewrite `GridAccumulatedCell` in VEN/ui/src/components/controller/GridAccumulatedCell.tsx: replace `buildStackedAreaData(...)` call with `const { data: allTimelines = {} } = useAllTimelines(hoursBack, hoursForward)`; extract `allTimelines[assetId]?.map(p => p.values["power_kw"] ?? 0)` per asset; zip on aligned timestamps; maintain existing `_pos`/`_neg` split per asset for recharts AreaChart; pass `nowMs` and `hoursBack`/`hoursForward` props

**Checkpoint**: GridAccumulatedCell stacked area chart renders correctly. `buildStackedAreaData` is not imported or called anywhere.

---

## Phase 8: User Story 6 — API Rename & Codebase Cleanup (Priority: P6)

**Goal**: `useRates` → `useTariffs`, `RateSnapshot` → `TariffSnapshot` everywhere in TypeScript. `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` deleted. `nowMs` memoized. `ControllerV2.tsx` cleaned up.

**Independent Test**: `grep -r "useRates\|RateSnapshot\|buildAssetTimeline\|buildTariffTimeline\|buildStackedAreaData\|isPast" VEN/ui/src` returns zero results.

- [ ] T030 [P] [US6] Rename `useRates` → `useTariffs` and return type `RateSnapshot[]` → `TariffSnapshot[]` in VEN/ui/src/api/hooks.ts; rename `RateSnapshot` type → `TariffSnapshot` and `PlannedRates` → `TariffSnapshot[]` in VEN/ui/src/api/hooks.ts; update all callers of `useRates` throughout VEN/ui/src/
- [ ] T031 [P] [US6] Delete `buildAssetTimeline`, `buildTariffTimeline`, and `buildStackedAreaData` from VEN/ui/src/components/controller/dataBuilders.ts; audit whether `findCurrentTariff`, `deriveAssetSummaries`, `deriveTariffSnapshot` are still used by any component for left-section summary stats — retain if used, delete if not
- [ ] T032 [US6] Fix `nowMs` in VEN/ui/src/components/controller/ControllerV2.tsx: replace `const nowMs = Date.now()` (recalculated each render) with `const nowMs = useMemo(() => Date.now(), [])`; remove all calls to deleted dataBuilders functions (`buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData`, `buildTariffSnapshot`); use `useAllTimelines` at page level if not already wired
- [ ] T033 [US6] Verify zero occurrences: `grep -r "useRates\|RateSnapshot\|buildAssetTimeline\|buildTariffTimeline\|buildStackedAreaData\|\.isPast\b" VEN/ui/src` — must return no results; fix any remaining references

**Checkpoint**: No banned symbols in TypeScript source. `dataBuilders.ts` contains only retained helper functions (or is empty/deleted). `nowMs` is memoized.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: BDD updates for existing features, documentation, full regression run.

- [ ] T034 [P] Update BDD ven_simulator.feature in tests/features/ to use `assets.<id>` field paths where applicable (replace any legacy named-field assertions with `assets.ev.power_kw` etc. per FR-028)
- [ ] T035 [P] Run full BDD test suite on Pi4-Server and confirm zero failures across all 27+ features: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` — investigate and fix any failures before proceeding
- [ ] T036 Write journal entry for speckit 005 in docs/history/project_journal.md: what was implemented, key decisions (recharts accessor vs dataKey, `EXTENDED_WINDOWS` lookup table, `build_asset_timeline` pure function, grid special case), issues encountered, lessons learned
- [ ] T037 Write key learnings in docs/reference/KEY_LEARNINGS.md: recharts nested object accessor pattern, `useAllTimelines` stale-time tradeoffs, `useMemo` for stable nowMs, `EXTENDED_WINDOWS` constant approach

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (BDD file written) — BLOCKS all user stories
- **US1 Backend (Phase 3)**: Depends on Phase 2 — first to implement; MVP
- **US2 Chart Fixes (Phase 4)**: Depends on Phase 3 (timeline endpoint must exist to wire the hook)
- **US3 Extended Window (Phase 5)**: Depends on Phase 4 (toggle changes `hoursBack`/`hoursForward` in `useTimeline`)
- **US4 Schema Controls (Phase 6)**: Depends on Phase 2 only (independent of US1–US3)
- **US5 GridAccumulatedCell (Phase 7)**: Depends on Phase 3 (needs `useAllTimelines` and working `/timeline/all`)
- **US6 Cleanup (Phase 8)**: Depends on all prior phases (deletes things replaced by US1–US5)
- **Polish (Phase 9)**: Depends on Phase 8

### User Story Dependencies

- **US1 (P1)**: Foundational only — no dependency on other stories
- **US2 (P2)**: US1 must be complete (wire hook to real endpoint)
- **US3 (P3)**: US2 must be complete (toggle drives `useTimeline` which is wired in US2)
- **US4 (P4)**: Foundational only — independent of US1–US3
- **US5 (P5)**: US1 must be complete (`useAllTimelines` needs `/timeline/all` working)
- **US6 (P6)**: All prior stories complete (cleanup deletes replaced code)

### Parallel Opportunities Within Phases

- **Phase 2**: T002, T003, T004 — T003 depends on T002; T004 depends on T002+T003; run T002→T003→T004 sequentially
- **Phase 3**: T005, T006, T007 can develop in parallel (all in timeline.rs); T008+T009 depend on T005–T007; T010 can start after T005–T007
- **Phase 4**: T013 + T014 can start in parallel immediately (types.ts and hooks.ts are independent files)
- **Phase 6**: T024 + T025 can run in parallel (hooks.ts and new component file)
- **Phase 8**: T030 + T031 can run in parallel (hooks.ts rename and dataBuilders.ts deletion are independent)
- **Phase 9**: T034 + T036 + T037 can run in parallel (different files)

---

## Parallel Example: Phase 3 (US1 Backend)

```
Can start together (all in timeline.rs, different sections):
  T005 — past history section
  T006 — future plan section
  T007 — grid special case

After T005–T007 complete:
  T008 — get_timeline handler (depends on build_asset_timeline)
  T009 — get_timeline_all handler (depends on build_asset_timeline)
  T010 — unit tests (depends on full implementation)

After T008–T009:
  T011 — BDD step definitions
  T012 — BDD verification run
```

---

## Implementation Strategy

### MVP First (Phase 1 → 2 → 3 only)

1. T001: Write BDD feature file (failing)
2. T002–T004: Rust module scaffolding and route stubs
3. T005–T012: Full backend timeline implementation + BDD passing
4. **STOP and VALIDATE**: `GET /timeline/ev`, `GET /timeline/all`, `GET /timeline/grid` all work correctly
5. Deploy/confirm against running Pi4 VEN

### Incremental Delivery

1. Phase 1 + 2 → foundation scaffolded
2. Phase 3 (US1) → backend endpoints working → MVP deliverable
3. Phase 4 (US2) → UI charts fixed, data from backend → visible to users
4. Phase 5 (US3) → extended window toggle → operational value
5. Phase 6 (US4) → dynamic controls → maintainability improvement
6. Phase 7 (US5) → stacked area from backend → data pipeline complete
7. Phase 8 (US6) → cleanup done → no dead code
8. Phase 9 → full regression confirmed + journal

---

## Notes

- **Recharts accessor**: `dataKey` dot notation does NOT work for nested `values.power_kw` maps. Use an accessor function: `<Line dataKey={(pt) => pt.values["power_kw"] ?? null} />`
- **Route order**: `GET /timeline/all` MUST be registered before `GET /timeline/:asset_id` in main.rs to prevent "all" being captured as an asset_id path parameter
- **Lock order**: In main.rs handlers, always acquire `sim` lock first, then `state`, consistent with existing handlers
- **Grid history**: Verify `AssetHistoryBuffer["grid"]` is being written by `monitor.rs` (speckit 004). If missing, the grid past section will be empty — acceptable fallback; add grid history write task if needed
- **POST /sim/override is full-replace**: DynamicControl must merge changed key into the full current override state before POSTing
- [P] tasks = different files, no blocking dependencies on concurrent tasks
- [Story] label maps task to specific user story for traceability
- Commit after each phase checkpoint — allows clean rollback if issues arise
