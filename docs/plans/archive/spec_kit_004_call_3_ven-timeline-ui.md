# Speckit Call 3 — VEN Timeline API & UI Rebuild

**Speckit feature name**: `ven-timeline-ui`
**Depends on**: `ven-simulator-reform` (call 1) AND `ven-controller-reform` (call 2) must both be complete
**This is the final speckit in the sequence**

## How to invoke

```
/speckit.specify <paste the Feature Description section below>
```

When prompted for a feature path / name, use: `ven-timeline-ui`

---

## Feature Description

Implement the backend timeline endpoints and rebuild the VEN controller-v2 UI to use them. This speckit delivers the complete data pipeline from backend history/plan to rendered asset cell diagrams, including dynamic simulation controls, per-cell extended time windows, and the stacked area accumulated view. The immediate diagram bugs (battery/base_load missing, PV wrong data, X-axis auto-domain, nowMs not memoized) are resolved as a natural consequence of the new data source.

### Prerequisites

Both previous speckits must be complete:
- **Speckit 1** (`ven-simulator-reform`): `Vec<AssetEntry>`, `AssetHistoryBuffer`, `SimSnapshot` with generic `assets` map, `GET /sim/schema`
- **Speckit 2** (`ven-controller-reform`): `AssetHistoryBuffer` is being written to each tick by `monitor.rs`, `controller/timeline.rs` stub exists, `GET /trace/history` works

### Scope

#### 1. Backend — `controller/timeline.rs`

Implement the stub created in speckit 2. This module assembles `Vec<AssetTimelinePoint>` by merging:
- **Past**: rows from `AssetHistoryBuffer` for the requested asset within the time window
- **Future**: plan slot allocations for this asset (FIRM + FLEXIBLE), with tariff-derived cost/CO₂

```rust
pub fn build_asset_timeline(
    asset_id: &str,
    history: &AssetHistoryBuffer,
    plan: &Plan,
    rates: &[RateSnapshot],
    window: TimeWindow,   // { hours_back: f64, hours_forward: f64 }
) -> Vec<AssetTimelinePoint>
```

`AssetTimelinePoint`:
```rust
struct AssetTimelinePoint {
    ts:     DateTime<Utc>,
    values: HashMap<String, f64>,
}
```

Well-known value keys:

| Key | Applies to |
|---|---|
| `power_kw` | all assets, grid |
| `cost_rate_eur_h` | all assets, grid |
| `co2_rate_g_h` | all assets, grid |
| `soc_pct` | EV, Battery |
| `temp_c` | Heater |
| `irradiance` | PV |
| `import_price_eur_kwh` | grid |
| `export_price_eur_kwh` | grid |
| `import_limit_kw` | grid |
| `export_limit_kw` | grid |

No `is_forecast` field — past vs. future is implicit in `ts` vs. `now`.

For future points (from plan), compute cost/CO₂ from the rate schedule at `slot.start`. For past points, these values are already stored in `AssetHistoryBuffer` (written by `monitor.rs`).

For the grid asset (`asset_id = "grid"`), merge tariff intervals with plan `net_import_kw` / `net_export_kw` for future points, and `AssetHistoryBuffer["grid"]` for past.

Function must be side-effect-free and fully unit-testable.

#### 2. Backend — new HTTP endpoints

**`GET /timeline/{asset_id}?hours_back=1&hours_forward=3`**
- Calls `build_asset_timeline` for the named asset
- Returns `Vec<AssetTimelinePoint>` (JSON array, sorted by `ts`)
- Valid `asset_id` values: any key in `sim.assets` plus `"grid"`
- `hours_back` and `hours_forward` default to 1.0 each

**`GET /timeline/all?hours_back=1&hours_forward=3`**
- Calls `build_asset_timeline` for every configured asset + grid
- Returns `HashMap<String, Vec<AssetTimelinePoint>>`
- One request covers all cells and the stacked area view
- Same time window parameters as the per-asset endpoint

**`GET /sim/schema`** (if not already exposed from speckit 1 wiring)
- Returns `HashMap<String, Vec<ControlDescriptor>>` from `AssetState::control_schema()` for all configured assets
- Used by UI to render dynamic simulation controls

#### 3. UI — API rename (UI half)

Complete the `/rates` → `/tariffs` rename on the frontend (backend renamed in speckit 2):

