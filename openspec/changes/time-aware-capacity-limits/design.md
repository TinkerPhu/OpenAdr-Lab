## Context

Capacity limits reach the VEN as OpenADR events and are parsed twice on every event poll (~30 s, `tasks/poll_events/detect.rs`):

- `parse_capacity_state` → `OadrCapacityState`: every IMPORT/EXPORT_CAPACITY_LIMIT value of every interval of every listed event, folded to the strictest; intervals' timing ignored. The VTN's `?active=true` keeps events whose lifespan has not ended, including ones that have not started.
- `parse_capacity_schedule` → `Vec<CapacitySnapshot>` (`planned_capacity_limits`): priority-resolved, non-overlapping `[start, end)` segments (GB-45), built by `collect_interval_groups`.

Who reads what today:

| Consumer | Reads | Time-aware? |
|---|---|---|
| Planner `cont_imp`/`cont_exp` + SIMPLE level-1 fraction (`milp_planner/inputs.rs`) | folded value, all slots | no |
| PV generation-limit resolver (`dispatcher::resolve_pv_generation_limit_kw`) | folded export | no |
| Grid asset limits (`sim_tick/finalize.rs`) | folded | no |
| `CapacityChange` trace (`detect.rs`) | folded import | no |
| `/capacity`, `/status` → Dashboard limit + compliance chip | folded | no |
| History sampler (`accumulator.rs`) | schedule, own "covers now" lookup | yes |
| GB-47 limit pass (`arbiter/limit.rs::capacity_import_limit_at_kw`) | schedule, own lookup | yes |

Interval timing — "when does interval i of this event run" — is decided in seven places:

| Place | Own period | Event-level fallback | Missing duration |
|---|---|---|---|
| `rate_schedule::collect_interval_groups` | yes | single-interval events only; multi-interval skipped | PT1H |
| `parse_alert_windows`, `parse_simple_windows`, `parse_dispatch_windows`, `parse_charge_state_setpoint` | yes | every interval gets the whole event window | PT1H |
| `reporter::event_is_active` | yes | none (no period → "active") | 1 year |
| `parse_capacity_state` | ignored | ignored | — |

The spec (OpenADR 3.1 User Guide §7.3 "intervalPeriod", Definition `eventRequest`/`interval`): an event-level `intervalPeriod` sets the start of the first interval and the default duration of all; an interval's own period overrides; an interval without its own start follows the previous interval immediately ("previous start + duration"). Example 8.10.1-1 (DOE) uses exactly that form. Schema default for a missing duration is `PT0S`; start `0001-01-01` means "now" and duration `P9999Y` means infinity.

## Goals / Non-Goals

**Goals:**
- One spec-conformant interval-timing resolver, used by every event parser; their private timing code deleted.
- One capacity-schedule lookup ("limit in force at t", "tightest limit over [from, to)"), used by every consumer that needs a limit at a time.
- Planner slot caps from the schedule; the folded state becomes "in force now".
- Spec-form DOE events parse into the schedule.

