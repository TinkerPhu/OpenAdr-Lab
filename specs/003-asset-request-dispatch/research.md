# Research: Asset Request Dispatch Refactor

**Feature**: 003-asset-request-dispatch
**Date**: 2026-03-15

## No External Research Required

This is a pure internal refactor of existing Rust code within the VEN simulator. All decisions are resolved by reading the current source.

---

## Decision 1: Where to implement `resolve_request_target`

**Decision**: Add as a method on `AssetState` (the enum) in `VEN/src/simulator/assets/mod.rs`.

**Rationale**: `AssetState` already follows the dispatch-via-enum-method pattern for `update`, `state_values`, `capabilities`, `control_schema`, `reset`, and `update_config`. Adding `resolve_request_target` follows the same established pattern. Each variant delegates to the inner struct which owns all config and current state.

**Alternatives considered**:
- Adding a trait `Requestable` — rejected (unnecessary abstraction for one method; constitution Principle IV: lean architecture)
- Adding free functions per asset type — rejected (breaks encapsulation already established by AssetState's method dispatch)

---

## Decision 2: Method signature — `current_values` parameter

**Decision**: Do NOT include a `current_values: &HashMap<String, f64>` parameter. Access `self.soc` and config fields directly on the inner struct, since the method is implemented with a `match self` inside `AssetState`.

**Rationale**: The inner structs (`EvCharger`, `Battery`) already hold live state in their fields (`.soc`, `.battery_kwh`, etc.). Passing `current_values` would redundantly re-derive what the struct already knows, and would require callers to call `state_values()` to create a HashMap just to pass it back in. Lean architecture (constitution Principle IV): no unnecessary indirection.

**Spec note**: The spec suggested `current_values` as a design option, but direct field access is simpler and equally correct.

---

## Decision 3: Source of live asset state in `post_requests` handler

**Decision**: Lock `ctx.sim` (the `Arc<Mutex<SimState>>`), clone the assets `Vec<AssetEntry>`, release the lock, then pass `&assets` to `create_from_body`.

**Rationale**: `AppCtx.sim: Arc<Mutex<SimState>>` holds the live mutable state including all `AssetEntry` objects with current SoC and config. The existing `ctx.state.sim()` path only returns a `SimSnapshot` (a derived view that stores `HashMap<String, AssetSnapshot>` — not full `AssetEntry` objects). The lock is held briefly (just to clone the Vec) to avoid holding it across the async call.

**Alternatives considered**:
- Adding a `sim_assets()` method to `AppState` — rejected (AppState doesn't hold SimState; the SimState is in AppCtx directly)
- Passing `SimSnapshot` — rejected (it does not contain `AssetState` structs needed for config-aware computation)

---

## Decision 4: `initial_soc` fallback when no sim state exists

**Decision**: Use `inner.soc` (the EvCharger/Battery's current SoC field, which is initialized to `initial_soc` from the profile at startup) as the fallback.

**Rationale**: After speckit 1, `EvCharger.soc` IS the live state, initialized to `initial_soc`. If no tick has run yet, `soc` == `initial_soc`. Using `inner.soc` directly is identical in semantics to the old `unwrap_or(cfg.initial_soc)` fallback.

---

## Decision 5: Battery default `target_soc`

**Decision**: Keep 1.0 (100%) as the default when `body.target_soc` is None for a Battery request. This matches the existing behavior in `resolve_target`.

---

## Affected Files (exhaustive)

| File | Change |
|------|--------|
| `VEN/src/simulator/assets/mod.rs` | Add `resolve_request_target` method to `AssetState` impl block |
| `VEN/src/simulator/assets/ev.rs` | Add `resolve_request_target` method on `EvCharger` |
| `VEN/src/simulator/assets/battery.rs` | Add `resolve_request_target` method on `Battery` |
| `VEN/src/controller/user_request.rs` | Replace `resolve_target` signature; remove `Profile`/`SimSnapshot` imports |
| `VEN/src/main.rs` | Update `post_requests` handler to pass `&assets` from `ctx.sim` lock |

No BDD feature files, UI files, API contracts, or other modules require changes.
