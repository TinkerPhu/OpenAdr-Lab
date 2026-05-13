# Quickstart: Remove Profile from Routes Layer (AB-06)

## What This Change Does

1. Adds `sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>` to `AppCtx`
2. Pre-computes it once at startup: `simulator::schema_from_profile(&profile)`
3. Updates `GET /sim/schema` to return `ctx.sim_schema.clone()` — no profile access
4. Adds `pub` visibility to `schema_from_profile` so integration tests can call it
5. Adds `VEN/tests/architecture.rs` — permanent boundary check
6. Adds `VEN/tests/schema_snapshot.rs` + fixture — schema identity test

## Verify After Implementation

```bash
# Constitution invariant — must be empty:
grep -r "use crate::profile" VEN/src/routes/

# Build:
cd VEN && cargo build

# All tests:
cd VEN && cargo test

# BDD (on Pi4):
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

## Key Files

| File | Change |
|------|--------|
| `VEN/src/main.rs` | Add `sim_schema` field; annotate `profile`; build schema at startup |
| `VEN/src/routes/sim.rs` | Replace `schema_from_profile` call with `ctx.sim_schema.clone()` |
| `VEN/src/simulator/mod.rs` | `pub(crate)` → `pub` on `schema_from_profile` |
| `VEN/tests/architecture.rs` | New: boundary check test |
| `VEN/tests/schema_snapshot.rs` | New: schema identity test |
| `VEN/tests/fixtures/schema_snapshot.json` | New: committed JSON snapshot |
