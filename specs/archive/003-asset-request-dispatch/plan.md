# Implementation Plan: Asset Request Dispatch Refactor

**Branch**: `003-asset-request-dispatch` | **Date**: 2026-03-15 | **Spec**: [spec.md](spec.md)

## Summary

Remove the hardcoded `match body.asset_id.as_str()` switch from `controller/user_request.rs` by adding a `resolve_request_target` method to `AssetState` in `simulator/assets/mod.rs`. Each energy-storage asset type implements the method on its own inner struct; non-storage assets return `None`. The `user_request.rs` module drops its `Profile` and `SimSnapshot` imports and receives `&[AssetEntry]` from the live `SimState` instead.

No API, behavior, or UI changes. The BDD test suite for `ven_user_request.feature` must continue to pass without modification.

## Technical Context

**Language/Version**: Rust (stable, 1.75+)
**Primary Dependencies**: axum, tokio, serde — existing VEN service
**Storage**: N/A (no persistence change)
**Testing**: Python behave (BDD, `tests/features/`), cargo test (unit)
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: web-service (VEN backend)
**Performance Goals**: No change — refactor only
**Constraints**: No API contract changes; all existing BDD scenarios must pass
**Scale/Scope**: 5 files changed; ~50 lines net delta

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | PASS | No field names changed; no API layer touched |
| II. BDD-First Testing | PASS | `ven_user_request.feature` already covers the acceptance criteria. No new BDD features needed (pure refactor). Existing scenarios must continue to pass; write one new scenario for the non-storage rejection case (AC5) if not already covered. |
| III. Upstream Compatibility | N/A | VEN is not openleadr-rs; no upstream PR |
| IV. Lean Architecture | PASS | Removes an abstraction leak (profile import); adds one method to an existing enum dispatch pattern |
| V. Infrastructure Parity | PASS | Tests run via SSH on Pi4-Server; deploy flow unchanged |

**Re-check after Phase 1**: PASS — no new entities, no new API surface, no new dependencies.

## Project Structure

### Documentation (this feature)

```text
specs/003-asset-request-dispatch/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code (affected files only)

```text
VEN/src/
├── simulator/
│   └── assets/
│       ├── mod.rs          ← add AssetState::resolve_request_target dispatch
│       ├── ev.rs           ← add EvCharger::resolve_request_target
│       └── battery.rs      ← add Battery::resolve_request_target
├── controller/
│   └── user_request.rs     ← refactor resolve_target; remove imports
└── main.rs                 ← update post_requests caller

tests/features/
└── ven_user_request.feature  ← add scenario: non-storage asset rejected (if missing)
```

**Structure Decision**: Single Rust project (VEN service). Only existing files are modified; no new files created in the source tree.

## Phase 0: Research (complete)

See [research.md](research.md).

Key findings:
1. `AssetState` already dispatches 6 methods via `match self`. Adding a 7th follows the exact same pattern — zero new abstractions.
2. `resolve_request_target` does NOT need a `current_values` parameter. `EvCharger.soc` and `Battery.soc` are live state; direct field access is simpler than re-deriving via `state_values()`.
3. `AppCtx.sim: Arc<Mutex<SimState>>` holds the full `Vec<AssetEntry>` with live state. The handler acquires the lock briefly, clones the Vec, releases the lock, then calls `create_from_body`.
4. `Battery` default `target_soc` = 1.0 (100%) when not specified — matches current behavior.
5. All 5 source files are within the VEN service; no other service is touched.

## Phase 1: Design & Contracts (complete)

See [data-model.md](data-model.md) and [quickstart.md](quickstart.md).

No external API contracts change. The `POST /user-requests` endpoint request/response schema is identical. The only change is internal method dispatch.

### Implementation Steps (for /speckit.tasks)

**Step 1 — EvCharger: add `resolve_request_target`**

In `VEN/src/simulator/assets/ev.rs`, add to `impl EvCharger`:

```rust
pub fn resolve_request_target(
    &self,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
) -> Option<(f64, f64)> {
    let target = target_soc.unwrap_or(self.soc_target);
    let delta = (target - self.soc).max(0.0);
    let kwh = delta * self.battery_kwh;
    if kwh < 1e-6 { return None; }
    Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
}
```

**Step 2 — Battery: add `resolve_request_target`**

In `VEN/src/simulator/assets/battery.rs`, add to `impl Battery`:

```rust
pub fn resolve_request_target(
    &self,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
) -> Option<(f64, f64)> {
    let target = target_soc.unwrap_or(1.0);
    let delta = (target - self.soc).max(0.0);
    let kwh = delta * self.capacity_kwh;
    if kwh < 1e-6 { return None; }
    Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
}
```

**Step 3 — AssetState: add dispatch method**

In `VEN/src/simulator/assets/mod.rs`, add to `impl AssetState`:

```rust
pub fn resolve_request_target(
    &self,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
) -> Option<(f64, f64)> {
    match self {
        Self::Ev(inner) => inner.resolve_request_target(target_soc, desired_power_kw),
        Self::Battery(inner) => inner.resolve_request_target(target_soc, desired_power_kw),
        Self::Heater(_) | Self::Pv(_) | Self::BaseLoad(_) => None,
    }
}
```

**Step 4 — user_request.rs: replace `resolve_target`**

Remove imports: `use crate::profile::Profile;` and `use crate::simulator::SimSnapshot;`

Add import: `use crate::simulator::AssetEntry;` (already imported transitively, but confirm)

Replace `resolve_target` signature:
```rust
fn resolve_target(
    body: &CreateUserRequestBody,
    assets: &[AssetEntry],
) -> Result<(f64, f64), RequestError> {
    if let Some(kwh) = body.target_energy_kwh {
        if kwh <= 0.0 { return Err(RequestError::ZeroEnergy); }
        return Ok((kwh, body.desired_power_kw.unwrap_or(1.0)));
    }
    let entry = assets.iter()
        .find(|a| a.id == body.asset_id)
        .ok_or_else(|| RequestError::UnknownAsset(body.asset_id.clone()))?;
    entry.state
        .resolve_request_target(body.target_soc, body.desired_power_kw)
        .ok_or(RequestError::ZeroEnergy)
}
```

Update `create_from_body` signature:
```rust
pub fn create_from_body(
    body: CreateUserRequestBody,
    assets: &[AssetEntry],
    now: DateTime<Utc>,
) -> Result<(UserRequest, EnergyPacket), RequestError>
```

Update internal call: `resolve_target(&body, assets)?`

**Step 5 — main.rs: update caller**

In `post_requests`:
```rust
let assets = ctx.sim.lock().await.assets.clone();
match controller::user_request::create_from_body(body, &assets, now) {
```

Remove the `let sim = ctx.state.sim().await;` line (no longer needed here).

**Step 6 — BDD: add non-storage rejection scenario**

Check `tests/features/ven_user_request.feature` for a scenario targeting `"pv"` or another non-storage asset. If absent, add:

```gherkin
Scenario: Request for a non-storage asset returns an error
  When I POST a user request for asset "pv" with target_soc 0.9 and latest_end in 12 hours
  Then the response status is 422
  And the response JSON has field "error"
```

**Step 7 — Run full BDD test suite**

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

All scenarios must pass. Zero regressions.