**Non-Goals:**
- Removing the folded limit fields and resolving at tick time everywhere (option B′; possible follow-up if the poll lag matters).
- Time-aware subscription/reservation and the spec's "fall back to SUBSCRIPTION when the limit schedule is exhausted" (option C).
- Multi-valued payloads (one interval, N values → N sub-intervals, §7.3); affects PRICE/GHG, separate item.
- `randomizeStart` (§7.3), unused by any VTN in this lab.
- Tariff "value at t" lookups (monitor, history sampler, tick cost); same shape of duplication, recorded as technical debt, not changed here.
- Feeding the sim-injected `grid_import_limit_kw` to the planner: it stays execution-only (GB-47's BDD relies on it).

## Decisions

### D1. One interval-timing resolver in `controller/`

A new `controller/event_timing.rs` (domain ring, next to `vtn_port`'s `OadrEvent`) exposes one function that returns every interval of an event with its absolute `[start, end)`. It walks intervals in order with a cursor starting at `event.intervalPeriod.start`: own start → absolute; otherwise the cursor. Own duration, else the event-level duration, else the missing-duration rule (D6). The cursor moves to the interval's end. `0001-01-01` starts and `P9999Y` durations keep their spec meanings. Open-ended ends are represented once (e.g. `DateTime::<Utc>::MAX_UTC`) so every caller compares plainly.

All seven call sites switch to it: `collect_interval_groups` (keeping only its looping expansion and priority resolution), the four window parsers, `reporter::event_is_active` ("any resolved interval covers now"), and `parse_capacity_state` (D3). Their private timing code is deleted, not wrapped.

*Alternatives:* fix only `collect_interval_groups` (smallest diff, but leaves six divergent copies — exactly the pattern this change exists to end); a trait per event kind (more machinery than one function needs).

### D2. Overlapping limits resolve by priority

The schedule already resolves overlaps by event priority, then `createdDateTime` (GB-45, spec §7.1), like prices. The "in force now" value is read from that schedule, so "strictest wins" disappears. One rule for every payload type. `test_parse_capacity_state_strictest_wins` becomes a priority test; the change will be explained with the implementation.

*Alternative:* keep "strictest wins" for limits only — conservative, but a second resolution rule that contradicts the spec and the schedule the history already records.

### D3. The folded state becomes "in force now"

`parse_capacity_state` keeps its signature and output type. Its `import_limit_kw`/`export_limit_kw` (+ event ids) are the schedule's values at the poll's `now`, via the shared lookup (D4). Subscription/reservation stay folded as today (non-goal). Every consumer of the folded value becomes correct unchanged. The `CapacityChange` trace now fires when the limit in force changes, including when a scheduled interval starts or ends.

*Accepted trade-off:* tick-time consumers of the folded value (PV export resolver, grid asset, Dashboard) lag up to one poll at an interval boundary. The planner's per-slot caps already curtail at the right slot, and the GB-47 limit pass and the history read the schedule at tick time.

### D4. One capacity-schedule lookup in `entities::capacity`

One function over `&[CapacitySnapshot]`: the tightest limit, per direction, among segments overlapping `[from, to)`; an instant is the degenerate span `[t, t]`, meaning segments with `start ≤ t < end`. Consumers:
- planner slot caps (span = slot);
- `parse_capacity_state` "in force now" (instant);
- GB-47 limit pass (instant; replaces `capacity_import_limit_at_kw`);
- history sampler per-sample limit (instant; replaces its own `find`).

The direction parameter reuses an existing import/export enum if one fits, else a small new one.

*Alternative:* a method on a `CapacitySchedule` newtype — nicer call sites, but changes the stored type in `AppState`, the route and the history sampler for no behaviour gain. Free function now; the newtype can come later.

### D5. Planner: slot cap = tightest overlapping scheduled limit

`SolveRequest` carries the schedule (from `state.planned_capacity_limits()` in `tasks/planning/cycle.rs`). `build_milp_inputs` computes `cont_imp[t]` = min(lookup over the slot, physical limit, allowance) and the same for export. SIMPLE level 1 takes its fraction of the slot's cap; alerts stay 0 on overlap. "Tightest overlapping" matches the alert/SIMPLE rule and never plans through the capped part of a coarse slot; near-horizon slots are 5 min, so the conservatism only touches the far horizon, and replans move the window into fine slots.

*Alternative:* time-weighted mean like tariffs — exact on energy, but lets a 1-h slot exceed a 20-min limit inside it.

### D6. Missing duration and untimed events — one rule each (confirm, see Open Questions)

- **Missing duration** (neither the interval nor the event gives one): open-ended. The VTN only lists events whose lifespan has not ended, so the event bounds it. Replaces PT1H (five parsers) and one year (reporter). The spec schema's `PT0S` default would silently drop such signals.
- **No start anywhere** (neither the interval, nor the event, nor a preceding interval): open-ended from the beginning of time, i.e. "in force while the VTN lists it". Keeps today's capacity behaviour for untimed events (the E2E UC events send 10000 kW untimed). Alerts, SIMPLE, dispatch and charge-state events without timing would become active; today they are skipped.

### D7. Looping stays a schedule concern

The "persistent daily prices" repetition (event duration longer than its intervals' span, `P9999Y`) stays in `collect_interval_groups`, applied to resolved intervals. Windows (alerts etc.) do not loop, as today.

## Risks / Trade-offs

- [Other parsers change for multi-interval events without per-interval periods: alert/SIMPLE/dispatch/charge-state windows become contiguous instead of all equal to the event window] → the spec defines contiguity; the lab's VTN tooling (harness, seed, BDD) sends single-interval events or per-interval periods, so no current sender is affected. Unit tests per parser pin the new shape.
- [`reporter::event_is_active` treats intervals without a period as active today; with the resolver they inherit the event's timing] → can change which events receive reports. Test: an event with only an event-level period in the past is no longer reported.
- [D6 untimed rule activates timing-less alerts/SIMPLE/dispatch] → confirm before implementation (Open Questions).
- [Poll lag for tick consumers of the folded value (D3)] → ≤ one poll (~30 s); the plan and the GB-47 pass carry the exact timing. B′ removes it if needed.
- [A planner that now sees future limits plans differently around them (pre-charging, pre-heating)] → intended; BDD asserts the plan shape, fleet KPIs (utilisation) can show the effect.

## Migration Plan

No persisted format changes: capacity state and schedule are in-memory and rebuilt every poll; `/capacity` keeps its shape. Deploy as usual (Node1 + Node2); rollback is a redeploy of the previous image.

## Open Questions

1. D6 missing duration: open-ended (recommended), PT1H (today's parsers), or PT0S (schema default)?
2. D6 untimed events: "in force while listed" for all event types (recommended; alerts etc. become active), or only for capacity limits (keeps today's alert skip, but two rules)?
