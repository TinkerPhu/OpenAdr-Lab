# VEN Backend — Refactoring Backlog

> Code quality review conducted 2026-04-28.
> Scope: `VEN/src/` (Rust backend). UI and BFF not yet reviewed.
>
> Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)

---

## Summary Map

```
VEN BACKEND — FRAGMENTATION MAP
════════════════════════════════════════════════════════════════

  profile.rs (26KB) ◄──── REAL
  controller/
    profile.rs (22KB) ◄─── DEAD (not in mod.rs, never compiled)
         │
         └── drift accumulating silently

  ────────────────────────────────────────────────────────────

  Profile.devices (legacy)     ←──────────────────────────────
  Profile.assets  (current)    ←── double-dispatch in 5 accessors

  ────────────────────────────────────────────────────────────

  AssetCapability (trait, 2 fields)
  AssetCapabilities (legacy, 6 fields) ← lives only for /capability API

  ────────────────────────────────────────────────────────────

  AssetConfig enum
  ┌──────────────────────────────────────────────────────┐
  │  ~15 methods × 5 variants = ~75 manual match arms    │
  │  Every new asset type: +15 match blocks              │
  │  Every new method: +5 match arms                     │
  └──────────────────────────────────────────────────────┘

  ────────────────────────────────────────────────────────────

  "battery","heater","ev","pv","boiler"
  ↑ string literals in 5+ files, no shared constants

  ────────────────────────────────────────────────────────────

  InnerState { 20+ fields across 3 concerns }
  └── AppState: ~30 async accessors (each = lock + clone)

  ────────────────────────────────────────────────────────────

  loops.rs = 48KB god function (13 phases inline)
```

---

## Priority Table

| # | Issue | Priority | Effort | Risk |
|---|-------|----------|--------|------|
| R-01 | Delete dead `controller/profile.rs` | 🔴 | Trivial | Zero |
| R-02 | Remove legacy `DeviceConfig` / `devices` fallback | 🔴 | Small | Need to audit YAML files |
| R-03 | Replace hardcoded string asset IDs with constants | 🟠 | Small | Mechanical |
| R-04 | Remove legacy `AssetCapabilities` struct | 🟠 | Medium | API shape change on `/capability` |
| R-05 | Split `spawn_sim_tick` god function in `loops.rs` | 🟠 | Medium | No behaviour change |
| R-06 | Split `InnerState` into domain sub-structs | 🟡 | Medium | Locking strategy change |
| R-07 | Remove `cancel_request` legacy fallback branch | 🟡 | Trivial | Harmless after a restart |
| R-08 | `AssetConfig` → `dyn Asset` dispatch via trait objects | 🔵 | Large | Correctness risk, deferred |

---

## Detailed Findings

---

### R-01 — Dead phantom file: `controller/profile.rs` 🔴

**Files:** `VEN/src/controller/profile.rs`, `VEN/src/controller/mod.rs`

`VEN/src/controller/profile.rs` (22 KB) is never compiled. `controller/mod.rs` does not
declare `pub mod profile`, so the Rust compiler never sees it. All imports across the codebase
use `crate::profile::*` (the real `VEN/src/profile.rs`, 26 KB).

The file appears to be a stale copy that diverged during the Phase A/B refactor — it still
contains slightly different content (missing `switching_penalty_eur` on `HeaterConfig`, among
other small differences), which means it will accumulate silent drift indefinitely.

```
VEN/src/profile.rs              ← 26 KB, used by everything
VEN/src/controller/profile.rs   ← 22 KB, NOT in controller/mod.rs, dead code
```

**Fix:** Delete `VEN/src/controller/profile.rs`.

---

### R-02 — Stalled migration: dual config format in `Profile` 🔴

**File:** `VEN/src/profile.rs`

`Profile` carries two parallel representations of the same YAML concept:

```rust
pub struct Profile {
    pub devices: DeviceConfig,       // OLD: named fields (ev, heater, pv, battery)
    pub assets: Vec<AssetProfile>,   // NEW: typed list with `type:` discriminator
    ...
}
```

Every config accessor does a double-check fallback:

```rust
/// Returns the EV config: checks `assets` list first, falls back to legacy `devices`.
pub fn ev_config(&self) -> Option<&EvConfig> {
    self.assets.iter().find_map(|a| if let AssetProfile::Ev(c) = a { Some(c) } else { None })
        .or_else(|| self.devices.ev.as_ref())
}
```

This pattern is repeated for 5 asset types. The `DeviceConfig` struct, its 5 optional fields,
and the 5 `default_*` free functions exist solely for backward compatibility.

**Prerequisite check:** Verify all three `VEN/profiles/ven-N.yaml` files and
`VEN/profiles/test.yaml` use the new `assets:` list format before deleting the legacy path.

**Fix:** Once confirmed, delete `DeviceConfig`, the `devices` field in `Profile`, and simplify
the 5 accessor methods to iterate only `self.assets`.

---

### R-03 — Hardcoded string asset IDs 🟠

