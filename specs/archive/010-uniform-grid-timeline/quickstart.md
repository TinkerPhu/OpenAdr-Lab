# Quickstart: Uniform-Grid Timeline API

## What This Changes

The VEN's `GET /timeline/all` and `GET /timeline/:asset_id` endpoints now resample all assets onto a **shared uniform time grid** with a **now-point**. The response format is unchanged — the differences are:

1. All assets share the same `ts` values at each array position (no more cross-asset misalignment).
2. Each array has three segments: history grid + now-point + future grid.
3. Grid timestamps are snapped to round boundaries (deterministic).
4. Empty buckets have `values: null` instead of being absent.

## Key Files

| File | Change |
|------|--------|
| `VEN/src/controller/timeline.rs` | New `resample_to_grid()` + `build_now_point()` functions |
| `VEN/src/main.rs` | Updated handlers: shared grid, `resolution` parameter, now-point insertion |
| `tests/features/timeline_grid.feature` | BDD scenarios for grid alignment + now-point |

## How to Test

### Unit tests (Rust)
```bash
cd VEN && cargo test timeline
```

### BDD tests (Docker)
```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/timeline_grid.feature"
```

### Manual verification
```bash
# Verify all assets have same timestamps and lengths
curl http://localhost:8211/timeline/all | jq '[to_entries[] | {key, len: (.value | length)}]'

# Verify uniform spacing in grid portions
curl http://localhost:8211/timeline/all | jq '.ev[:5] | [.[].ts]'

# Verify grid alignment across assets
curl http://localhost:8211/timeline/all | jq '{ev_ts: [.ev[:3][].ts], battery_ts: [.battery[:3][].ts]}'

# Test resolution parameter
curl "http://localhost:8211/timeline/all?resolution=30" | jq '.ev | length'

# Test deprecated max_points still works
curl "http://localhost:8211/timeline/all?max_points=100" | jq '.ev | length'
```

## What Stays the Same

- Response shape: `Record<string, {ts, values}[]>` — no structural change
- Value keys: `power_kw`, `soc`, `cost_rate_eur_h`, `co2_rate_g_h`, etc.
- 404 for unknown assets on `/:asset_id`
- `/tariffs` endpoint — not resampled

## What Changes

- All assets share identical `ts` values at each index (previously independently downsampled)
- Each array has three segments: history grid, now-point, future grid
- Grid timestamps snapped to round boundaries (deterministic)
- Empty grid buckets: `{"ts": "...", "values": null}` (previously absent)
- `resolution` parameter (seconds) replaces `max_points` as the primary density control
- `max_points` still accepted as deprecated alias
