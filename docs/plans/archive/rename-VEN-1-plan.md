# Rename VEN-1: UUID ID + Consistent venName

## Goal

Make VEN-1 consistent with VEN-2 and VEN-3:
- **ID**: Change from string `ven-1` to a hardcoded UUID (e.g. `a1b2c3d4-e5f6-7890-abcd-ef1234567890`) in the SQL fixtures
- **venName**: Change from `ven-1-name` to `ven-1` (matching VEN-2 = `ven-2`, VEN-3 = `ven-3`)

Do sequentially: **first the ID**, then the **venName**.

---

## Why VEN-1 Is Different Today

VEN-2 and VEN-3 are provisioned via the API at runtime → VTN assigns UUID IDs automatically.
VEN-1 is pre-seeded via SQL fixtures in `openleadr-rs/fixtures/` with hardcoded string ID `ven-1`.
Its venName `ven-1-name` also doesn't follow the pattern of the other VENs.

---

## Part 1: Replace ID `ven-1` → UUID

Pick a fixed UUID, e.g. `a1b2c3d4-e5f6-7890-abcd-ef1234567890` (generate a real one at implementation time).

### 1.1 openleadr-rs Submodule — SQL Fixtures

| File | What to change |
|---|---|
| `fixtures/vens.sql` | Lines 7, 34: `'ven-1'` → `'<UUID>'` |
| `fixtures/test_user_credentials.sql` | Lines 51, 58, 59: VEN ID `'ven-1'` → `'<UUID>'` |
| `fixtures/openadr_testsuite_user.sql` | Lines 34, 41: VEN ID `'ven-1'` → `'<UUID>'` |
| `fixtures/vens-programs.sql` | Lines 2, 11: `'ven-1'` → `'<UUID>'` |

### 1.2 openleadr-rs Submodule — Rust Tests

All references to `"ven-1"` as a VEN ID in Rust test code. These are **inside the submodule** (fork `TinkerPhu/openleadr-rs`).

| File | Approx lines | Context |
|---|---|---|
| `openleadr-vtn/src/data_source/postgres/ven.rs` | 332, 495, 557, 583, 592, 608, 614 | `"ven-1".parse().unwrap()` in unit tests |
| `openleadr-vtn/src/api/ven.rs` | 123, 152, 154, 165, 173, 181, 185 | API test assertions `"ven-1"` |
| `openleadr-vtn/src/api/resource.rs` | 136, 150, 153, 170, 176, 184, 194, 209, 217, 222, 232, 255, 267, 277, 287, 296, 316 | `/vens/ven-1/resources` URL paths |
| `openleadr-vtn/src/api/event.rs` | 702, 715, 727, 749, 759, 772, 833 | VEN role `"ven-1"` |
| `openleadr-vtn/src/api/program.rs` | 619, 631, 643, 672, 685, 706 | `targetValues=ven-1` query string |
| `openleadr-vtn/src/api/report.rs` | 131 | VEN role |
| `openleadr-vtn/src/api/user.rs` | 183 | VEN role |
| `openleadr-client/tests/common/mod.rs` | 19 | `("ven-1", "ven-1")` client credentials mapping |

**Important**: The `client_id` in `test_user_credentials.sql` line 88 is also `'ven-1'`. This is the OAuth credential, not the VEN entity ID. The `client_id` stays `ven-1` (matches docker-compose `CLIENT_ID`). Only the VEN entity ID (PK in `ven` table) changes to UUID.

Wait — actually `client_id` is separate from `ven.id`. In the credentials table:
- `user_id` = `'ven-1-user'` (unchanged)
- `client_id` = `'ven-1'` (the OAuth login — **keep as-is**, this is what VEN uses to authenticate)
- The VEN entity `ven.id` is what gets the UUID

So the credentials row stays the same. Only `ven.id`, `user_ven.ven_id`, and `ven_program.ven_id` change.

### 1.3 VEN Application — Rust Source

| File | Lines | What |
|---|---|---|
| `VEN/src/config.rs` | 23 | Default `VEN_NAME` fallback `"ven-1"` — this is venName, not ID. **No change needed for ID step.** |
| `VEN/src/reporter.rs` | 153-201 | Unit test fixtures use `"ven-1"` as clientName in reports. clientName = venName, so **deferred to Part 2**. |

### 1.4 Docker Compose Files

The docker-compose `CLIENT_ID: "ven-1"` and `CLIENT_SECRET: "ven-1"` are OAuth credentials, **not** VEN entity IDs. **No change needed** — the VEN authenticates with client_id/client_secret, which map to the `user_credentials` table, not the `ven.id`.

### 1.5 Test Infrastructure (Python/Behave)

