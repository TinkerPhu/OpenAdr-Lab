# Data Model: Grid-Aligned UI Timeline

## Type Changes

### AssetTimelinePoint (updated)

Single field change — `values` becomes nullable.

| Field | Type (before) | Type (after) | Description |
|-------|---------------|--------------|-------------|
| ts | number | number (unchanged) | Epoch ms — X-axis value |
| values | Record<string, number> | Record<string, number> \| null | Sparse values map. `null` for empty grid buckets (no data covers this time bucket). |

### StackedAreaPoint (unchanged)

No changes. The positional-zip builder produces the same output type.

| Field | Type | Description |
|-------|------|-------------|
| ts | number | Epoch ms |
| {asset}_pos | number | Max(0, kw) for each asset |
| {asset}_neg | number | Min(0, kw) for each asset |
| gridPowerKw | number \| null | Grid net power from "grid" asset |

### TariffTimePoint (unchanged)

No changes. `buildPowerPoints` already produces `null` for missing values — it just needs to handle when the input `values` is `null`.

## Removed Code

| Item | Location | Replacement |
|------|----------|-------------|
| `findNearest()` | GridAccumulatedCell.tsx:17-32 | Positional indexing (direct array[i] access) |
| `TOLERANCE_MS` | GridAccumulatedCell.tsx:51 | Removed entirely (no tolerance needed) |
| `buildStackedFromAllTimelines()` | GridAccumulatedCell.tsx:34-80 | New positional-zip function (same location) |

## State Transitions

None — this feature is a read-only UI change. No new persistent state.
