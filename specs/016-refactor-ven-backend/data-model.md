# Data Model: 016 — Refactor VEN Backend

All changes are structural reorganisations of existing types. No new data is introduced and no persistence format changes.

---

## 1. `VEN/src/ids.rs` — New module

```rust
//! Canonical asset-identifier string constants.
//! Use these instead of inline literals when matching or looking up assets by id.
pub const EV: &str        = "ev";
pub const BATTERY: &str   = "battery";
pub const PV: &str        = "pv";
pub const HEATER: &str    = "heater";
pub const BOILER: &str    = "boiler";
pub const BASE_LOAD: &str = "base_load";
```

**Registration**: `mod ids;` added to `VEN/src/main.rs`.

---

## 2. `VEN/src/profile.rs` — Simplified `Profile`

### Before

```rust
pub struct Profile {
    #[serde(default)] pub devices: DeviceConfig,  // ← REMOVED
    #[serde(default)] pub assets: Vec<AssetProfile>,
    // ...unchanged fields...
}

pub struct DeviceConfig {                          // ← ENTIRE STRUCT REMOVED
    pub ev: Option<EvConfig>,
    pub heater: Option<HeaterConfig>,
    pub pv: Option<PvConfig>,
    pub battery: Option<BatteryConfig>,
    #[serde(default = "default_base_load")]
    pub base_load_w: f64,
}
```

### After

```rust
pub struct Profile {
    // devices field removed entirely
    #[serde(default)] pub assets: Vec<AssetProfile>,
    #[serde(default)] pub simulator: SimulatorConfig,
    #[serde(default)] pub planner: PlannerConfig,
    #[serde(default)] pub grid: GridConfig,
    #[serde(default)] pub packets: Vec<PacketSeed>,
}
```

`EvConfig`, `HeaterConfig`, `PvConfig`, `BatteryConfig` are **kept** — they are the payload types inside `AssetProfile` variants.

### Accessor changes (all 5 methods)

| Method | Before (simplified) | After |
|--------|--------------------|----|
| `ev_config()` | `find_in_assets().or(devices.ev.as_ref())` | `find_in_assets()` |
| `heater_config()` | `find_in_assets().or(devices.heater.as_ref())` | `find_in_assets()` |
| `pv_config()` | `find_in_assets().or(devices.pv.as_ref())` | `find_in_assets()` |
| `battery_config()` | `find_in_assets().or(devices.battery.as_ref())` | `find_in_assets()` |
| `base_load_kw()` | `find_in_assets().unwrap_or(devices.base_load_w / 1000.0)` | `find_in_assets().unwrap_or(default_base_load_kw())` |

### Startup guard in `try_load()`

```rust
async fn try_load(path: &str) -> anyhow::Result<Self> {
    let contents = tokio::fs::read_to_string(Path::new(path)).await?;
    let profile: Profile = serde_yaml::from_str(&contents)?;
    if profile.assets.is_empty() {
        anyhow::bail!(
            "Profile has no assets — check for legacy 'devices:' key (got {} bytes)",
            contents.len()
        );
    }
    Ok(profile)
}
```

`main.rs` calls `try_load()` directly and propagates with `?`:

```rust
let profile = if let Some(ref path) = cfg.profile_path {
    Profile::try_load(path).await?
} else {
    warn!("PROFILE_PATH not set, using default profile");
    Profile::default()
};
```

---

## 3. `VEN/src/assets/mod.rs` — Dead code removed

The following are deleted:

| Item | Reason |
|------|--------|
| `struct AssetCapabilities` | Dead — `GET /capability` uses `AssetCapability` (singular) |
| `struct EnergyState` | Only used by `AssetCapabilities` |
| `struct TimeWindow` | Only used by `AssetCapabilities` |
| `fn capabilities(&self) -> AssetCapabilities` × 5 impls | Dead — no callers outside this file |

No remaining types or methods are changed.

---

