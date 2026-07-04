# Research: 016 — Refactor VEN Backend

All NEEDS CLARIFICATION items were resolved during the `speckit.specify` verification pass and the `speckit.clarify` session. No unknowns remain. This document records each architectural decision so tasks.md can reference rationale directly.

---

## D-01 — `ids` module placement

**Decision**: New top-level file `VEN/src/ids.rs`, registered in `VEN/src/main.rs` as `mod ids;`

**Rationale**: A small, single-purpose module for public string constants is the idiomatic Rust pattern. It keeps `common/mod.rs` (28 KB, focused on utilities) from accumulating unrelated material. Every site that currently embeds a magic literal can add a one-line `use crate::ids;` import.

**Constants required** (from code survey):

```rust
pub const EV: &str       = "ev";
pub const BATTERY: &str  = "battery";
pub const PV: &str       = "pv";
pub const HEATER: &str   = "heater";
pub const BOILER: &str   = "boiler";
pub const BASE_LOAD: &str = "base_load";
```

**Alternatives considered**:
- Add to `common/mod.rs` — rejected: conflates string constants with behavioural utilities
- Private `const` in each call-site file — rejected: recreates the magic-literal duplication risk

---

## D-02 — `DeviceConfig` removal: accessor simplification

**Decision**: Remove the `.or(self.devices.xxx.as_ref())` fallback arm from each of the 5 `Profile` accessors. The primary iterator over `self.assets` is kept unchanged. For `base_load_kw()`, the fallback `unwrap_or(self.devices.base_load_w / 1000.0)` becomes `unwrap_or(default_base_load_kw())` (0.5 kW, matching the existing default fn).

**Rationale**: The startup guard in `Profile::try_load()` (D-04) ensures `assets` is never empty when a YAML file is loaded. The fallback is dead-by-invariant for all production paths.

**Impact on `Profile::default()`**: `default()` still returns `assets: vec![]` (used as in-process fallback when `PROFILE_PATH` is unset and in unit tests). It is **not** affected by the startup guard (which lives in `try_load()`). Callers of `profile.ev_config()` on a `default()` profile correctly receive `None`, unchanged.

**Removal checklist**:
- `devices: DeviceConfig` field on `Profile` struct
- `DeviceConfig` struct, `default_base_load()` fn, and all its sub-types (`EvConfig`, etc. stay — they are referenced directly by `AssetProfile::Ev`)
- Wait: `EvConfig`, `HeaterConfig`, `PvConfig`, `BatteryConfig` are **also** the types inside `AssetProfile` variants — they must be **kept**. Only the `DeviceConfig` wrapper struct and its `ev/heater/pv/battery/base_load_w` fields are deleted.
- The 5 `.or(...)` fallback arms in accessor methods

---

## D-03 — `InnerState` 3-way split with `serde(flatten)`

**Decision**: Nest 3 sub-structs inside `InnerState` using `#[serde(flatten)]`. The sub-struct names:

| Sub-struct name | Fields | Serde treatment |
|---|---|---|
| `PollingState` | `programs`, `events`, `reports` | derived Serialize + Deserialize (persisted) |
| `ControllerSimState` | `sensor`, `sim`, `inject_state`, `controller_trace` | `sensor` serialised; rest `#[serde(skip)]` |
| `HemsState` | all 13 HEMS fields | all `#[serde(skip)]` |

Name rationale for `ControllerSimState`: the existing `simulator::SimState` type (physics) is also in scope in `state.rs`'s crate. Using a distinct name avoids ambiguity for readers.

**`serde(flatten)` preserves JSON shape**: The existing `state.json` format has `programs`, `events`, `reports`, and `sensor` at the top level. `#[serde(flatten)]` on each sub-struct keeps these keys at the same JSON level — no migration, no backwards-compat break.

**`InnerState`'s manual `Clone` impl** can be replaced with `#[derive(Clone)]` after the split, since all 3 sub-structs can derive `Clone`.

