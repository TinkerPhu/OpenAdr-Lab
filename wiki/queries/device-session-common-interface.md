---
title: "Query: Would a Common Interface for Device Sessions Help?"
type: query
created: 2026-07-04
updated: 2026-07-04
synced_commit: 5a9a304
sources: [VEN/src/entities/user_request.rs, VEN/src/services/user_request.rs, VEN/src/entities/device_session.rs, VEN/src/assets/ev.rs, VEN/src/state.rs, VEN/src/tasks/planning.rs]
tags: [query, device-session, architecture, milp]
---

# Query: Would a Common Interface for Device Sessions Help?

**Question**: Wouldn't it be advantageous and simplifying to have a common interface for
"schedulable unit of energy delivery for a specific asset: a fixed amount of energy (kWh,
or an equivalent target such as SoC or temperature) and a deadline" — i.e. a shared trait
across `EvSession`/`HeaterTarget`/`ShiftableLoad`?

## Answer

No — recommended against, for two reasons grounded in the current code.

**A common interface for the target+deadline data already exists, as a struct, not a
trait.** `UserRequest` (`entities/user_request.rs`) carries `target_energy_kwh`,
`desired_power_kw`, `deadlines`, `session_id`, `session_type` — this *is* "schedulable
energy + deadline," unified across all three creation paths
(`UserRequestService::create_ev/create_heater/create_shiftable`,
`services/user_request.rs`). The type-specific structs exist beneath that layer precisely
because what happens next diverges too much to unify further ([[hems-planning]]).

**The part that would genuinely benefit from a trait already has one — `AssetMilpContext`**
([[asset-layer]], [[ven-hexagonal-architecture]]). The solver only ever sees
`Vec<Box<dyn AssetMilpContext>>`; a `DeviceSession` trait would sit below that and
wouldn't replace it.

**Why a session-level trait wouldn't shrink code:**
- The session→MILP translation is asset-specific and doesn't unify: EV computes
  `core_kwh = (target_soc − current_soc) × battery_kwh` and tracks plugged state
  (`assets/ev.rs`); heater works off temperature delta; `ShiftableLoad` isn't a
  level-by-deadline problem at all — it's fixed `power_kw` for `duration_min` within
  `[earliest_start, latest_end]`, a block-placement constraint. A trait covering all
  three would need an enum/associated-type escape hatch for "target," reimplementing the
  dispatch `SessionType` already provides.
- Storage cardinality differs by domain fact, not accident: `state.rs` holds
  `Option<EvSession>`, `Option<HeaterTarget>` (one EV, one heater) but
  `Vec<ShiftableLoad>` (multiple loads can be scheduled concurrently).
- No code today needs polymorphic iteration over sessions. `tasks/planning.rs:74-76`
  fetches each session type individually and feeds it to its own `*MilpContext`
  constructor — every current caller already wants the concrete type.

**When it would be worth it**: if a real cross-cutting need appears — e.g. a generic
"sessions expiring soon" check — a thin trait exposing just `fn deadline(&self) ->
DateTime<Utc>` could make sense then. Not speculatively now; nothing would consume it
today.
