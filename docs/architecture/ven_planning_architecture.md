# VEN Planning & Asset Architecture

**Status:** Design guideline — forward-looking target architecture.
Describes the *ideal* design derived from first-principles analysis.
The delta between this and the current implementation is the refactoring backlog.

Related documents:
- [VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md) — current implementation reference
- [Domain_definitions.md](Domain_definitions.md) — vocabulary

---

## 1. Design Principles

These principles drive every decision in this document.

| # | Principle | Rationale |
|---|---|---|
| P1 | **Single truth per concern** | Each piece of data has exactly one owner. No mirroring, no derived caches masquerading as data. |
| P2 | **Physics lives in the asset** | Only an asset knows how its state evolves given a setpoint. The planner never embeds asset physics. |
| P3 | **Obligations live above assets** | The planner does not know the difference between an OpenADR FIRM event and a policy reserve — both are `Reservation` records. |
| P4 | **Transparency by construction** | Every planning decision emits a structured `PlanReason`. No post-hoc reconstruction. |
| P5 | **Flexibility is a first-class output** | The VEN's grid value is not its plan — it is the dispatchable headroom it can deliver on demand. This must be continuously computable. |
| P6 | **One planning model, always** | The planner is locked to greedy forward step. Switching models between invocations destroys traceability. Sophistication enters via richer context, not model mixing. |
| P7 | **Composition over magic** | `simulate_forward()` is built from `step()`. `capability_trajectory()` is built from `step()`. New assets implement one primitive; everything else is free. |

---

## 2. Asset Interface

Each controllable or observable device implements the `Asset` trait. The trait
boundary is the only contract between the planner and the physics.

```rust
trait Asset {
    // ── Identity ──────────────────────────────────────────────────────────
    fn id(&self) -> &str;

    // ── State ─────────────────────────────────────────────────────────────
    /// Current state snapshot (SoC, temperature, plug status, …).
    /// Used by: dispatcher, timeline, planner (as initial state).
    fn current_state(&self) -> AssetState;

    // ── Physics primitives ────────────────────────────────────────────────
    /// Dynamic capability given a state.
    /// Returns the physically feasible power range at this instant.
    /// Always SoC-dependent for storage assets — do not call with a stale state.
    fn capability(&self, state: &AssetState) -> AssetCapability;

    /// Advance one time step. Returns the new state and actual power.
    /// `actual_power_kw` may differ from `setpoint_kw` due to physics constraints
    /// (e.g., SoC ceiling clamps effective charge rate).
    fn step(
        &self,
        state: &AssetState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (AssetState, f64 /* actual_power_kw */);

    // ── History ───────────────────────────────────────────────────────────
    /// Slice of the asset's own history buffer over [now − window, now].
    /// The buffer is owned by the asset (single truth).
    /// Aggregation across assets is a query-time operation, not a storage operation.
    fn history(&self, window: Duration) -> TimeSeries;

    // ── Derived (default implementations, free for all assets) ────────────

    /// Project asset state forward given an explicit setpoint schedule.
    /// Composed from repeated `step()` calls. Planner uses this for plan validation
    /// and for the timeline future slice. No override needed for simple assets.
    fn simulate_forward(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        // default: chain step() calls across setpoint intervals
    }

    /// Project asset state forward with no external setpoint (idle / thermostat / free-run).
    /// Used for: timeline beyond plan horizon, greedy lookahead context.
    fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory {
        // default: simulate_forward with setpoint = 0.0 (or asset idle default)
    }

    /// How does capability evolve over time in free-run?
    /// Used by planner to detect opportunity windows (e.g., "battery fills by 10:30").
    fn capability_trajectory(
        &self,
        initial: &AssetState,
        duration: Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        // default: repeated step(idle_setpoint) + capability()
    }
}
```

### 2.1 Key types

```rust
struct AssetCapability {
    /// Floor of feasible power range. Always ≤ 0. -5.0 = can export up to 5 kW.
    max_export_kw: f64,
    /// Ceiling of feasible power range. Always ≥ max_export_kw.
    /// For uncontrollable assets equals max_export_kw (point-range).
    max_import_kw: f64,
    // may be extended: ramp_rate_kw_per_s, min_on_duration, …
}

/// State is asset-specific. The enum carries only the variant relevant
/// to the asset type — the planner receives it opaquely via AssetState.
enum AssetState {
    Battery  { soc_pct: f64, … },
    Ev       { soc_pct: f64, plugged: bool, … },
    Heater   { temperature_c: f64, … },
    Pv       { /* stateless — irradiance from time-of-day model */ },
    BaseLoad { /* stateless — fixed profile */ },
    Grid     { /* virtual — no controllable state */ },
}

struct Trajectory {
    points: Vec<TrajectoryPoint>,
}
struct TrajectoryPoint {
    ts:            DateTime<Utc>,
    power_kw:      f64,
    state:         AssetState,   // state AFTER this step — planner reads final row for chaining
}
```