**Lock structure unchanged**: `AppState` still holds a single `Arc<RwLock<InnerState>>`. The sub-struct split is organisational only, not a lock decomposition.

**Accessor update pattern**: Every `AppState` accessor currently reads/writes `inner.field`. After the split it reads/writes `inner.polling.field`, `inner.ctrl_sim.field`, or `inner.hems.field`. The accessor method signatures (return type, async) are unchanged.

---

## D-04 — Startup guard for empty `assets`

**Decision**: Add post-parse validation in `Profile::try_load()` immediately after `serde_yaml::from_str`:

```rust
if profile.assets.is_empty() {
    anyhow::bail!(
        "Profile has no assets — check for legacy 'devices:' key (got {} bytes)",
        contents.len()
    );
}
```

`Profile::load()` (the public wrapper) already demotes errors from `try_load()` to a `warn!` + `default()` fallback. After this change, a legacy YAML silently becomes the default profile — but emits a loud warning with the error text. To make it a hard failure, `main.rs` would need to call `try_load()` directly. Given the spec chose "fail loudly" (FR-009), the `main.rs` call site should be changed to call `try_load()` directly (or re-check the result).

**Revised approach** (to satisfy FR-009 — refuse startup):
- Change `main.rs` to call `Profile::try_load()` directly and propagate the error:

```rust
let profile = if let Some(ref path) = cfg.profile_path {
    Profile::try_load(path).await?   // propagates → main() returns Err → process exits
} else {
    warn!("PROFILE_PATH not set, using default profile");
    Profile::default()
};
```

This makes `Profile::load()` (the soft fallback) unused for production paths; it can remain for tests that want the tolerant behaviour.

---

## D-05 — `controller/profile.rs` deletion (R-01)

**Decision**: Delete the file. It is confirmed absent from `controller/mod.rs` (`mod profile;` statement not present). The 22 KB file is unreachable dead code.

**Verification**: `grep -rn "controller::profile\|controller/profile" VEN/src/` returns no hits.

**Procedure**: `git rm VEN/src/controller/profile.rs` — the compiler will confirm nothing references it.

---

## D-06 — `AssetCapabilities` dead-code removal (R-04)

**Decision**: Delete the following from `VEN/src/assets/mod.rs`:
- `struct AssetCapabilities` and its `impl`
- `struct EnergyState` (used only by `AssetCapabilities`)
- `struct TimeWindow` (used only by `AssetCapabilities`)
- `fn capabilities(&self) -> AssetCapabilities` — 5 implementations, one on each `AssetConfig` variant

**Verification**: `routes/assets.rs:74` already calls `cfg.capability(&entry.state)` (new API returning `AssetCapability`). `AssetConfig::capabilities()` (plural) is only self-referenced within `assets/mod.rs`. No route, no controller, no test calls it.

---

## D-07 — Legacy `cancel_request` `None =>` branch (R-07)

**Decision**: Remove the `None =>` arm from the `match session_type` block in `AppState::cancel_request`. The `None` variant was the pre-`SessionType` fallback path; every `UserRequest` now carries a `SessionType`.

**Verification**: `UserRequest` struct has `session_type: SessionType` (non-Option); the `None` branch is structurally unreachable.

---

## D-08 — `"boiler"`/`"heater"` constant substitution (R-03, R-08)

**Decision**: Replace the two string literals in `hems.rs:275` with `crate::ids::HEATER` and `crate::ids::BOILER`. The dual-match logic is kept as-is (both asset types share the HEMS session path). Add a `// TODO(boiler-physics):` comment explaining the deferral.

**Constants also used in other files**: Callers of `profile.heater_config()` use the `"heater"` id indirectly (it is the default id in `HeaterConfig`). The `ids` constants are for places where the type discriminant string is written inline (match arms, `find_asset()` calls, etc.). Audit: `grep -rn '"heater"\|"boiler"\|"ev"\|"battery"\|"pv"\|"base_load"' VEN/src/` — replace all non-YAML, non-test inline string matches with `ids::*` constants.
