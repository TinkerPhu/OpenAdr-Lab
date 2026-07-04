---
title: Three-Tier Plan Grid
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/architecture/ven_milp_planner.md, VEN/src/tasks/planning.rs]
tags: [planner, grid, zones, alignment]
---

# Three-Tier Plan Grid

How the [[milp-planner]]'s 48 h horizon is discretised
(docs/architecture/ven_milp_planner.md §2).

## Zones

| Zone | Range | Step | Slots | Purpose |
|---|---|---|---|---|
| A | 0–8 h | 5 min | 96 | EV deadlines, battery, heater cycles |
| B | 8–24 h | 10 min | 96 | overnight scheduling |
| C | 24–48 h | 15 min | 96 | inter-day thermal strategy |

Invariant: every zone's `step_s` must be an **integer multiple of Zone A's** (validated at
startup), so forward-filling coarse-zone data to Zone-A resolution is exact repetition,
never interpolation (§2.1). Configured per profile (`planner.plan_zones`); test profiles
use a single coarse zone for solver speed.

## The alignment rule

`now` is always truncated to the nearest Zone-A boundary before building the horizon:
`now_aligned = floor(unix_ts / step_A_s) * step_A_s` (§2.2). This is load-bearing, not
cosmetic — four reasons:

1. **Gate stability** — consecutive replans share identical slot grids, so the adoption
   gate compares cost slot-for-slot instead of comparing misaligned windows.
2. **Warm-start continuity** — slot *t* of the new plan covers the same physical window
   as slot *t* of the old plan.
3. **Block-commitment anchor** — locked setpoints (e.g. heater relays) stay on slot
   boundaries across replans.
4. **UI readability** — clean clock times in the [[ven-ui]] timeline.

## Three "nows" — do not mix

| Name | Value | Used for |
|---|---|---|
| `wall_now` | `Utc::now()` at loop top | `plan.created_at`, gate decay, envelope, status report |
| aligned `now` | `align_to_step(wall_now)` | all slot timestamps, tariff sampling, deadlines, MILP inputs |
| request `now` | `Utc::now()` per HTTP request | timeline now-point only |

Passing aligned time into gate decay would clamp elapsed time to zero whenever
`step_s > replan_interval_s`, silently disabling decay — hence `wall_now` is mandatory
post-solve (§2.2, timestamp inventory). First-slot convention: the first slot may start up
to one step in the past; the [[dispatcher]] executes it immediately on adoption.
