# HTTP API Contracts: 016 — Refactor VEN Backend

This refactoring makes **no changes to any HTTP endpoint**. All contracts below document the preserved interface and confirm each endpoint's implementation path through the refactored code.

---

## Preserved Endpoints

### `GET /capability`

**Path**: `VEN/src/routes/assets.rs:74`

**Before refactoring**: Calls `cfg.capability(&entry.state)` — already uses the live `AssetCapability` (singular) API.

**After refactoring**: Unchanged. The dead `AssetCapabilities` (plural) struct and `capabilities()` method are removed from `assets/mod.rs` but were never called from this route.

**Response shape**: Unchanged.

```
GET /capability
→ 200 JSON: AssetCapabilityResponse (array of per-asset capability objects)
```

---

### `POST /hems/requests` and `GET /hems/requests`

**Path**: `VEN/src/routes/hems.rs`

**Change**: The inline `"heater"` / `"boiler"` string literals at ~line 275 are replaced with `crate::ids::HEATER` / `crate::ids::BOILER`. Behaviour identical.

**Response shapes**: Unchanged.

---

### All other VEN endpoints

No source changes in their handler files. Refactoring changes are restricted to:
- `profile.rs` (data type cleanup — not called from routes directly)
- `state.rs` (accessor field path changes — return types unchanged)
- `assets/mod.rs` (dead code deletion — no live methods removed)
- `controller/profile.rs` (deleted file — not reachable from routes)

**No route registrations added or removed** (`routes/mod.rs` unchanged).

---

## Startup Behaviour Change (FR-009)

This is the **only observable behaviour change** introduced by this refactoring.

### Before

When `PROFILE_PATH` points to a YAML file containing `devices:` (legacy format) instead of `assets:`:
- YAML parses successfully (both fields exist in `Profile`)
- `assets` is empty, `devices` is populated
- VEN starts and runs with legacy device configs

### After

When `PROFILE_PATH` points to a YAML file with `devices:` and no `assets:`:
- YAML parses successfully
- `try_load()` post-parse guard fires: `anyhow::bail!("Profile has no assets — check for legacy 'devices:' key ...")`
- `main()` receives `Err`, logs the error, and **exits with a non-zero status code**
- VEN does not start

### Not affected

- YAML files already using `assets:` format (all 5 production profiles: ven-1, ven-2, ven-3, test, no_pv_test)
- VENs started without `PROFILE_PATH` (use `Profile::default()` — bypasses the guard)
