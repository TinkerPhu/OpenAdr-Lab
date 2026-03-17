# Research: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

## Decision 1: Timeline Data Structure

**Decision**: Use the existing `AssetTimelinePoint { ts: DateTime<Utc>, values: HashMap<String, f64> }` struct already defined in `VEN/src/controller/trace.rs`. The same struct serves both the history buffer rows and the new merged timeline output.

**Rationale**: `AssetHistoryBuffer.to_timeline(window)` already returns `Vec<AssetTimelinePoint>`. The plan future slots use the same shape with known keys (`power_kw`, `cost_rate_eur_h`, `co2_rate_g_h`). Using one type end-to-end avoids mapping overhead.

**Alternatives considered**:
- A separate `MergedTimelinePoint` type — rejected (no benefit; same fields, extra type to maintain).
- Adding an `is_forecast: bool` field — rejected by spec (past/future implicit in `ts` vs. `now`).

---

## Decision 2: Future Point Cost/CO₂ Computation

**Decision**: Use `PlanTimeSlot.import_price_eur_kwh`, `co2_g_kwh`, and `PacketAllocation.power_kw` to compute cost/CO₂ rates for future timeline points. `cost_rate_eur_h = allocation.power_kw * slot.import_price_eur_kwh`. `co2_rate_g_h = allocation.power_kw * slot.co2_g_kwh`.

**Rationale**: `PlanTimeSlot` already contains the per-slot tariff values used during planning. Re-deriving them from `TariffSnapshot` would require a second O(n) lookup; using the plan's cached values is simpler and consistent.

**Alternatives considered**:
- Re-scanning `TariffSnapshot` list at query time — rejected (redundant; plan already holds the values).

---

## Decision 3: Grid Timeline Source

**Decision**: For `asset_id = "grid"`, future points use `PlanTimeSlot.net_import_kw` / `net_export_kw` and the slot's tariff fields. Past points use `AssetHistoryBuffer["grid"]` which is written by `monitor.rs` with the site-level power reading.

**Rationale**: The grid "asset" is not a physical asset entry in `sim.assets` but it does have a history buffer entry keyed `"grid"` (or the net site power row). Past tariff intervals are stored in `TariffSnapshot.is_forecast = false` rows.

**Alternatives considered**:
- A dedicated grid timeline builder separate from the asset builder — rejected (same merge pattern; `asset_id = "grid"` is handled as a special case inside `build_asset_timeline` with a guard).

---

## Decision 4: Merge Strategy (Past + Future Boundary)

**Decision**: `now` (at query time) is the boundary. Points with `ts < now` come exclusively from `AssetHistoryBuffer`; points with `ts >= now` come from plan slots. No deduplication — if a plan slot happens to overlap a history point at the same second (rare edge), both are included and the client sorts by `ts`.

**Rationale**: The ring buffer capacity is 3600 rows = 1 hour at 1 Hz. Plan slots are 15-minute intervals. The two sources occupy non-overlapping time ranges in practice. The `window` parameter trims both ends: history to `[now - hours_back, now)`, plan to `[now, now + hours_forward)`.

**Alternatives considered**:
- Deduplication by timestamp — rejected (unnecessary complexity; the overlap never occurs in practice).

---

## Decision 5: `GET /timeline/all` Response Format

**Decision**: `HashMap<String, Vec<AssetTimelinePoint>>` serialised as a JSON object `{ "ev": [...], "battery": [...], "grid": [...], ... }`. The set of keys is `sim.assets.keys()` plus `"grid"`.

**Rationale**: Single-request fetch enables the stacked area chart (GridAccumulatedCell) and the page-level `useAllTimelines` hook without N+1 API calls. JSON object keyed by asset ID matches the existing `GET /sim` `assets` field convention.

**Alternatives considered**:
- An array of `{ assetId, points }` objects — rejected (harder to look up by ID on the client; object keying is idiomatic for this pattern).

---

## Decision 6: Frontend AssetTimelinePoint Type

**Decision**: Replace `AssetTimePoint` (flat named fields) with `AssetTimelinePoint { ts: number, values: Record<string, number> }` — matching the backend shape exactly. Chart components extract `values.power_kw`, `values.cost_rate_eur_h`, `values.co2_rate_g_h` by key.

**Rationale**: The backend returns a sparse values map. Projecting it to named fields at the hook layer would reintroduce the DTO normalization that Principle I prohibits. Client code accesses `values.power_kw` directly — readable and consistent with the backend contract.

**Alternatives considered**:
- Projecting to `{ powerKw, costRateEurH, co2RateGH }` in the hook — rejected (DTO normalization; violates Principle I).
- Keeping `AssetTimePoint` — rejected (it has `isPast` which must be removed; also the flat fields are incompatible with the sparse values map).

---

## Decision 7: Extended Window Toggle UI State

**Decision**: Per-cell `useState<boolean>` in the asset cell component (or a parent cell wrapper). `false` = default ±1h; `true` = extended parameters per cell type. State is lost on page reload (session-scoped, not persisted).

**Rationale**: Simplest implementation; no global state store needed. The extended window only affects the `hoursBack`/`hoursForward` args to the timeline hook, which changes the query key and triggers a cache-separate refetch.

**Alternatives considered**:
- Persisting in `localStorage` — deferred; not in spec.
- Global state store — rejected (unnecessary complexity for per-cell toggle).

---

## Decision 8: Dynamic Control Override POST Body

**Decision**: `POST /sim/override` continues to accept the existing `SimOverride` struct (a flat JSON object). The `DynamicControl` component builds a partial override object `{ [descriptor.key]: value }` and merges it into the current override state before posting. This avoids any backend API change.

**Rationale**: The existing `POST /sim/override` endpoint is already a full-replace API. The frontend already tracks the full override state. Adding `descriptor.key`-based controls does not require a new endpoint — just a change in how the client constructs the POST body.

**Alternatives considered**:
- A new `PATCH /sim/override/:key` endpoint — rejected (backend change not needed; client can track the full state).

---

## Decision 9: `buildAssetTimeline` Deletion Strategy

**Decision**: Delete `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` from `dataBuilders.ts`. Retain `findCurrentTariff`, `deriveAssetSummaries`, `deriveTariffSnapshot` until confirmed unused by left-section summary stats review; then delete remaining functions if unused.

**Rationale**: The three timeline builders are directly replaced by the backend endpoint. The summary stat helpers (`deriveAssetSummaries`, `deriveTariffSnapshot`) serve the left section live data display from `useSim()` + `useTariffs()` — not from timeline data. They remain useful until the left section is also migrated.

---

## Decision 10: `useRates` → `useTariffs` Rename

**Decision**: Rename `useRates` → `useTariffs` and `RateSnapshot` type → `TariffSnapshot` across all TypeScript source. The internal API call already targets `/tariffs` (not `/rates`) — only the naming changes.

**Rationale**: The hook was named `useRates` when the endpoint was `/rates`. The backend renamed to `/tariffs` in speckit 004 but the frontend was not updated. This speckit completes the rename for consistency (Principle I: one vocabulary everywhere).

**Alternatives considered**:
- Keeping `useRates` as an alias — rejected (aliases defeat the purpose of the rename; would require maintaining both names).