### 2.2 Why `step()` is the single primitive

All higher-level functions are derived from `step()`:

```
capability_trajectory = repeated step(idle) + capability()
simulate_free         = simulate_forward(setpoints = [idle…])
simulate_forward      = repeated step() across setpoint intervals
```

A new asset only implements `capability()` and `step()`. It receives all planning
and timeline functions for free. This is the extensibility guarantee.

### 2.3 SoC is state, not capability

SoC is the time-integral of power flux:

```
SoC(t) = SoC(t₀) + ∫[t₀, t]  P(τ) / capacity_kwh  dτ
```

It is stored in `AssetState`, not `AssetCapability`. Capability is *derived from* SoC
at each call to `capability(state)`. This is intentional:

- `AssetCapability` is a momentary snapshot valid for one planning step.
- `AssetState` carries the integration variable that propagates across steps.
- The planner never integrates energy itself — it reads SoC from `TrajectoryPoint.state`
  after each `step()` call.

---

## 3. Timeline Architecture

The timeline for any asset has three segments with different owners.

```
──────────────────────────────────────────────────────────────────► time
│◄────── past window ──────►│◄─ now ─►│◄────── future window ──────►│
│                            │         │                              │
│  asset.history(window)     │ current │  simulate_forward(          │
│  (asset-owned ring buffer) │ _state()│    current_state,           │
│  single truth, no mirror   │         │    plan_setpoints)           │
│                            │         │  + simulate_free(            │
│                            │         │    final_plan_state,         │
│                            │         │    remaining_duration)       │
└────────────────────────────┴─────────┴──────────────────────────────┘
```

### 3.1 Past — asset-owned buffer

Each asset owns its history buffer (ring buffer, fixed capacity ≈ 1 h at 1 s ticks).
The dispatcher writes one row per tick: `(ts, power_kw, state_values…)`.

**No central mirror.** Aggregation across assets (e.g., grid = Σ assets) is a
query-time operation:

```rust
fn grid_power_at(t: DateTime<Utc>, assets: &[&dyn Asset]) -> f64 {
    assets.iter().map(|a| a.history_at(t).power_kw).sum()
}
```

Rationale: a mirror creates two sources of truth. Sparse columns, NaN alignment, and
lifecycle management (asset removed and re-added) are all simpler with one buffer per asset.

### 3.2 Current — `current_state()`

The live simulation tick output. No buffering. Timeline endpoints read it directly.

### 3.3 Future — plan-derived, not forecast-derived

The timeline future slice is built from plan setpoints via `simulate_forward()`, **not**
from the asset's standalone `forecast()` method (which is an internal planner tool).

```
For each plan slot in [now, horizon]:
    setpoint = plan.get_setpoint(asset_id, slot)
    trajectory = asset.simulate_forward(current_state, setpoints)

Beyond plan horizon:
    trajectory_tail = asset.simulate_free(final_plan_state, remaining_duration)
```

The free-run tail is the most honest representation of "what happens if no further
instructions arrive" — and that is the correct semantics for an unplanned period.

---

## 4. Planner Architecture

### 4.1 Locked model: greedy forward step

The planner is **permanently locked** to a single algorithm: greedy forward step with
per-step state tracking. This is a non-negotiable architectural constraint (P6).

Rationale:
- Every decision is traceable to an explicit state, capability, and fired rule.
- The algorithm is identical on every invocation — no mode switching.
- Sophistication enters via richer context (capability trajectory, lookahead window),
  not via a different model.

### 4.2 Planning loop