## 4. `VEN/src/state.rs` — `InnerState` 3-way split

> ⚠️ **Corrected approach** (supersedes earlier draft): FR-013 requires **three separate `Arc<RwLock<T>>`** — not a single `InnerState` with `serde(flatten)`. `AppState` is restructured to hold three independent locks; serialisation uses a private `PersistedVenState` helper.

### `AppState` after split

```rust
pub struct AppState {
    pub polling:  Arc<RwLock<PollingState>>,
    pub ctrl_sim: Arc<RwLock<ControllerSimState>>,
    pub hems:     Arc<RwLock<HemsState>>,
}

// INVARIANT: No function may acquire more than one lock simultaneously.
// Always snapshot-and-release: acquire → clone needed fields → drop guard → work on snapshot.
// No guard may cross an .await point or a second lock acquisition.
```

`InnerState` is deleted entirely; `AppState::new()` initialises three `Arc::new(RwLock::new(T::default()))` fields.

### New sub-structs

```rust
/// Polling loop state — programs/events/reports persisted to state.json.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub programs: Vec<serde_json::Value>,
    pub events:   Vec<serde_json::Value>,
    pub reports:  Vec<serde_json::Value>,
}

/// Controller-side sim state — sensor snapshot and override handles.
/// `sensor` is persisted; other fields are runtime-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerSimState {
    pub sensor: SensorSnapshot,
    #[serde(skip)] pub sim:              Option<SimSnapshot>,
    #[serde(skip)] pub inject_state:     SimInjectState,
    #[serde(skip)] pub controller_trace: ControllerTrace,
}

impl Default for ControllerSimState { /* initialise with SensorSnapshot::empty_now() */ }

/// HEMS orchestration state — all runtime, none persisted.
#[derive(Debug, Clone, Default)]
pub struct HemsState {
    pub active_plan:          Option<Plan>,
    pub planned_tariffs:      Vec<TariffSnapshot>,
    pub capacity_state:       OadrCapacityState,
    pub report_obligations:   Vec<OadrReportObligation>,
    pub asset_ledger:         HashMap<String, AssetLedgerEntry>,
    pub active_requests:      Vec<UserRequest>,
    pub site_envelope:        Option<SiteFlexibilityEnvelope>,
    pub ev_session:           Option<EvSession>,
    pub heater_target:        Option<HeaterTarget>,
    pub shiftable_loads:      Vec<ShiftableLoad>,
    pub shiftable_runtimes:   Vec<ShiftableLoadRuntime>,
    pub baseline_override:    Option<BaselineOverride>,
    pub ev_settings:          EvSettings,
}
```

### Persistence via `PersistedVenState`

`state.json` keeps the same flat JSON structure (`programs`, `events`, `reports`, `sensor` as top-level keys). A private helper assembles and disassembles from the three locks:

```rust
#[derive(Serialize, Deserialize)]
struct PersistedVenState {
    programs: Vec<serde_json::Value>,
    events:   Vec<serde_json::Value>,
    reports:  Vec<serde_json::Value>,
    sensor:   SensorSnapshot,
}

// to_json: acquire polling.read + ctrl_sim.read separately → assemble → serialize
// load_from_json: deserialize → acquire polling.write + ctrl_sim.write separately → distribute
```

`HemsState` has no persisted fields (`#[derive(Default)]` is sufficient for loading).

### Accessor update convention

All `AppState` accessor methods acquire the appropriate sub-lock:

| Old path | New lock | New path |
|----------|----------|----------|
| `inner.programs` | `self.polling.read()` | `polling.programs` |
| `inner.events` | `self.polling.read()` | `polling.events` |
| `inner.reports` | `self.polling.read()` | `polling.reports` |
| `inner.sensor` | `self.ctrl_sim.read()` | `ctrl_sim.sensor` |
| `inner.sim` | `self.ctrl_sim.read()` | `ctrl_sim.sim` |
| `inner.inject_state` | `self.ctrl_sim.write()` | `ctrl_sim.inject_state` |
| `inner.controller_trace` | `self.ctrl_sim.write()` | `ctrl_sim.controller_trace` |
| `inner.active_plan` | `self.hems.write()` | `hems.active_plan` |
| … (all 13 HEMS fields) | `self.hems.read/write()` | `hems.*` |

