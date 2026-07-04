# Developer Quickstart: 027-clean-timeline-infra

## What this feature does

Removes three infra-ring imports from `controller/timeline.rs` (VG-03 in
`docs/plans/ven_backend_architecture_refactoring_v2.md`):

```
use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState}  ← removed
```

After this change, `controller/timeline.rs` is a pure domain module: it depends only
on `entities/` and `controller/` types. The infra-to-domain conversion happens
exclusively in `simulator/mod.rs::to_timeline_snapshot()`.

## Files changed

| File | Change |
|------|--------|
| `VEN/src/controller/timeline.rs` | Remove infra imports; replace `TimelineAssetData` and `TimelineSnapshot` fields with domain types; move `HeaterPlanTrajectory` here from `assets/heater.rs`; update `build_now_point` and `build_asset_timeline` to use pre-computed domain values |
| `VEN/src/assets/heater.rs` | Remove `HeaterPlanTrajectory` struct + `next_slot` impl (moved to domain); keep `plan_trajectory()` as infra construction helper if needed, or inline into `to_timeline_snapshot` |
| `VEN/src/simulator/mod.rs` | Update `to_timeline_snapshot()` to perform all infra→domain conversions: map `AssetHistoryBuffer` → `Vec<TimelinePoint>`, call `state_values()` per point, pre-compute `recent_avg_power`, build `HeaterPlanTrajectory` from heater config/state |

## Local build check

```powershell
# From repo root on Windows:
wsl -e bash -l -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && cargo check 2>&1"
wsl -e bash -l -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && cargo test 2>&1"
```

## Architecture invariants to verify before committing

```bash
grep "use crate::assets" VEN/src/controller/timeline.rs    # must be empty
grep "use crate::simulator" VEN/src/controller/timeline.rs # must be empty
wsl wc -l VEN/src/simulator/mod.rs                         # must be ≤ 500
```

## BDD test

Full test suite runs on Pi4-Server via SSH:

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && bash tests/run_all_tests.sh --e2e"
```

Timeline-specific BDD scenarios are in `tests/features/` — search for `timeline` to
find the relevant `.feature` files.

## Key design decisions (see research.md for full rationale)

- `TimelinePoint` carries `state_values: HashMap<String, f64>` pre-computed from
  `AssetConfig::state_values()` at the infra boundary — this preserves SoC, temp, etc.
  in the past history display.
- `HeaterPlanTrajectory` moves to `controller/timeline.rs` (pure math, no infra deps).
  Its construction (reading `AssetConfig::Heater` + `AssetState::Heater`) stays in
  `to_timeline_snapshot()`.
- `TimelineAssetData.current_power_kw` holds the pre-computed 60-second rolling average
  for the now-point — replaces the `AssetHistoryBuffer::recent_avg_power()` call in
  `build_now_point`.
- `TimelineSnapshot.grid_current_kw` is a new field for the grid now-point.

## Pre-existing issue (do not fix in this PR)

`controller/timeline.rs` is 1316 lines — pre-existing 500-line violation. This feature
will not make it worse but will not fully resolve it. A dedicated file-split task should
follow.