| File | Lines | What |
|---|---|---|
| `tests/features/steps/ven_isolation_steps.py` | 19 | `get_token_value("ven-1", "ven-1")` — OAuth client_id/secret. **No change.** |
| `tests/features/steps/reports_steps.py` | 40, 65, 85 | `"clientName": "ven-1"` — clientName = venName. **Deferred to Part 2.** |
| `tests/features/steps/use_case_steps.py` | 304 | `"clientName": "ven-1"` — **Deferred to Part 2.** |
| `tests/features/steps/sim_steps.py` | 89, 95 | `"auto-ven-1-..."` report name prefix — uses venName. **Deferred to Part 2.** |
| `tests/features/steps/resilience_steps.py` | 9, 16 | `"test-ven-1"` — docker service name. **No change.** |
| `tests/features/helpers/api_client.py` | 7 | `"http://test-ven-1:8080"` — docker service name. **No change.** |

### 1.6 Feature Files

| File | Lines | What |
|---|---|---|
| `tests/features/ven_isolation.feature` | 12, 27 | `clientName "ven-1"` — venName. **Deferred to Part 2.** |
| `tests/features/use_cases.feature` | 17, 48, 69, 79 | `from "ven-1"` — refers to venName. **Deferred to Part 2.** |
| `tests/features/ui_use_cases.feature` | 19, 21, 36, 38, 81, 110 | `from "ven-1"` — venName. **Deferred to Part 2.** |
| `tests/features/ven_resilience.feature` | 42, 43 | `"test-ven-1"` — service name. **No change.** |

### 1.7 UI Source (TypeScript/React)

| File | Lines | What |
|---|---|---|
| `VEN/ui/src/App.tsx` | 19, 91 | `venName: "ven-1"` — **Deferred to Part 2.** |
| `VTN/ui/src/__tests__/*.test.tsx` | Various | Mock data with `"ven-1"` — mostly venName or arbitrary test IDs. Review each. |
| `VTN/ui/src/components/EventFormDialog.tsx` | 149 | helperText example `"ven-1"` — cosmetic. **Deferred to Part 2.** |

### 1.8 Seed Script

`scripts/seed_vtn.py` — Uses `ven-1-name` in targets (venName, not ID). **No change for ID step.**

### 1.9 Documentation

These files mention `ven-1` in various contexts. Update where it clearly refers to the VEN entity ID:

- `docs/USE-CASE-MANUAL.md`
- `docs/concept_vtn_ven_demand_response_simulation.md`
- `docs/project_journal.md` (historical — may leave as-is)
- `README.md`
- `VEN/status_example.json`, `VEN/report_example.json`
- `VEN/ui/src/api/status_example.json`
- `VTN/vtn_setup_from_blog_step_by_step.md`
- `VTN/vtn_rust_bff_blueprint.md`
- `VEN/ven_container_blueprint.md`

**Decision**: Documentation that describes historical steps (journal, blueprints) can be left as-is. Only update docs that serve as current reference (USE-CASE-MANUAL, README, example JSONs).

### Part 1 Summary

The ID rename is **mostly contained within the openleadr-rs submodule** (fixtures + Rust tests). The outer project barely uses the VEN entity ID directly — it uses OAuth client_id (`ven-1`) and venName (`ven-1-name`) instead.

**Key insight**: `client_id` ≠ `ven.id`. The OAuth credential `client_id=ven-1` stays the same. Only the database PK `ven.id` changes to a UUID.

---

## Part 2: Replace venName `ven-1-name` → `ven-1`

### 2.1 openleadr-rs Submodule — SQL Fixtures

| File | What |
|---|---|
| `fixtures/vens.sql` | Line 10: `'ven-1-name'` → `'ven-1'` |
| `fixtures/test_user_credentials.sql` | Line 54: `'ven-1-name'` → `'ven-1'` |
| `fixtures/openadr_testsuite_user.sql` | Line 37: `'ven-1-name'` → `'ven-1'` |

### 2.2 openleadr-rs Submodule — Rust Tests

| File | Lines | What |
|---|---|---|
| `openleadr-vtn/src/data_source/postgres/ven.rs` | 336 | `"ven-1-name"` → `"ven-1"` |
| `openleadr-vtn/src/api/program.rs` | 608 | `"ven-1-name"` in target values → `"ven-1"` |

### 2.3 Seed Script

`scripts/seed_vtn.py` — Lines 24, 62, 79, 177, 211: `"ven-1-name"` → `"ven-1"` in target values.

### 2.4 VEN Application

| File | Lines | What |
|---|---|---|
| `VEN/src/config.rs` | 23 | Default already `"ven-1"` — coincidence, **no change needed** |
| `VEN/src/reporter.rs` | 153-201 | Unit test `"ven-1"` as clientName — already correct after rename |

### 2.5 Docker Compose

`VEN/docker-compose.yml` line 31: `VEN_NAME: "ven-1"` — already `ven-1`. **No change.**
`tests/docker-compose.test.yml` line 72: `VEN_NAME: "ven-1"` — already `ven-1`. **No change.**

