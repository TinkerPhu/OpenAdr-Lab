# Speckit Call 1a — Asset Request Target Resolution

**Speckit feature name**: `ven-asset-request-target`
**Depends on**: `ven-simulator-reform` (call 1) must be complete — needs `Vec<AssetEntry>` and `AssetState` enum
**Must be complete before**: spec_kit_call_2 (controller reform uses `user_request.rs` with `Vec<AssetEntry>`)

## How to invoke

```
/speckit.specify <paste the Feature Description section below>
```

When prompted for a feature path / name, use: `ven-asset-request-target`

---

## Feature Description

Remove the hardcoded asset-type switch from `controller/user_request.rs` by moving per-asset request target resolution into the `AssetState` enum. This is a small focused refactor — no API change, no behavior change, no UI change.

### Context

`controller/user_request.rs: resolve_target()` contains:

```rust
match body.asset_id.as_str() {
    "ev" => { /* reads ev_cfg from profile, computes delta_soc * battery_kwh */ }
    "battery" => { /* reads bat from profile, computes delta_soc * capacity_kwh */ }
    other => Err(RequestError::UnknownAsset(other.to_string())),
}
```

This is the same hardcoded-per-asset-type pattern that speckit 1 eliminated everywhere else. Adding a new energy-storage asset type requires editing `user_request.rs`. Additionally, `user_request.rs` imports `Profile` and calls `profile.ev_config()` / `profile.battery_config()` — named methods that tie it to the old hardcoded profile structure. After speckit 1, each asset owns its own config inside `AssetEntry.state`, so the profile import is redundant.

### Change

#### 1. Add `resolve_request_target` to `AssetState`

In `VEN/src/simulator/assets/mod.rs`, add one method to the `AssetState` enum interface:

```rust
fn resolve_request_target(
    &self,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
    current_values: &HashMap<String, f64>,  // from AssetEntry state_values()
) -> Option<(f64, f64)>  // (target_energy_kwh, desired_power_kw); None = not requestable
```

Per-type implementations:
- **`EvCharger`** (`ev.rs`): `delta = (target_soc.unwrap_or(config.soc_target) - current_soc).max(0.0)`, `kwh = delta * config.battery_kwh`, `power = desired_power_kw.unwrap_or(config.max_charge_kw)` — returns `Some((kwh, power))` if `kwh > 1e-6`, else `None`
- **`Battery`** (`battery.rs`): same pattern with `config.capacity_kwh` and `config.max_charge_kw`
- **`Heater`**, **`PvInverter`**, **`BaseLoad`**: return `None` — not energy-storage, SoC target concept does not apply

`current_soc` is read from `current_values.get("soc_pct").map(|pct| pct / 100.0).unwrap_or(config.initial_soc)`.

#### 2. Update `controller/user_request.rs`

Replace `resolve_target(body, profile, sim)` with a lookup into `Vec<AssetEntry>`:

```rust
fn resolve_target(
    body: &CreateUserRequestBody,
    assets: &[AssetEntry],              // replaces profile + sim
) -> Result<(f64, f64), RequestError> {
    if let Some(kwh) = body.target_energy_kwh {
        if kwh <= 0.0 { return Err(RequestError::ZeroEnergy); }
        return Ok((kwh, body.desired_power_kw.unwrap_or(1.0)));
    }
    let entry = assets.iter()
        .find(|a| a.id == body.asset_id)
        .ok_or_else(|| RequestError::UnknownAsset(body.asset_id.clone()))?;
    let current_values = entry.state.state_values();
    entry.state
        .resolve_request_target(body.target_soc, body.desired_power_kw, &current_values)
        .ok_or(RequestError::ZeroEnergy)
}
```

Remove `Profile` import from `user_request.rs`. Update `create_from_body` signature to pass `&[AssetEntry]` instead of `(&Profile, Option<&SimSnapshot>)`. Update all callers in `main.rs` accordingly.

### Acceptance criteria

1. `controller/user_request.rs` contains no `match body.asset_id.as_str()` switch.
2. `controller/user_request.rs` does not import `Profile` or `SimSnapshot`.
3. `POST /requests` for `asset_id: "ev"` with `target_soc` behaves identically to before.
4. `POST /requests` for `asset_id: "battery"` with `target_soc` behaves identically to before.
5. `POST /requests` for a non-requestable asset (e.g. `"pv"`) returns a 400 / ZeroEnergy error.
6. All existing UC-05 (user request) BDD scenarios pass unchanged.