```
initial_state ← asset.current_state() for each asset

for each time step t in [now, horizon]:

    reservations_t ← reservation_layer.query(t)          // § 5

    for each asset a:
        phys_cap    ← a.capability(state[a])
        avail_cap   ← phys_cap − reservations_t[a.id]    // reserved capacity is off-limits

        (setpoint, reason) ← rules.choose(
            avail_cap,
            tariff[t],
            grid_limit[t],
            obligations[t],
            lookahead_context[a],                         // capability_trajectory result
        )

        (next_state, actual_kw) ← a.step(state[a], setpoint, dt)

        record PlanStep {
            ts,
            asset_id:       a.id(),
            state_before:   state[a],
            capability:     phys_cap,
            reserved_kw:    reservations_t[a.id],
            available_cap,
            setpoint_kw:    setpoint,
            actual_power_kw: actual_kw,
            reason,                                       // § 4.4
        }

        state[a] ← next_state
```

### 4.3 Lookahead context (greedy failure mitigation)

Greedy with a fixed threshold fails when a better opportunity lies N steps ahead
(e.g., a cheaper tariff window shortly after a cheap-but-not-cheapest window).

Mitigation: before the planning loop, compute a capability trajectory in free-run
for each asset. The `rules.choose()` function receives this as context:

```rust
struct LookaheadContext {
    capability_trajectory: Vec<(DateTime<Utc>, AssetCapability)>,
    tariff_minimum_ahead:  f64,   // min tariff in [t, t + lookahead_window]
    soc_ceiling_eta:       Option<DateTime<Utc>>,  // when does storage fill at idle?
}
```

Example rule enriched by lookahead:

> "Charge only if `tariff[t] ≤ tariff_minimum_ahead × (1 + tolerance)`"

The planning model remains greedy-forward. The lookahead is read-only context,
not a separate optimization pass.

### 4.4 PlanReason — the audit trail

Every `PlanStep` carries the rule that fired. This is not cosmetic: it is the
primary mechanism for operator transparency, UI decision traces, and test assertions.

```rust
enum PlanReason {
    FirmObligation     { source: ReservationSource, required_kw: f64 },
    CheapTariff        { tariff_eur_per_kwh: f64, threshold: f64 },
    ExpensiveTariff    { tariff_eur_per_kwh: f64, threshold: f64 },
    GridImportLimit    { limit_kw: f64 },
    GridExportLimit    { limit_kw: f64 },
    SocCeiling         { soc_pct: f64 },
    SocFloor           { soc_pct: f64 },
    UserOverride       { request_id: Uuid, mode: UserRequestMode },
    PolicyReserve      { policy_id: String },
    ComfortBound       { asset_id: String, bound: ComfortBound },
    OpportunityMissed  { reason: String },   // deliberate non-action with explanation
    Idle,
}
```

`reason` is emitted by `rules.choose()` at the same moment the setpoint is chosen.
It is never reconstructed after the fact.

---

## 5. Reservation Layer

### 5.1 Concept

The reservation layer is the single choke point between obligation sources and the
planner. The planner queries it; it does not know the source of any reservation.

```rust
struct Reservation {
    window:     (DateTime<Utc>, DateTime<Utc>),
    asset_id:   Option<String>,    // None = site-level (applies to all assets proportionally)
    kw:         f64,               // positive = import reduction; negative = export reduction
    direction:  FlexDirection,     // UP (reduce consumption) | DOWN (increase consumption)
    source:     ReservationSource,
    priority:   u8,                // lower = higher priority; for conflict resolution
}

enum ReservationSource {
    VtnFirmEvent   { event_id: String },
    PolicySchedule { policy_id: String },
    PolicyDefault,
    UserRequest    { request_id: Uuid },
}
```

### 5.2 Sources

All three sources produce `Reservation` records in the same format.

```
┌──────────────────────┐   ┌──────────────────────┐   ┌──────────────────────┐
│  VTN OpenADR         │   │  FlexibilityPolicy   │   │  User Requests       │
│  FIRM events         │   │  (§ 6)               │   │  (§ 7)               │
│  + pre-announced     │   │                      │   │                      │
│    future events     │   │                      │   │                      │
└──────────┬───────────┘   └──────────┬───────────┘   └──────────┬───────────┘
           │                          │                           │
           └──────────────────────────▼───────────────────────────┘
                              Reservation Layer
                              (time-indexed, priority-ordered)
                                        │
                                        │ query(t) → Vec<Reservation>
                                        ▼
                                   Planner (§ 4)
```

### 5.3 Conflict resolution

When two reservations overlap on the same asset:

1. Higher priority (lower number) wins.
2. Equal priority, same direction: take the stricter (larger kW).
3. Equal priority, opposing directions: flag `PlanWarning::ConflictingReservations`.

### 5.4 Site-level reservations

