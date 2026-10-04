# Proposal

## Why

The EV charts on the Control and History pages show power and SoC but not whether the car
was, or is predicted to be, plugged in. So "why didn't it charge here?" can't be answered
from the chart. The plugged state exists live (the EV asset reports `plugged`), but only the
Control page's past hour carries it to the API. History doesn't store it. For the future,
the EV's predicted presence is the planner's per-slot availability mask, which never leaves
the solver. That is derived state with no UI surface (`ui-transparency`).

## What Changes

- **The band marks absence, not presence.** A shaded background means "unplugged". No band
  means the EV is available, the same as every always-present asset (battery, heater, PV),
  which have no band either. Opacity is proportional to the unplugged fraction
  (`1 − plugged`), so a partly plugged minute or bucket is shaded partly.
- **Control, past:** EV timeline points already carry `plugged` (a 0..1 fraction after
  grid resampling). The chart draws the "Unplugged" band from it.
- **History:** the 1-minute history sampler records the EV's `plugged` as the window mean
  (0..1) into a new nullable `tick_samples.plugged` column. `/history/ticks` returns it, and
  the History EV chart draws the band from it.
- **Control, future:** the planner's per-slot EV availability is the EV's predicted
  presence: the live plug state, the stated sessions (away after a departure until a stated
  return) and the usage forecast where one exists. A solved plan carries it as `plugged`
  1.0/0.0 next to the EV's planned `soc` on each slot, the EV's future timeline points carry
  it, and the chart draws a visually distinct "Predicted away" band. Planner behaviour is
  unchanged.
- **Chart refactor (no behaviour change for PV):** one generic time-range shading helper in
  the shared chart package (`ui-charts`), next to the existing day/night and plan-zone
  shading. Each asset declares its own shading (classifier and colours). PV curtailment
  moves onto it, EV unplugged is the second declaration. The EV band is painted as a real
  background, under the lines.
- One wire name, `plugged` (1 = plugged), on every layer: state map, plan state, timeline,
  history row. Only the chart inverts it for display.

No breaking changes: the new column and keys are additive and nullable. Rows and plans
without a value draw no band, i.e. they read as "plugged". That is accepted.

## Capabilities

### New Capabilities
- `ev-plugged-band`: the EV's measured and predicted plugged-in state is recorded, carried
  through the timeline and history APIs, and its absence is drawn as a background band on
  the EV charts of the Control and History pages.

### Modified Capabilities
<!-- none: openspec/specs/ holds no capabilities in this project -->

## Impact

- **VEN backend:** `controller/milp_planner/{asset_port,planned_state,results}.rs` (the
  availability mask written into the plan's planned state), `services/history_sampling.rs`,
  `entities/history.rs` (`TickSample.plugged`), `history_store` (schema v12).
- **APIs (additive):** `/timeline/*` future EV points gain `values.plugged`.
  `/history/ticks` rows gain `plugged: number | null`.
- **Shared chart package:** `ui-charts/src/` gains the time-range shading helper.
- **VEN UI:** `AssetTimelineChart.tsx` (PV classifier moves out, `pvCurtailment` prop
  removed), `AssetMidSection.tsx` (no more `assetId` branches), `controller/types.ts`
  (per-asset chart declarations), `History.tsx` and `api/types.ts` (`plugged` passed
  through).
- **Tests and docs:** Rust unit tests, UI unit tests, BDD scenarios in
  `tests/features/ven_timeline.feature` / `ven_history.feature`, and the use-case doc
  `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (UC-01 EV).
