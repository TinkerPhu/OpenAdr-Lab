# Quickstart: Verify Phase 4 Implementation

**Branch**: `021-decouple-profile-domain`

This document describes how to verify that Phase 4 is correctly implemented.
No new user-facing behaviour is introduced — verification is structural + test-based.

---

## 1. Structural invariant check (SC-001)

Run from `VEN/` after building:

```bash
# Must return zero matches
grep -r "use crate::profile" \
  VEN/src/entities \
  VEN/src/assets \
  VEN/src/controller \
  VEN/src/simulator
```

Expected: no output.

```bash
# SC-005: PlannerObjective importable from domain ring directly
grep -r "use crate::entities.*PlannerObjective" VEN/src/
```

Expected: matches in controller/ and entities/ files (not in profile.rs imports).

---

## 2. Compile check

```bash
cd VEN
cargo build 2>&1 | grep -E "^error"
```

Expected: no errors.

Offline compile-check (no database required — VEN has no SQLx):

```bash
cd VEN
cargo check
```

---

## 3. Unit tests (SC-002, SC-003)

```bash
cd VEN
cargo test --workspace 2>&1 | tail -20
```

Expected: all tests pass. Test count must not decrease vs Phase 3 baseline.

Verify new per-asset unit tests exist:

```bash
grep -rn "#\[test\]" VEN/src/assets/
```

Expected: at least one `#[test]` block in each of `battery.rs`, `ev.rs`, `heater.rs`,
`pv.rs`, `base_load.rs`.

---

## 4. BDD suite (SC-004, FR-010)

From Pi4-Server (requires running stack):

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
```

Expected: all scenarios pass. No `--tags` exclusions needed.

---

## 5. Integration smoke test

Build and run the VEN Docker image locally:

```bash
cd VEN
docker compose up -d --build ven-1
docker compose logs ven-1 | grep -E "error|panic|PROFILE"
```

Expected: no errors; `loaded simulator profile` log line present; VEN registers with VTN.

---

## 6. Runtime objective override check (Edge Case)

Verify the watch-channel objective override still works after `PlannerObjective` moved to `entities/`:

```bash
# POST a HEMS user-request with objective=min_ghg override (if applicable)
# OR check that active_objective is correctly initialised at startup
grep "active_objective" VEN/src/main.rs
```

Expected: `active_objective` initialised from `planner_params.objective`, not from `profile.planner.objective` directly.

---

## 7. Profile.rs bridge re-export removed (final cleanup)

After all domain callers are updated:

```bash
grep "pub use.*PlannerObjective" VEN/src/profile.rs
```

Expected: no match (bridge re-export was removed in the final cleanup task).