| Old | New |
|---|---|
| `useRates()` | `useTariffs()` |
| `RateSnapshot` TypeScript type | `TariffSnapshot` |
| `rates` prop/variable names | `tariffs` |
| `dataBuilders.ts` comment about RateSnapshot misnomer | Remove (misnomer is fixed) |

All components that previously imported `useRates` or referred to `RateSnapshot` are updated. No behavioral change — only naming.

#### 4. UI — API hooks

Add to `VEN/ui/src/api/hooks.ts`:

```ts
// Per-asset timeline (for individual asset cells that need independent windows)
useTimeline(assetId: string, hoursBack: number, hoursForward: number)
// → { data: AssetTimelinePoint[] }

// All-asset timeline (for page-level prefetch + stacked area)
useAllTimelines(hoursBack: number, hoursForward: number)
// → { data: Record<string, AssetTimelinePoint[]> }

// Control schema (static per session)
useSimSchema()
// → { data: Record<string, ControlDescriptor[]> }
```

`AssetTimelinePoint` in TypeScript:
```ts
interface AssetTimelinePoint {
  ts: number;      // epoch ms
  values: Record<string, number>;
}
```

Remove from hooks: any hook that joins `/trace` + `/plan` + `/rates` data in the frontend. These joins move to the backend.

#### 5. UI — remove dataBuilders.ts frontend assembly

Remove from `VEN/ui/src/components/controller-v2/dataBuilders.ts`:
- `buildAssetTimeline` — replaced by `GET /timeline/{asset_id}`
- `buildTariffTimeline` — replaced by `GET /timeline/grid`
- `buildStackedAreaData` — replaced by `GET /timeline/all`

`findCurrentTariff`, `deriveAssetSummaries`, `deriveTariffSnapshot` may be retained if still needed for the left section summary stats (live sim snapshot data, not timeline). Otherwise remove.

#### 6. UI — AssetTimelineChart fixes

Fix the four root-cause diagram issues:

| Issue | Fix |
|---|---|
| No past data for battery / base_load | Data now comes from `AssetHistoryBuffer` via timeline endpoint — all assets present |
| PV shows setpoint limit not generation | `power_kw` in timeline is actual measured value from `SimSnapshot`, not reactor setpoint |
| X-axis auto-domain | Fixed ±1h domain by default: `domain={[nowMs - 3_600_000, nowMs + 3_600_000]}` |
| `nowMs` not memoized | `const nowMs = useMemo(() => Date.now(), [])` at page level — or passed as stable prop |

Additional fixes:
- Remove `isPast` from `AssetTimePoint` type — field is unused, concept is implicit in timestamp position
- Line style identifies series (not past/future): power=solid, cost rate=dashed, CO₂eq=dotted — fixed across full time span
- Add a vertical reference line at `nowMs` (red dotted) to mark the present within the diagram
- Display asset name as the diagram title (not "Power")

Three series per asset cell in the asset color:

| Series | Line style | Y-axis | Unit |
|---|---|---|---|
| `power_kw` | solid | left | kW |
| `cost_rate_eur_h` | dashed | right | €/h |
| `co2_rate_g_h` | dotted | right | g/h |

#### 7. UI — per-cell extended time window toggle

Each cell header gets a toggle icon (no checkbox — single icon button indicating active mode). State is per-cell, session-scoped. Toggles between default and extended time window:

| Cell | Default | Extended window |
|---|---|---|
| EV | ±1h | Until SoC target or 24h cap (`hours_forward=24`) |
| Battery | ±1h | 24h (`hours_forward=24`) |
| Tariff / GridTariffCell | ±1h | 24h forward, no past (`hours_back=0, hours_forward=24`) |
| Capacity limits | ±1h | 24h forward, no past |
| Plan overview / GridAccumulatedCell | ±1h | Full plan horizon 24h |
| Heater, PV, BaseLoad | ±1h | No extended view |

The toggle updates `hoursBack` / `hoursForward` parameters passed to the relevant hook. The backend returns exactly the requested window — no frontend trimming.

#### 8. UI — AssetRightSection dynamic controls

Replace hardcoded per-asset control logic in `AssetRightSection` with schema-driven rendering:

```tsx
function AssetRightSection({ assetId, simOverrides, onOverrideChange }) {
  const { data: schema } = useSimSchema();
  const controls = schema?.[assetId] ?? [];
  return (
    <Box>
      {controls.map(descriptor => (
        <DynamicControl
          key={descriptor.key}
          descriptor={descriptor}
          value={simOverrides?.[descriptor.key] ?? null}
          onChange={(key, val) => onOverrideChange({ [key]: val })}
        />
      ))}
    </Box>
  );
}
```

`DynamicControl` renders a `Slider`, `Switch`, or `NumberInput` based on `descriptor.kind`. Override POST body is a generic `Record<string, number>` matching the schema key names.

#### 9. UI — GridAccumulatedCell stacked area

`GridAccumulatedCell` calls `useAllTimelines` and extracts the `power_kw` series per asset for stacking:

```tsx
const { data: allTimelines } = useAllTimelines(hoursBack, hoursForward);
// allTimelines["ev"] → AssetTimelinePoint[] with values.power_kw
// allTimelines["heater"] → ...
// zip on ts, stack positive values above X axis, negative below
```

Because all assets are sampled on the same 1s tick clock, timestamps align exactly — no interpolation needed. The stacked area uses existing Recharts `<AreaChart>` with positive/negative split per asset (same `_pos`/`_neg` pattern as before, but data comes from the timeline endpoint instead of `buildStackedAreaData`).

#### 10. UI — ControllerV2 page cleanup

In `ControllerV2.tsx`:
- Replace `nowMs = Date.now()` (recalculated every render) with `const nowMs = useMemo(() => Date.now(), [])` — or derive it from the latest `ts` in the sim snapshot
- Remove calls to `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData`, `buildTariffSnapshot`
- Use `useAllTimelines` at the page level; pass individual asset slices down to `AssetCell` components

#### 11. BDD test suite updates (timeline + sim response scenarios)

Per the pre-agreed scope:

**Rewrite**:
- `GET /sim` response assertions — field paths change from named snapshots to `assets.<id>.values.<key>`
- Asset timeline data scenarios — update to use `GET /timeline/{asset_id}` and assert `values.power_kw`
- `GET /timeline/all` — new scenarios for stacked area data source

**New scenarios**:
- `GET /timeline/{asset_id}` returns past + future merged points
- `GET /timeline/grid` returns tariff + net power timeline
- `GET /timeline/all` returns all assets in one call
- Per-cell extended window: `hours_forward=24` returns correct horizon
- Event-driven status reports sent after plan cycle (if not covered in speckit 2)

**Verify** (no change expected):
- UC-01–UC-12 controller scenarios — data pipeline changes should not affect planner/dispatcher/request behavior

### API changes in this speckit

| Endpoint | Change |
|---|---|
| *(new)* `GET /timeline/{asset_id}` | Unified past+future timeline per asset |
| *(new)* `GET /timeline/all` | All-asset timelines in one call |
| *(new)* `GET /timeline/grid` | Grid timeline (tariffs, capacity, net power) |
| `GET /sim/schema` | First UI consumer wired up (may already exist from speckit 1) |

### Acceptance criteria

1. `GET /timeline/ev?hours_back=1&hours_forward=1` returns a merged array of past (from history buffer) and future (from plan) `AssetTimelinePoint` entries sorted by `ts`.
2. `GET /timeline/all` returns entries for every configured asset and `"grid"`.
3. EV asset cell diagram shows actual past charging power (solid), cost rate (dashed), CO₂ rate (dotted) — no gap in the past section.
4. Battery and base_load asset cells show power data in the past section (previously missing).
5. PV asset cell shows actual generated power, not the export limit setpoint.
6. X-axis is fixed ±1h; the NOW reference line is always visible at the centre.
7. Extended window toggle works per-cell: EV switches to 24h forward, tariff cell shows no past.
8. `AssetRightSection` renders controls from schema — no hardcoded per-asset-type logic.
9. `GridAccumulatedCell` stacked area chart renders correctly from `useAllTimelines`.
10. `dataBuilders.ts`: `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` are deleted.
11. `AssetTimePoint.isPast` is removed from all TypeScript types.
12. `useRates` / `RateSnapshot` do not appear anywhere in TypeScript source; `useTariffs` / `TariffSnapshot` used throughout.
13. All timeline + sim-response BDD scenarios pass.