Accessor **signatures** (return type, `async`, visibility) are unchanged — callers see no difference.

### Legacy `cancel_request` branch removed (R-07)

`session_type: Option<SessionType>` (confirmed in `VEN/src/entities/user_request.rs`). The old `None =>` arm silently retains the request without clearing any state; it is removed and replaced with a `warn!()`.

```rust
// BEFORE — None arm silently no-ops (dead after Plan C)
match session_type {
    Some(SessionType::Ev)            => { /* clear ev_session */ }
    Some(SessionType::Heater)        => { /* clear heater_target */ }
    Some(SessionType::ShiftableLoad) => { /* remove load + runtime */ }
    None => { state.active_requests.retain(|r| r.id != request_id) }  // ← REMOVED
}

// AFTER — None produces a warning; requests are always marked cancelled
match session_type {
    Some(SessionType::Ev)            => { /* clear ev_session */ }
    Some(SessionType::Heater)        => { /* clear heater_target */ }
    Some(SessionType::ShiftableLoad) => { /* remove load + runtime */ }
    None => {
        tracing::warn!("cancel_request: unexpected session_type: None for request {}", request_id);
        // request is still marked cancelled via active_requests.retain below
    }
}
```

---

## 5. `VEN/src/loops.rs` — New helper types for US6 phase extraction

Three small stack-local types introduced by the phase extraction in US6. All are defined in `VEN/src/loops.rs` (or a `loops/` submodule if file grows large).

### `ClearedInjectField`

Returned by `apply_sim_injections` to communicate which fields were consumed and should be cleared on `AppState`:

```rust
#[derive(Debug)]
pub(crate) enum ClearedInjectField {
    PowerKw,
    SocPercent,
    TempC,
    // extend as new inject fields are added
}
```

### `Setpoints`

Output of `build_setpoints`; holds computed target values for this tick. All fields are `Option` — `None` means "no instruction for this asset this tick":

```rust
#[derive(Debug, Default)]
pub(crate) struct Setpoints {
    pub ev_charge_kw:      Option<f64>,
    pub heater_target_c:   Option<f64>,
    pub battery_charge_kw: Option<f64>,
    // extend per asset type
}
```

### `DeviationState`

Mutable counter bundle passed into `apply_deviation_correction`; holds the Plan F/G state machine counters for the current tick. Stack-allocated, not persisted:

```rust
#[derive(Debug, Default)]
pub(crate) struct DeviationState {
    pub deviation_ticks:       u32,
    pub correction_is_active:  bool,
    pub prev_correction_kw:    f64,
}
```

These types are **local to `loops.rs`** and not exposed via `AppState` or HTTP routes.

---

## 6. `VEN/src/routes/hems.rs` — Constant substitution (R-03, R-08)

Line ~275 before:

```rust
if asset_id == "heater" || asset_id == "boiler" {
```

After:

```rust
// TODO(boiler-physics): boiler and heater share the HEMS session path for now.
// Full boiler physics (200L DHW tank, heuristic forecast) deferred to a future feature.
if asset_id == crate::ids::HEATER || asset_id == crate::ids::BOILER {
```

Additional audit: all other inline `"heater"`, `"boiler"`, `"ev"`, `"battery"`, `"pv"`, `"base_load"` asset-id literals in `VEN/src/` (excluding YAML profiles and test fixtures) are replaced with the corresponding `crate::ids::*` constant.

---

## 7. `VEN/src/controller/profile.rs` — Deleted

File deleted via `git rm`. The `controller/mod.rs` does not contain `mod profile;` — the file is unreachable. No source changes required.