Some reservations apply to the site aggregate, not to a specific asset
(e.g., "reduce total import by 10 kW"). The planner distributes these
across assets using the `distribute()` policy:

- Default: proportional to current `capability.max_import_kw`.
- Override: explicit per-asset fractions in the reservation record.

---

## 6. FlexibilityPolicy

Proactive flexibility maintenance is architecturally identical to committed
flexibility maintenance. The policy generates synthetic reservations; the planner
treats them identically to VTN FIRM events.

### 6.1 Three policy layers

**Layer 1 — Default reserve (always active)**

```yaml
flexibility_policy:
  default_reserve:
    up_kw: 3.0      # always hold 3 kW headroom for consumption reduction
    down_kw: 3.0    # always hold 3 kW headroom for consumption increase
```

Produces one persistent low-priority `Reservation` per direction per asset.
This is the minimum viable proactive policy — it prevents the planner from
ever consuming all available headroom.

**Layer 2 — Scheduled windows (contracts, known DR patterns)**

```yaml
flexibility_policy:
  scheduled_windows:
    - id: "peak_dr_weekday"
      days: [Mon, Tue, Wed, Thu, Fri]
      time: "16:00–20:00"
      reserve_up_kw: 10.0
      pre_load_minutes: 60   # start reserving 60 min before window
```

The `pre_load_minutes` field is critical: it instructs the planner to lock the
required capacity early enough for it to actually be available at window start
(e.g., battery cannot charge from 10% to usable in seconds).

**Layer 3 — Pre-announced VTN events (future activePeriod)**

A VTN can announce an event with `activePeriod.start` in the future. The OpenADR
interface creates a reservation when the event is *received*, not when it becomes
active. The planner begins protecting capacity immediately.

```
Event received at 09:00, activePeriod: 15:00–17:00, 10 kW reduction required
→ Reservation created: [15:00–17:00, 10 kW UP, source: VtnFirmEvent]
→ Planner locks battery capacity from 09:00 to ensure 20 kWh available by 15:00
→ PlanReason::FirmObligation fires for any step that touches that capacity
```

### 6.2 Adaptive layer (future)

Learned patterns feed into Layer 2 automatically:

```
SignalHistoryBuffer
    → PatternDetector (cluster arrival times by weekday / season)
    → PolicyAdvisor (generate ScheduledWindow recommendations)
    → FlexibilityPolicy.scheduled_windows (operator confirms or auto-applies)
```

The output is always a `ScheduledWindow` — the downstream architecture is unchanged.

---

## 7. OpenADR Event Modeling

### 7.1 Event → internal obligation mapping

| OpenADR EventType | Internal destination | Mechanism |
|---|---|---|
| `SIMPLE` (active now, FIRM) | Reservation Layer | `Reservation { source: VtnFirmEvent, priority: 1 }` |
| `SIMPLE` (activePeriod future) | Reservation Layer (pre-announced) | Created at event receipt, active at window start |
| `PRICE` | Planner context (tariff[t]) | Not a reservation — informs `rules.choose()` |
| `IMPORT_CAPACITY_LIMIT` | Reservation Layer (site-level) | Hard constraint, priority 0 |
| `EXPORT_CAPACITY_LIMIT` | Reservation Layer (site-level) | Hard constraint, priority 0 |
| `DISPATCH_SETPOINT` | Dispatcher direct override | Bypasses planner entirely |
| `ALERT_GRID_EMERGENCY` | High-priority Reservation, all assets | Priority 0, immediate |

### 7.2 FIRM vs. FLEXIBLE distinction

OpenADR events are inherently FIRM (the VEN has accepted an obligation).
The FIRM / FLEXIBLE split in the planner refers to the *planning horizon*:

- **FIRM slots** (near horizon, ≤ N hours): committed allocations with specific assets.
- **FLEXIBLE slots** (far horizon): characterized demand windows without committed assets.

An OpenADR FIRM event always generates a FIRM-horizon reservation. It is the
planner's job to ensure FIRM slots honor all reservations; FLEXIBLE slots represent
the remaining opportunity space.

### 7.3 Report obligations from events

Events with `reportDescriptors` create `ReportObligation` records. The reporter
consults these to know what to send back:

| Obligation type | Source |
|---|---|
| `USAGE` | Asset history buffer (energy integral over interval) |
| `DEMAND` | `asset.current_state().actual_power_kw` |
| `STORAGE_CHARGE_LEVEL` | `asset.current_state()` → SoC |
| `USAGE_FORECAST` | FIRM slots: `simulate_forward(plan_setpoints)`; FLEXIBLE: range `[0, cap.max_import_kw]` |
| `IMPORT_CAPACITY_RESERVATION` | `flexibility_envelope.up_kw` (§ 8) |
| `EXPORT_CAPACITY_RESERVATION` | `flexibility_envelope.down_kw` (§ 8) |

---

## 8. User Demands, Preferences, and Leeway

### 8.1 Three distinct concerns

| Concern | Question it answers | Owner |
|---|---|---|
| **Demand** | What energy outcome does the user need? | `UserRequest` entity |
| **Preference** | Within the feasible space, what does the user prefer? | Asset profile (comfort bounds) |
| **Leeway** | How much flexibility is the user willing to donate to the grid? | `FlexibilityPolicy.default_reserve` + UserRequest tolerance fields |

These must remain separate. Conflating demand and leeway is the most common
design error: an EV that "wants to be full by 08:00" has a firm demand; how
aggressively it charges in the meantime is a leeway question.

### 8.2 User demand → UserRequest → EnergyPacket

```
POST /user-requests
  {
    asset_id:         "ev",
    mode:             "BY_DEADLINE",
    target_soc_pct:   80,
    deadline:         "2026-03-22T08:00:00Z",
    budget_eur:       2.50,             // optional ceiling
    interruptible:    true,             // leeway: planner may pause and resume
    tolerance_min:    10,               // leeway: ±10 min around deadline is acceptable
  }
```

The User Request Manager translates this into an `EnergyPacket` and emits
`PlanTrigger.USER_REQUEST`. The planner incorporates it in the next planning cycle.

`interruptible: true` and `tolerance_min` are **leeway fields** — they expand the
feasible space the planner can use. An `interruptible` packet can be paused during
an unexpected grid event without violating the user's demand.

### 8.3 Preferences → asset profile (comfort bounds)

Asset-level preferences (heater comfort band, EV minimum SoC for daily use) live
in the asset profile YAML:

```yaml
assets:
  - type: heater
    id: heater
    comfort_min_c: 19.0
    comfort_max_c: 22.0
    # planner will not schedule heater below comfort_min_c
    # but may allow it to drift within band to absorb cheap tariff slots
```

These translate to `ComfortBound` records in the reservation layer:
- `comfort_min`: floor reservation (must keep enough capacity to prevent breach)
- `comfort_max`: ceiling reservation (stop charging above this)

The reason tag `PlanReason::ComfortBound` makes these decisions visible.

### 8.4 Leeway as donated flexibility

When a user grants leeway (interruptible packet, tolerance window, soft budget),
that leeway becomes **available flexibility** the planner can donate back to grid
shaping. The `FlexibilityPolicy.default_reserve` is the global floor; leeway from
user requests can temporarily exceed it.

```
Total flexibility available to grid at time t:
    = Σ_assets [ capability(state) − reserved_FIRM − reserved_policy ]
      + Σ_packets [ leeway_kw contributed by interruptible packets ]
```

This sum is the `FlexibilityEnvelope` reported to the VTN.

---

## 9. Flexibility Envelope — First-Class Output

The flexibility envelope is not a side-effect of planning — it is the VEN's primary
product for the grid. It must be computable at any moment without running a full
planning cycle.

```rust
struct FlexibilityEnvelope {
    ts:           DateTime<Utc>,
    up_kw:        f64,   // how much the VEN can reduce consumption right now
    down_kw:      f64,   // how much the VEN can increase consumption right now
    up_duration:  Duration,   // how long it can sustain up_kw
    down_duration: Duration,
}

fn compute_envelope(
    assets: &[&dyn Asset],
    reservation_layer: &ReservationLayer,
    now: DateTime<Utc>,
) -> FlexibilityEnvelope {
    let reserved = reservation_layer.query(now);
    let total_up   = assets.iter().map(|a| {
        let state = a.current_state();
        let avail = reservation_layer.available_cap(a.id(), a.capability(&state), now);
        (state.actual_power_kw() - avail.max_export_kw).max(0.0)
    }).sum();
    let total_down = assets.iter().map(|a| {
        let state = a.current_state();
        let avail = reservation_layer.available_cap(a.id(), a.capability(&state), now);
        (avail.max_import_kw - state.actual_power_kw()).max(0.0)
    }).sum();
    // duration: simulate_free each asset and find when headroom first drops below threshold
    …
}
```

