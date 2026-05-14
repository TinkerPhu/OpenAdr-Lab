# Phase 6: Remove `PROFILE` from Routes (AB-06)

**Source plan:** `docs/plans/ven_backend_architecture_refactoring.md §4 Phase 6`  
**Branch base:** `022-deterministic-test-env` (or main after merging)  
**Prerequisite:** Phase 5 (application service layer) should be complete or the only remaining Profile use in routes is the `GET /sim/schema` route in `routes/sim.rs` — that breach alone is enough to implement Phase 6 as a standalone.  
**Estimated effort:** 0.5–1 day

---

## Problem

Architecture breach AB-06: `routes/` accesses `Profile` (raw YAML config) directly. The route layer must be pure HTTP — it must not know about config format or YAML structure.

**Current breach (confirmed in source):**

```
routes/sim.rs:118  →  crate::simulator::schema_from_profile(&ctx.profile)
```

`AppCtx.profile: Arc<Profile>` is present on every route handler through the shared state. The `GET /sim/schema` handler calls `schema_from_profile(&ctx.profile)`, which walks the raw `AssetProfile` variants and constructs `ControlDescriptor` maps. This ties the HTTP layer directly to `simulator/` and `profile::`.

After Phase 5, additional profile reads from services may surface in routes — this phase removes all of them.

---

## Goal

**Routes must have zero `profile::` imports and zero `ctx.profile` accesses.** All profile-derived values must be pre-computed and stored in `AppCtx` as typed, domain-level values at startup, so route handlers read an already-prepared value — not the raw config.

```
Before: route → ctx.profile → schema_from_profile(&profile) → ControlDescriptor map
After:  route → ctx.sim_schema  (pre-built HashMap, no Profile access at call time)
```

---

## Scope

### In scope

1. **`GET /sim/schema` (`routes/sim.rs`)** — the only confirmed profile access in routes today.
   - Pre-compute the `HashMap<String, Vec<ControlDescriptor>>` from `Profile` in `main.rs` at startup.
   - Store it in `AppCtx` as `pub sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>`.
   - Route handler reads `ctx.sim_schema.clone()` — no profile access.
   - Remove `ctx.profile` from `AppCtx` if this is the last consumer in the route layer (check all callers first).

2. **Any additional `ctx.profile` or `profile::` references surfaced in routes after Phase 5** — handled with the same pattern: extract value at startup into a typed field in `AppCtx`.

3. **Success criterion compile-check** — add a `#[cfg(test)]` assertion in `routes/sim.rs` (or a `tests/phase6.rs` integration test) that verifies `routes/hems.rs` and `routes/sim.rs` do not import `profile::`. A `grep`-based CI check is acceptable as a lightweight alternative.

### Out of scope

- `tasks/sim_tick/tick.rs` and `tasks/sim_tick/mod.rs` — `profile: Arc<Profile>` on the tick task context. Tasks are driving adapters, not routes; they are cleaned in Phase 4/5.
- `main.rs` — reading `Profile` at startup is correct; that is the infrastructure entry point.
- Any changes to business logic.

---

## Acceptance Criteria

| # | Criterion | How to verify |
|---|-----------|---------------|
| AC-1 | `grep -r "profile" VEN/src/routes/` returns 0 matches on import lines (`use crate::profile`, `use crate::simulator::schema_from_profile`, `ctx.profile`) | CI grep or `cargo check` |
| AC-2 | `GET /sim/schema` returns identical JSON before and after the change | Existing BDD suite covers this if an integration test is added; at minimum: manual diff |
| AC-3 | `AppCtx.profile` field is removed from `AppCtx` or documented as routes-only-unused with a tracked removal ticket | Code review |
| AC-4 | All BDD scenarios remain green on Pi4 | Full BDD run |
| AC-5 | A `#[cfg(test)]` or integration test confirms `routes/sim.rs` builds without any direct profile import | `cargo test` |

---

## Implementation Notes

### Pre-compute sim_schema at startup

In `main.rs` (or wherever `AppCtx` is constructed):

```rust
// Build once from profile; stored as Arc so cloning into route response is cheap.
let sim_schema = Arc::new(crate::simulator::schema_from_profile(&profile));

let ctx = AppCtx {
    ...
    sim_schema,
    // profile: can be removed from AppCtx if no other route accesses it
};
```

`schema_from_profile` is in `simulator/mod.rs` and already constructs from `Profile` — no logic changes needed there; only the call site moves from handler to startup.

### AppCtx field type

```rust
// main.rs
pub struct AppCtx {
    ...
    pub sim_schema: Arc<HashMap<String, Vec<crate::assets::ControlDescriptor>>>,
}
```

If `ControlDescriptor` is not already re-exported from `assets/mod.rs`, add the re-export there.

### Route handler after change

```rust
// routes/sim.rs — before
let schema = crate::simulator::schema_from_profile(&ctx.profile);

// routes/sim.rs — after
let schema = ctx.sim_schema.as_ref().clone();
```

### Profile removal from AppCtx

After this phase, check if any other field in `AppCtx` still needs `profile`. If not, remove `pub profile: Arc<Profile>` from `AppCtx`. This is the structural payoff of the phase — the route context has no config dependency.

If `profile` must stay (e.g., tasks/ still reference it via `AppCtx`), document it clearly and track removal in Phase 4/5 cleanup.

---

## Testing Obligations (from §6 per-phase table)

> _"Route-level test confirming no `profile::` import compiles in `routes/hems.rs`"_

Extend to cover `routes/sim.rs` as well. Acceptable implementations:

**Option A — grep in CI (`run_all_tests.sh`):**
```bash
! grep -rn "profile::" VEN/src/routes/ 2>/dev/null
```

**Option B — Rust compile-time test in `VEN/tests/phase6_boundary.rs`:**
```rust
// Verifies routes module does not import profile types at all.
// If routes/sim.rs ever re-adds a profile import, this module will fail to compile.
// (No runtime assertions needed — the absence of `use profile::` is the test.)
#[allow(unused_imports)]
mod _boundary_check {
    // This file intentionally imports only route-level types.
    // Any profile:: leak in routes/ causes a compile error here.
}
```

Option A is simpler and sufficient. Add it to `run_all_tests.sh` or a `make lint` target.

---

## Related architecture IDs

- **AB-06** (this phase): `R_HEMS → PROFILE` (and `R_SIM → PROFILE`)
- **AB-04** (Phase 4): Profile removed from `entities/`, `assets/`, `controller/`, `simulator/` — must be complete before this phase removes the last foothold
- The full breach list and diagram are in `docs/architecture/ven_backend_review.md` and `docs/architecture/ven_backend_components.md`
