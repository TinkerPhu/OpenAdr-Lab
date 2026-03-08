## Context

The lab currently runs openleadr-rs on the `dev` branch (OpenADR 3.0 + our patches #372/#374).
The upstream `openadr3_1` branch (tag `v0.2.0-rc1`) passes all CI checks and introduces breaking
schema and API changes. This design covers the full migration across all six layers: VTN core,
BFF, VTN UI, VEN app, VEN UI, and test/seed infrastructure.

The migration is a fresh-deploy strategy — no live data migration. The PostgreSQL database is
wiped and re-created from the new schema on first boot. Seed data is re-loaded via a rewritten
seed script.

## Goals / Non-Goals

**Goals:**
- Run the full lab stack against `openadr3_1` with all tests passing
- Adopt the 3.1 scope-based auth model natively (no workarounds)
- Replace VEN_NAME-based targeting with clientId-based targeting throughout
- Self-registering VEN flow (VEN calls `POST /vens` with its own token)
- Redesign VEN simulator and reactor as clean 3.1-compatible modules
- All three UIs reflect 3.1 wire format

**Non-Goals:**
- MQTT subscriptions (polling retained)
- Compact price representation (issue #238)
- mDNS discovery (issue #315)
- Upstream PRs for lab-specific additions during this migration

## Decisions

### D1: Fresh deploy over live migration

**Decision**: Wipe the database and re-seed on deploy. Do not write SQL to transform 3.0 data
to 3.1 schema.

**Rationale**: The 3.1 migration drops 6 tables and changes 4 column types. `targets` changes
from JSONB to `text[]`; `ven_program` is dropped; user tables are replaced. Transforming live
data would require complex, error-prone SQL that adds risk with no lab value. A fresh seed
brings the database to a known-good 3.1 state in minutes.

**Alternative considered**: Write a migration script. Rejected because the schema diverges too
heavily — there is no meaningful row-level mapping from `{type, values}` JSONB to flat `text[]`.

---

### D2: Single `bl-client` credential in the BFF

**Decision**: Replace the dual-credential BFF (any-business + ven-manager) with a single
`bl-client` credential that holds scopes `{read_all, write_vens, write_programs, write_events, write_users}`.

**Rationale**: The 3.0 dual-credential hack existed because the role system split business and
ven-manager access. In 3.1, a single OAuth client can hold all required write scopes. The BFF
becomes a straightforward single-token proxy.

**Alternative considered**: Keep dual credentials for organizational separation. Rejected as
unnecessary complexity — the scope model is the correct abstraction for access control.

**API impact**:
```
POST /auth/token
  client_id: "bl-client"
  client_secret: "bl-client"
  → { access_token, token_type: "Bearer", expires_in: 2592000 }
```

---

### D3: VEN self-registration flow

**Decision**: Each VEN authenticates with its own credential (scopes: `read_targets`,
`read_ven_objects`, `write_reports`, `write_subscriptions`) and calls `POST /vens` with
`VenVenRequest` on first startup to register itself.

**Rationale**: 3.1 defines `VenVenRequest` (no `clientID` in body — derived from JWT sub)
vs `BlVenRequest` (business layer sets `clientID` explicitly). The self-registration flow
is the spec's intended VEN identity mechanism and removes the need for a separate
provisioning script step to link VEN objects to credentials.

**VEN provisioning sequence (3.1)**:
```
1. bl-client creates user with VEN scopes via /users (or fixture SQL)
2. bl-client creates user_credential with client_id + client_secret
3. VEN authenticates: POST /auth/token → token (sub = client_id)
4. VEN calls: POST /vens { venName: "ven-1" }
   → VTN sets VEN.clientID = token.sub automatically
5. bl-client sets program targets: ["ven-1-client", "ven-2-client"]
   → VEN sees only its enrolled programs via read_targets scope
```

**VEN profile update**: Add `client_id` field to YAML profiles so the VEN knows its OAuth
identity. The VEN's `venName` is a human label only.

---

### D4: Flat text[] targets — clientId as the privacy address

**Decision**: Program and event `targets` are `Vec<String>` (clientIds). An empty `targets`
array means "open to all VENs." Privacy filtering: VTN returns events/programs where
`client_id = ANY(targets) OR cardinality(targets) = 0`.

**Rationale**: The 3.1 spec simplifies targets from a typed `{type, values}` map to a flat
string array. The privacy filtering semantic is unchanged — the identifier changes from
`venName` to OAuth `clientId`.

**Wire format comparison**:
```json
// 3.0 (old)
"targets": [{"type": "VEN_NAME", "values": ["ven-1"]}]

// 3.1 (new)
"targets": ["ven-1-client"]
```

**Impact on our code**: The `TargetMap` type disappears. All code that constructed or parsed
`{type: "VEN_NAME", values: [...]}` objects is removed. `Target` in openleadr-wire is now
a newtype around `Identifier` (a plain string).

---

### D5: VEN simulator and reactor — clean redesign

**Decision**: Delete the existing `VEN/src/simulator/` and `VEN/src/reactor/` directories
and rewrite from scratch for 3.1. The existing physics models (PV, heater, EV) were correct
but tightly coupled to 3.0 event structures. The new design will:

- Use simpler, more testable model structs (no deep nesting)
- Accept 3.1 `EventInterval` payloads directly (flat `payloads: Vec<ValuesMap>`)
- Redesign the FSM with cleaner state transitions
- Keep the `GET /sim` and `GET /trace` endpoints for UI compatibility
- Remove `POST /sim/override` temporarily (can be re-added as a future improvement)

**Approach**: For the migration, choose the simplest correct implementation. The simulator
should demonstrate DR response (ramp/hold) without over-engineering. Per-VEN YAML profiles
remain; add `client_id` field.

---

### D6: Submodule strategy — TinkerPhu fork branch off upstream openadr3_1

**Decision**: In the `TinkerPhu/openleadr-rs` fork, create a branch `openadr3_1` that tracks
`upstream/openadr3_1`. Point the lab submodule to `TinkerPhu/openleadr-rs@openadr3_1`.

**Rationale**: The fork pattern is already established for our 3.0 patches. The upstream
`openadr3_1` branch is at `v0.2.0-rc1` with all-green CI. Future lab-specific fixes can be
branched from there as before.

**Steps**:
```bash
cd openleadr-rs
git fetch upstream
git checkout -b openadr3_1 upstream/openadr3_1
git push origin openadr3_1
# Update submodule pointer in lab root
```

## Risks / Trade-offs

**R1: Long Pi4 build time after branch switch**
→ Mitigation: `openadr3_1` has a new migration file and likely new Cargo.lock entries.
Expect a full rebuild (~25 min). Run during off-hours or accept the delay.

**R2: Seed script complexity**
→ Mitigation: The seed script must create users with scopes, credentials, VEN objects (via
bl-client calling `/vens` with `BlVenRequest` including `clientID`), and programs with
flat clientId targets. This is simpler than the 4-step 3.0 sequence but requires care with
the new user API shape. Test interactively before finalizing.

**R3: Integration test rewrite scope**
→ Mitigation: The BDD provisioning steps, enrollment assertions, and report field assertions
all change. This is the largest test effort. Prioritize the provisioning and enrollment
scenarios first; port simulator/reactor tests after the redesign is stable.

**R4: VEN self-registration race condition**
→ On first boot, all 3 VENs may attempt `POST /vens` simultaneously. The VTN handles
duplicate clientId with `UNIQUE INDEX ven_client_id_unique` — second attempt returns 409.
→ Mitigation: VEN startup logic: attempt `POST /vens`; if 409, treat as "already registered"
and continue. Log at INFO level.

**R5: Report schema change breaks existing test assertions**
→ `programId` and `venId` are gone; `clientId` is now on the Report object (not in request).
All test steps and UI displays that reference `programId` in report context must be updated.

## Migration Plan

### Phase 1 — VTN Core (submodule + DB)
1. Create `TinkerPhu/openleadr-rs@openadr3_1` branch from `upstream/openadr3_1`
2. Update submodule pointer; commit
3. Update `VTN/docker-compose.yml` fixture SQL for 3.1 user schema
4. Deploy: `docker compose down vtn && docker compose up --build -d vtn` (DB wiped, migrated)
5. Verify VTN responds to `POST /auth/token` with new fixture credentials

### Phase 2 — BFF
6. Rewrite `VTN/bff/src/vtn_client.rs` for single credential
7. Update `VTN/bff/src/config.rs` and route handlers
8. Deploy BFF; verify `GET /programs`, `GET /vens` etc. via BFF

### Phase 3 — VEN App
9. Update `VEN/src/models.rs`, `vtn.rs`, `reporter.rs` for 3.1 wire format
10. Add `client_id` to VEN YAML profiles and `VEN/docker-compose.yml`
11. Rewrite `VEN/src/simulator/` and `VEN/src/reactor/`
12. Implement VEN self-registration on startup
13. Deploy 3 VEN instances; verify self-registration and event polling

### Phase 4 — UIs
14. Update `VTN/ui/` for 3.1 types (program form, VEN list, reports)
15. Update `VEN/ui/` for 3.1 types (programs, reports)
16. Deploy both UIs; smoke-test in browser

### Phase 5 — Seed + Tests
17. Rewrite `scripts/seed_vtn.py` for 3.1 provisioning
18. Run seed; verify programs and events visible to correct VENs
19. Update integration test steps for new auth, enrollment, and report assertions
20. Run full BDD suite; fix failures
21. Update `docs/project_journal.md` and `docs/KEY_LEARNINGS.md`

### Rollback
Not applicable — this is a lab environment with no production traffic. If migration fails,
re-checkout the `dev` submodule branch and redeploy.

## Open Questions

- **Q1**: Does the upstream `openadr3_1` branch include a user management API (`/users`, `/users/{id}/credentials`)?
  The fixture SQL adds users directly. We need to confirm whether the admin UI / seed script can
  create users via API or must use fixture SQL injection.

- **Q2**: Should the VEN simulator redesign retain the same device types (PV, heater, EV, load)
  or simplify to fewer devices? Agreed to redesign completely — decide device set during implementation.

- **Q3**: The `write_subscriptions` scope is in the VEN fixture. Do we create any subscriptions
  in the seed data, or leave that for a future MQTT phase?