This value is:
- Reported to the VTN via `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` reports.
- Exposed via `GET /flexibility` for operator dashboards.
- Used by the planner to emit `PlanWarning::LowFlexibility` when headroom drops below threshold.

---

## 10. Further Implications & Open Questions

### 10.1 Rules as the planner's explicit policy

`rules.choose()` (§ 4.2) is the planner's decision logic. It is currently implicit
in the 8-phase greedy algorithm. Making rules an **explicit, enumerated, configurable
set** would enable:

- Operator-visible policy ("we are currently in cost-minimization mode")
- Runtime mode switching without code changes
- Testability: unit-test each rule in isolation against known states

Suggested structure:
```rust
enum PlanningMode { CostMinimization, SelfSufficiency, GridSupport, Emergency }
// Each mode = a prioritized rule set. The planner applies the active mode's rules.
```

### 10.2 Grid as a virtual asset

The grid is not controllable but participates in timelines, constraints, and reports.
It should implement `Asset` with:
- `capability()` returning `[−ExportLimit, ImportLimit]` from active VTN events
- `history()` reading `SiteMeter` (net import/export)
- `simulate_forward()` computing net = Σ other assets (derived, not physical)

This makes the grid a first-class timeline participant without special-casing.

### 10.3 Uncontrollable assets (PV, BaseLoad)

`capability()` for uncontrollable assets returns `[power_now, power_now]` — a point
range with no headroom. The planner allocates zero flexibility from them.
`simulate_free()` still provides the forecast (irradiance model, baseline profile),
which is essential for net-grid calculations.

### 10.4 Multi-asset coordination in `rules.choose()`

The current model calls `rules.choose()` per-asset independently. Some decisions are
inherently multi-asset:

- "Charge battery only if PV surplus covers the demand" requires knowing PV output at t.
- "Don't exceed site import limit" requires summing all asset setpoints.

The planning loop should pass a `SiteContext` to `rules.choose()`:
```rust
struct SiteContext {
    planned_others_kw: f64,  // sum of already-decided setpoints for other assets at t
    import_limit_kw:   f64,
    export_limit_kw:   f64,
    pv_forecast_kw:    f64,
}
```
Asset order in the planning loop then matters: PV and BaseLoad are resolved first
(uncontrollable), their output feeds into `SiteContext` for controllable assets.

### 10.5 Trajectory chaining for multi-step lookahead

The current greedy loop advances one step per asset per time slot. For assets with
strong inter-slot dependencies (battery SoC, EV deadline), a short lookahead
(e.g., 30-min forward simulation before committing a setpoint) catches constraint
violations before they happen rather than replanning reactively.

This does not change the planning model — it is a pre-commit validation pass
using `simulate_forward()` on the tentative setpoint sequence.

---

## 12. Implementation Roadmap

### 12.1 What does not need a rewrite

The physics models in `battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs` are correct
and largely stay. The planner's 8-phase scheduling logic is largely correct and stays.
What changes is how pieces are wired together — interfaces, ownership, and new abstractions
layered in. The existing 801-step BDD suite is the safety net for every phase.

### 12.2 One pre-work step before touching code

This document is sufficient for orientation. Before Phase A begins, one additional
artifact is required: a **concrete Rust interface spec** (`ven_asset_interface_spec.md`).
It locks exact type signatures, field names (with units), `AssetState` variant contents,
and which types live in which modules. Phase A touches every asset file, the simulator,
the dispatcher, and the timeline controller simultaneously; starting without locked types
means refactoring mid-refactor.
See: [ven_asset_interface_spec.md](ven_asset_interface_spec.md).

### 12.3 Phases

Phases are in strict dependency order. Each is independently deployable and
BDD-validated before the next begins.

```
Pre-work (spec) ──► Phase A ──► Phase B ──► Phase C ──► Phase D
                                       └──► Phase E ──► Phase F
```

#### Phase A — Asset trait + per-asset history buffer
**Nature:** Interface refactor (not a rewrite)
**Touches:** `assets/mod.rs`, all 5 asset files, `simulator/mod.rs`,
`controller/trace.rs`, `loops.rs`

