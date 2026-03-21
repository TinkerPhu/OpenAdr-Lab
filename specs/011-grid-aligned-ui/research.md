# Research: Grid-Aligned UI Timeline

## R1: How should `values: null` be represented in the TypeScript type?

**Decision**: Make `AssetTimelinePoint.values` a union type `Record<string, number> | null`.

**Rationale**: The backend sends `null` for empty grid buckets (RF-05c data-model.md). The simplest representation is a nullable field. This is a single-line type change that propagates naturally — every consumer that accesses `.values` must now null-check first.

**Alternatives considered**:
- Empty object `{}` instead of `null` — rejected because the backend sends `null`, and DTO normalization is prohibited (Constitution Principle I applied by analogy to internal APIs).
- Wrapper type `AssetTimelinePointOrGap` — rejected as unnecessary abstraction (Principle IV).

## R2: How should charts render null-values entries?

**Decision**: Filter out null-values entries before passing data to recharts. Entries with `values: null` produce no data point in charts (visual gap).

**Rationale**: recharts handles gaps natively when data points are missing from the array. Filtering nulls before chart rendering is simpler than converting to zero (which would be incorrect — null means "no data", not "zero power"). The `AssetTimelineChart` and `StackedAreaChart` both use recharts, so the same approach works everywhere.

**Alternatives considered**:
- Convert null to zero — rejected because zero power has a real meaning; null means no data covers that bucket.
- Use recharts `connectNulls` — rejected because the underlying data point shouldn't exist at all; filtering is cleaner.

## R3: Should the positional-zip builder live in GridAccumulatedCell or be extracted?

**Decision**: Keep the positional-zip builder as a module-level function in `GridAccumulatedCell.tsx`, replacing `buildStackedFromAllTimelines` in place.

**Rationale**: It's the only consumer of the stacked-area data shape. Extracting to a separate file adds indirection without benefit (Principle IV). The function signature changes but stays in the same location.

**Alternatives considered**:
- Extract to `dataBuilders.ts` — rejected because the stacked area builder is specific to `GridAccumulatedCell` and not reused elsewhere.

## R4: Should the API client parse `now_ms` from the response?

**Decision**: No. The response shape is unchanged — there is no top-level `now_ms` field. The now-point is embedded within each asset's array as a regular entry. The UI continues to derive `nowMs` from `Date.now()` (as it already does in `ControllerV2Page`).

**Rationale**: RF-05c chose to keep the response format flat. The now-point's `ts` is just another timestamp in the array — no special extraction needed. The UI uses client-side `Date.now()` for the cursor position, and the now-point provides the *value* at that time.

## R5: Impact on RawDiagnostics page

**Decision**: Minimal change. `RawDiagnostics` calls `api.allTimelines()` directly and displays raw JSON. It should handle `values: null` entries without crashing (just display them as-is in the raw output).

**Rationale**: The raw diagnostics page shows unprocessed data. Null values are valid JSON and should be displayed faithfully. No special rendering logic needed — just ensure no code crashes on null access.