**Files:** `loops.rs`, `controller/dispatcher.rs`, `controller/timeline.rs`,
`controller/milp_planner.rs`, `routes/hems.rs`

Asset IDs are string literals scattered across the codebase with no shared authoritative source:

| Literal | Files |
|---------|-------|
| `"battery"` | `loops.rs:433`, `dispatcher.rs:52,180`, `milp_planner.rs` (assertions) |
| `"heater"` | `dispatcher.rs:56,72`, `hems.rs:275` |
| `"ev"` | `dispatcher.rs:59`, `hems.rs:250`, `milp_planner.rs` (assertions) |
| `"pv"` | `timeline.rs:352` |
| `"boiler"` | `hems.rs:275` only — not present elsewhere |

`"boiler"` is particularly concerning: it appears in one route handler as an alias for
`"heater"` but has no corresponding entry in any other file. If a VEN YAML sets `id: boiler`
for a heater asset, the route will match but the dispatcher will not.

**Fix:** Define constants in a shared module (e.g. `VEN/src/assets/ids.rs`):

```rust
pub mod ids {
    pub const BATTERY:   &str = "battery";
    pub const EV:        &str = "ev";
    pub const HEATER:    &str = "heater";
    pub const PV:        &str = "pv";
    pub const BASE_LOAD: &str = "base_load";
}
```

Replace all literal occurrences. Decide explicitly whether `"boiler"` is a supported alias
or a typo.

---

### R-04 — Stalled migration: `AssetCapabilities` (legacy) vs `AssetCapability` (new) 🟠

**File:** `VEN/src/assets/mod.rs` and every asset file

Two parallel capability types coexist:

```rust
/// Planning capability descriptor (legacy — kept for planner compat)
pub struct AssetCapabilities {
    pub asset_id: String,
    pub max_import_kw: f64,
    pub max_export_kw: f64,
    pub is_flexible: bool,
    pub energy_state: Option<EnergyState>,
    pub availability: Option<TimeWindow>,
}

/// Point-in-time feasible power range (new, used by Asset trait)
pub struct AssetCapability {
    pub max_export_kw: f64,
    pub max_import_kw: f64,
}
```

Every asset type (`Battery`, `Ev`, `Heater`, `Pv`, `BaseLoad`) still implements both
`capability()` (new) and `capabilities()` (legacy). The legacy version is only called from
the HTTP `GET /capability` route to produce the API response shape.

The "planner compat" comment in `mod.rs` is stale — the planner uses `AssetConfig::capability()`
(new) exclusively.

**Fix:** Update `routes/assets.rs` to construct the API response from `AssetCapability` directly
(it only uses `max_import_kw` and `max_export_kw` anyway). Then delete `AssetCapabilities`,
`EnergyState`, `TimeWindow`, and the 5 `capabilities()` method implementations.

---

### R-05 — `loops.rs` god function — `spawn_sim_tick` 🟠

**File:** `VEN/src/loops.rs` (48 KB)

The sim tick loop is a single async function handling at least 13 sequential phases inline:

1. Lock `sim` mutex + snapshot capacity / inject state
2. Compute setpoints from plan (dispatcher)
3. Apply battery correction overlay (Plan F)
4. Apply correction hold (Plan G)
5. Emit `CorrectionActive`/`CorrectionCleared` SSE events
6. Run simulator `tick()`
7. Inject shiftable load runtimes into sim snapshot
8. Update `AppState` sim snapshot
9. Post-tick ledger accounting (`monitor::record_tick`)
10. Push `HistoryPoint` into per-asset ring buffers
11. Update Grid virtual asset with net power + VTN limits
12. Refresh site envelope
13. Detect plan trigger conditions + emit planner events

None of these phases can be unit-tested in isolation without running the full tick.

**Fix:** Extract each logical phase into a named `fn` (no async needed for most):

```
tick_compute_setpoints(sim, plan, capacity, ...) -> HashMap<String, f64>
tick_apply_correction(setpoints, sim, plan, ...) -> (f64, bool)
tick_run_sim(sim, setpoints, inject, dt) -> SimSnapshot
tick_update_history(sim) // mutates in place
tick_update_ledger(ledger, snap, rates, dt)
tick_refresh_envelope(sim, now) -> SiteFlexibilityEnvelope
```

This keeps `spawn_sim_tick` as the orchestrator but makes each phase independently testable.

---

### R-06 — `InnerState` god struct + accessor proliferation 🟡

**File:** `VEN/src/state.rs` (21 KB)

`InnerState` has grown to 20+ fields spanning three unrelated concerns. All are locked together
under one `Arc<RwLock<InnerState>>`, meaning reading `programs` acquires the same lock as
writing `ev_session`:

```rust
pub struct InnerState {
    // ── OpenADR polling (persisted) ─────────────────────
    programs: Vec<serde_json::Value>,
    events:   Vec<serde_json::Value>,
    reports:  Vec<serde_json::Value>,

    // ── Physics sim state (partly persisted) ─────────────
    sensor:       SensorSnapshot,
    sim:          Option<SimSnapshot>,
    inject_state: SimInjectState,
    controller_trace: ControllerTrace,

    // ── HEMS controller state (all #[serde(skip)]) ───────
    active_plan:        Option<Plan>,
    planned_tariffs:    Vec<TariffSnapshot>,
    capacity_state:     OadrCapacityState,
    report_obligations: Vec<OadrReportObligation>,
    asset_ledger:       HashMap<String, AssetLedgerEntry>,
    active_requests:    Vec<UserRequest>,
    site_envelope:      Option<SiteFlexibilityEnvelope>,
    ev_session:         Option<EvSession>,
    heater_target:      Option<HeaterTarget>,
    shiftable_loads:    Vec<ShiftableLoad>,
    shiftable_runtimes: Vec<ShiftableLoadRuntime>,
    baseline_override:  Option<BaselineOverride>,
    ev_settings:        EvSettings,
}
```

`AppState` then wraps this with ~30 async accessor methods, each being a trivial
`read().await.field.clone()` or `write().await.field = value` — mechanical boilerplate that
provides no logic.

**Fix (incremental):** Split into two or three domain sub-structs, each with its own
`Arc<RwLock<...>>`. A sensible first cut:

```
PollingState { programs, events, reports }         ← written by poll loops, read by routes
HemsState    { active_plan, tariffs, sessions, ... } ← written by controller, read by routes
SimState     { sensor, sim, inject_state, ... }    ← written by sim tick, read by routes
```

The coarse lock is fine for now but becomes a bottleneck if the sim tick and HTTP handlers
start contending (the tick loop holds the write lock for all 13 phases).

---

### R-07 — `cancel_request` legacy fallback branch 🟡

**File:** `VEN/src/state.rs`

`cancel_request` contains a legacy fallback for `UserRequest` records that predate the
`session_type: Option<SessionType>` field added in Plan C:

```rust
None => {
    // Legacy path: match session_id against ev/heater for requests
    // created before Plan C added session_type.
    if let Some(sid) = session_id {
        if inner.ev_session.as_ref().map(|s| s.id) == Some(sid) {
            inner.ev_session = None;
        } else if inner.heater_target.as_ref().map(|t| t.id) == Some(sid) {
            inner.heater_target = None;
        }
    }
}
```

Since `active_requests` is `#[serde(skip)]` — not persisted to disk — old requests without
`session_type` only exist within a single VEN uptime. After a restart, all new requests will
have the field. The legacy branch is already dead after one clean restart.

**Fix:** Delete the `None =>` arm. Add a `warn!()` log for `session_type: None` to detect
any unexpected case during the transition period.

---

### R-08 — `AssetConfig` dispatch explosion 🔵 *(deferred)*

**File:** `VEN/src/assets/mod.rs`

`AssetConfig` is a manual dispatch enum with ~15 methods, each a full `match` over 5 variants
= ~75 match arms. Every new asset type requires 15 new match arms. Every new method requires
5 new match arms.

```rust
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}

// × 15 methods, each:
pub fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
    match self {
        Self::Battery(cfg) => cfg.step(state, setpoint_kw, dt),
        Self::Ev(cfg)      => cfg.step(state, setpoint_kw, dt),
        ...
    }
}
```

The `Asset` trait exists and is implemented by each physics type, but `AssetConfig` bypasses
it with manual dispatch instead of `Box<dyn Asset>` or a macro-generated forwarder.

**Why deferred:** Switching to `dyn Asset` changes object layout, potentially impacts
serialisation (`AssetConfig` derives `Serialize`/`Deserialize`), and requires threading
lifetime/ownership concerns through `SimState`. High correctness risk for an incremental gain.

A lighter alternative: a `delegate_asset!` macro that generates all match arms from a single
declaration:

```rust
delegate_asset! {
    impl AssetConfig {
        fn step(state, setpoint_kw, dt) -> (AssetState, f64);
        fn capability(state) -> AssetCapability;
        ...
    }
}
```

This eliminates the repetition without changing the dispatch model.

---

## Notes

- `AssetProfile` (YAML deserialized, in `profile.rs`) and `AssetConfig` (runtime physics,
  in `assets/mod.rs`) share the same variant names (`Ev`, `Battery`, etc.) but hold different
  inner types. The naming was chosen to avoid a collision introduced during Phase A. This is
  documented but can still confuse newcomers. Consider renaming `AssetProfile` →
  `AssetProfileYaml` or `AssetSpec` to make the distinction explicit.

- `SimInjectState` mixes three injection behaviours (A = one-shot, B = frozen+EMA, C = frozen+snap)
  in a single flat struct. The clearing/decay logic for each behaviour is scattered across
  `state.rs` (`clear_inject_field`) and `simulator/mod.rs` (tick). A small `InjectBehaviour`
  tagged enum per field would make the intent self-documenting.





- Implement snapshot-and-release pattern for sim mutex now (may change timing)