(The VEN uses `VEN_NAME` env var which becomes the `venName` in the VTN. This already says `ven-1`, which is the target value. But the fixture sets `ven_name='ven-1-name'` — so currently there's a mismatch between the fixture venName and what docker-compose sends. After the fixture change, they'll match.)

### 2.6 Test Steps & Feature Files

| File | Lines | Change |
|---|---|---|
| `tests/features/enrollment.feature` | 16 | `ven-1-name` → `ven-1` |
| `tests/features/use_cases.feature` | 9, 40, 61, 82, 112, 133 | `ven-1-name` → `ven-1` |
| `tests/features/ven_simulator.feature` | 33, 43, 53 | `ven-1-name` → `ven-1` |
| `tests/features/ui_use_cases.feature` | 26, 71, 100, 115 | `ven-1-name` → `ven-1` |
| `tests/features/steps/ven_isolation_steps.py` | 153 | `"ven-1-name"` → `"ven-1"` |

### 2.7 Feature files using venName `"ven-1"` (clientName context)

These already say `"ven-1"` — but this was actually the **client_id**, not the venName. After rename, `venName = "ven-1"` too, so the assertions still hold. **No change needed** in:
- `tests/features/ven_isolation.feature` lines 12, 27
- `tests/features/use_cases.feature` lines 17, 48, 69, 79
- `tests/features/ui_use_cases.feature` lines 19, 21, 36, 38, 81, 110

### 2.8 Reports Steps

`clientName` in reports comes from `venName`, which after the rename becomes `ven-1`. The test assertions already expect `"ven-1"`. **No change needed** in:
- `tests/features/steps/reports_steps.py` (lines 40, 65, 85)
- `tests/features/steps/use_case_steps.py` (line 304)
- `tests/features/steps/sim_steps.py` (lines 89, 95)

### 2.9 VEN UI

`VEN/ui/src/App.tsx` line 19: `venName: "ven-1"` — already correct. **No change.**

### 2.10 VTN UI Tests

Review mock data in `VTN/ui/src/__tests__/*.test.tsx` — update any `ven-1-name` references.

### 2.11 Documentation

Same files as Part 1. Update `ven-1-name` → `ven-1` in:
- `docs/USE-CASE-MANUAL.md` (lines 53, 226, 340, 458, 547, 600, 624, 657)
- Other current-reference docs

### 2.12 MEMORY.md

Update the memory note: "VEN-1 fixture venName is `ven-1-name`" → remove or update after rename.

---

## Risks & Considerations

1. **Submodule changes**: The openleadr-rs fork needs a commit. This affects the submodule pointer in the main repo.

2. **Rust test breakage**: The Rust tests in openleadr-rs use `"ven-1"` extensively. Need to build and run `cargo test` in the submodule after changes.

3. **`targetValues=ven-1` in query strings** (program.rs line 672): After Part 2, this becomes ambiguous — is it the ID or the venName? Actually targets filter by venName, so after rename venName=`ven-1`, this query string stays the same but now matches the new venName. Should work.

4. **Test environment**: After fixture changes, the test DB will have the new UUID. The VEN container still authenticates with `client_id=ven-1` → the `user_credentials` table still has `client_id='ven-1'` → auth still works. The VEN entity ID in the DB is a UUID, but the VEN app never directly uses its own entity ID — it discovers it via the API.

5. **Production data**: The running VTN on Pi4-Server has `ven-1` as the VEN entity ID in its live database. After deploying the new fixtures, the VTN auto-migrates but fixtures only run on a fresh DB. **The production DB won't change** — only the test environment (which uses ephemeral DBs) will see the new UUID. The production VEN-1 entity would need manual migration or re-provisioning.

6. **Confusion potential**: After rename, both the `client_id` and the `venName` will be `ven-1`, but the `ven.id` will be a UUID. This is actually the same pattern as VEN-2 and VEN-3.

---

## Execution Order

1. Generate a real UUID for VEN-1 entity ID
2. **Part 1**: Replace `ven-1` → UUID in all VEN entity ID contexts (fixtures, Rust tests)
3. Build & test openleadr-rs: `cargo test` in submodule
4. Commit submodule changes
5. **Part 2**: Replace `ven-1-name` → `ven-1` in all venName contexts (fixtures, seed, tests, docs)
6. Run full integration test suite on Pi4-Server
7. Update MEMORY.md
8. Commit main repo (with updated submodule pointer)

---

## Estimated Scope

| Area | Part 1 (ID) | Part 2 (venName) |
|---|---|---|
| Fixture SQL files | 4 files, ~10 replacements | 3 files, ~3 replacements |
| Rust tests (submodule) | ~8 files, ~50+ replacements | 2 files, ~2 replacements |
| Seed script | 0 | 1 file, ~5 replacements |
| Test steps/features | 0 | ~6 files, ~15 replacements |
| Docs | ~3 files | ~2 files |
| UI code | Review needed | ~1 file |
| **Total** | **~15 files** | **~15 files** |
