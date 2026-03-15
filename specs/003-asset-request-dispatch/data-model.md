# Data Model: Asset Request Dispatch Refactor

**Feature**: 003-asset-request-dispatch
**Date**: 2026-03-15

## No Data Model Changes

This refactor does not add, remove, or rename any entities, fields, or API shapes. The `UserRequest` and `EnergyPacket` entities are unchanged. The only change is internal to how the `target_energy_kwh` and `desired_power_kw` values are *computed* from the live asset state.

---

## Interface Change: `AssetState::resolve_request_target`

A new private-to-the-simulator method is added to the `AssetState` enum. This is not an external API — it is an internal capability of the simulator asset model.

### Signature

```
AssetState::resolve_request_target(
    &self,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
) -> Option<(f64, f64)>
```

**Returns**: `Some((target_energy_kwh, desired_power_kw))` if the asset supports energy requests and there is useful work to do, `None` otherwise.

### Per-Variant Behaviour

| Variant | Requestable | target_energy_kwh | desired_power_kw |
|---------|-------------|-------------------|-----------------|
| `Ev(EvCharger)` | Yes | `(target_soc.unwrap_or(inner.soc_target) - inner.soc).max(0.0) * inner.battery_kwh` | `desired_power_kw.unwrap_or(inner.max_charge_kw)` |
| `Battery(Battery)` | Yes | `(target_soc.unwrap_or(1.0) - inner.soc).max(0.0) * inner.capacity_kwh` | `desired_power_kw.unwrap_or(inner.max_charge_kw)` |
| `Heater(Heater)` | No | — | — |
| `Pv(PvInverter)` | No | — | — |
| `BaseLoad(BaseLoad)` | No | — | — |

**Zero-energy rule**: If the computed `target_energy_kwh` is < 1e-6 (effectively zero), returns `None`.

---

## Signature Change: `create_from_body`

The public function signature in `controller/user_request.rs` changes:

**Before**:
```
create_from_body(body, profile: &Profile, sim: Option<&SimSnapshot>, now) -> Result<...>
```

**After**:
```
create_from_body(body, assets: &[AssetEntry], now) -> Result<...>
```

- `Profile` import removed
- `SimSnapshot` import removed
- `AssetEntry` import added (from `crate::simulator`)

---

## Caller Change: `post_requests` in `main.rs`

**Before**:
```rust
let sim = ctx.state.sim().await;
create_from_body(body, &ctx.profile, sim.as_ref(), now)
```

**After**:
```rust
let assets = ctx.sim.lock().await.assets.clone();
create_from_body(body, &assets, now)
```

The `ctx.profile` argument is eliminated entirely.