The existing `AssetState` enum dispatch (`update()`, `forecast()`, `state_values()`) is
replaced by the `Asset` trait with `step()` and `capability()`. Physics inside each
asset file changes minimally — `update()` becomes `step()`, implicit SoC bounds become
`capability()`. Default impls provide `simulate_forward()` and `simulate_free()` for free.
The history buffer moves from `ControllerTrace.asset_history` into each `AssetEntry`
(single truth, no mirror).

**Gate:** all existing BDD scenarios green.

#### Phase B — Reservation layer
**Nature:** Extraction + new struct
**Touches:** new `controller/reservation.rs`, `controller/planner.rs`,
`controller/openadr_interface.rs`

Capacity limit checks scattered across planner phases 1–3 are extracted into an
explicit `ReservationLayer`. VTN FIRM events produce `Reservation` records.
The planner calls `reservation_layer.query(t)` per step. Behaviour is identical
initially — pure reorganisation.

**Gate:** all existing BDD scenarios green (same decisions, different plumbing).

#### Phase C — FlexibilityPolicy
**Nature:** New module (additive)
**Touches:** new `controller/flexibility_policy.rs`, YAML profile schema, `loops.rs`,
`controller/openadr_interface.rs`

Layer 1 (default reserve) and Layer 2 (scheduled windows) implemented. Pre-announced
VTN events (future `activePeriod`) handled in `openadr_interface.rs` — reservation
created at event receipt, not at activation.

**Gate:** new BDD scenarios — "policy reserve prevents opportunistic discharge before
DR window."

#### Phase D — Planner loop refactor + PlanReason
**Nature:** Significant refactor (not rewrite)
**Touches:** `controller/planner.rs`, `entities/plan.rs`

The 8-phase algorithm is restructured around the greedy-forward loop. Phase logic
does not disappear — phases 2–6 become the implementation of `rules.choose()`.
`PlanReason` added to every `PlanStep`. `LookaheadContext` (capability trajectory,
tariff minimum ahead) enriches the rules.

**Gate:** existing BDD scenarios green + new scenarios asserting `PlanReason` values.

#### Phase E — Flexibility envelope as first-class output
**Nature:** Additive
**Touches:** `controller/planner.rs`, `routes/hems.rs`, `controller/reporter.rs`

`compute_envelope()` becomes a standalone function, not a plan side-effect.
`GET /flexibility` returns it continuously. Reporter uses it for
`IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` report obligations.

**Gate:** new BDD scenario — "envelope reflects reserved capacity, not physical max."

#### Phase F — User leeway expansion
**Nature:** Small extension
**Touches:** `entities/user_request.rs`, `controller/user_request.rs`, `routes/hems.rs`

`tolerance_min` and `budget_eur` added to `UserRequest`. Interruptible packets
contribute their headroom back to the flexibility envelope.

**Gate:** new BDD scenarios for tolerance and budget constraints.

### 12.4 Phase summary

| Phase | Nature | Risk | Prerequisite |
|---|---|---|---|
| Pre-work: interface spec | Design (half-day) | — | This document |
| A: Asset trait | Interface refactor | Medium — wide blast radius | Spec |
| B: Reservation layer | Extraction | Low — same behaviour | A |
| C: FlexibilityPolicy | New module | Low — additive | B |
| D: Planner refactor | Significant refactor | Medium — logic reorganised | B |
| E: Flexibility envelope | Additive | Low | B |
| F: User leeway | Small extension | Low | D |

---

## 11. Relation to Current Implementation

| Architectural element | Current state | Target |
|---|---|---|
| Asset trait | Partial (`forecast()`, `history()` on `AssetState` enum dispatch) | Full `Asset` trait with `step()`, `capability()`, `simulate_forward()`, `simulate_free()` |
| History buffer ownership | Central `ControllerTrace.asset_history` | Per-asset ring buffer; central trace becomes query aggregator only |
| Planner model | 8-phase greedy with phases mixing physics and policy | Greedy forward step, pure loop, reservation layer separates obligation types |
| Reservation layer | Implicit in planner phases | Explicit `ReservationLayer` struct, queried per step |
| FlexibilityPolicy | Not present | Explicit module, produces `Reservation` records |
| Pre-announced events | Not handled | OpenADR interface creates reservations at event receipt |
| Flexibility envelope | Computed as plan side-effect | First-class `compute_envelope()`, always queryable |
| PlanReason | Partial (PlanWarnings) | Full `PlanReason` on every `PlanStep` |
| User leeway fields | `interruptible` only | `tolerance_min`, `budget_eur`, `interruptible` all mapped to leeway in reservation layer |
