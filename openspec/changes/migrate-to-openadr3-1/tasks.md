## 1. Submodule — Switch to openadr3_1 branch

- [ ] 1.1 Create `openadr3_1` branch in `TinkerPhu/openleadr-rs` fork from `upstream/openadr3_1`
- [ ] 1.2 Push the branch to origin (`TinkerPhu/openleadr-rs`)
- [ ] 1.3 Update the lab submodule pointer to `TinkerPhu/openleadr-rs@openadr3_1` and commit
- [ ] 1.4 Verify `git submodule status` reflects the new commit hash on Pi4-Server after `git pull`

## 2. VTN — Fixture SQL and Docker deploy

- [ ] 2.1 Write new `VTN/fixtures/users.sql` for 3.1 scope model: `bl-client` (read_all + write scopes) and `ven-1-client`, `ven-2-client`, `ven-3-client` (VEN scopes)
- [ ] 2.2 Write `VTN/fixtures/credentials.sql` with hashed secrets for all four clients
- [ ] 2.3 Update `VTN/docker-compose.yml` to mount new fixture SQL instead of old role-based fixtures
- [ ] 2.4 Deploy VTN on Pi4: `docker compose down vtn && docker compose up --build -d vtn` (⚠️ wipes DB)
- [ ] 2.5 Validate: `POST /auth/token` with `bl-client`/`bl-client` returns a token (HTTP 200)
- [ ] 2.6 Validate: `POST /auth/token` with `ven-1-client`/`ven-1-client` returns a token (HTTP 200)
- [ ] 2.7 Validate: `GET /programs` with `bl-client` token returns HTTP 200 with empty array

## 3. BFF — Simplify to single credential

- [ ] 3.1 Remove `VEN_MANAGER_CLIENT_ID` / `VEN_MANAGER_CLIENT_SECRET` from `VTN/bff/src/config.rs`; add single `BL_CLIENT_ID` / `BL_CLIENT_SECRET`
- [ ] 3.2 Rewrite `VTN/bff/src/vtn_client.rs` to use a single `VtnClient` with `bl-client` credential
- [ ] 3.3 Remove dual-client route switching in `VTN/bff/src/routes/`; all routes use the single client
- [ ] 3.4 Update `VTN/docker-compose.yml` BFF env vars (`BL_CLIENT_ID=bl-client`, `BL_CLIENT_SECRET=bl-client`)
- [ ] 3.5 Deploy BFF on Pi4: `docker compose up --build -d bff`
- [ ] 3.6 Validate: `GET /api/programs`, `GET /api/events`, `GET /api/vens` all return HTTP 200 via BFF

## 4. VEN App — Wire format and self-registration

- [ ] 4.1 Update `VEN/src/models.rs`: `targets: Vec<String>` (not TargetMap), remove `programId`/`venId` from Report, add `eventID` and `clientName`
- [ ] 4.2 Update `VEN/src/vtn.rs`: use `VenVenRequest` for `POST /vens` self-registration on startup
- [ ] 4.3 Implement self-registration logic: call `POST /vens` on startup; handle HTTP 409 as "already registered"
- [ ] 4.4 Update `VEN/src/reporter.rs`: remove `programId`, set `eventID` from triggering event, use `clientName` from profile `venName`
- [ ] 4.5 Add `client_id` field to `VEN/profiles/ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`
- [ ] 4.6 Add `CLIENT_ID` env var to each VEN service in `VEN/docker-compose.yml`
- [ ] 4.7 Validate Rust compilation: `cargo build` in `VEN/` succeeds with no errors

## 5. VEN Simulator and Reactor — Redesign

- [ ] 5.1 Delete `VEN/src/simulator/` directory contents and `VEN/src/reactor/` directory contents
- [ ] 5.2 Implement new `VEN/src/simulator/mod.rs`: device state structs (power kW per device), tick update function, `GET /sim` response type
- [ ] 5.3 Implement device models in simulator: base load, PV (sin curve), flexible device (setpoint-driven)
- [ ] 5.4 Implement sim state persistence: atomic write to `/data/sim_state.json` on each tick
- [ ] 5.5 Implement new `VEN/src/reactor/mod.rs`: FSM (Idle → Ramping → Holding → RampingBack → Idle), process 3.1 event interval payloads
- [ ] 5.6 Implement decision trace: append entry on each state transition; expose via `GET /trace`
- [ ] 5.7 Wire simulator and reactor into the VEN main polling loop
- [ ] 5.8 Remove `POST /sim/override` endpoint
- [ ] 5.9 Run `cargo test` in VEN; fix any compilation or unit test failures

## 6. VEN Deploy and Validate

- [ ] 6.1 Deploy 3 VEN instances on Pi4: `docker compose up --build -d` (⚠️ full rebuild ~11 min)
- [ ] 6.2 Validate: each VEN self-registers — `GET /vens` (bl-client token) shows 3 VEN objects with correct `clientID` values
- [ ] 6.3 Validate: `GET /sim` returns device state on each VEN (ports 8211, 8212, 8213)
- [ ] 6.4 Validate: `GET /trace` returns an array on each VEN

## 7. Seed Script — Rewrite for 3.1

- [ ] 7.1 Rewrite `scripts/seed_vtn.py`: authenticate as `bl-client`, create 3 programs with flat `targets[]`
- [ ] 7.2 Seed "Summer Peak DR" with `targets: ["ven-1-client", "ven-2-client"]`
- [ ] 7.3 Seed "EV Managed Charging" with `targets: ["ven-2-client", "ven-3-client"]`
- [ ] 7.4 Seed "HVAC Optimization" with `targets: []` (open to all)
- [ ] 7.5 Add idempotency: skip program/event creation if already exists (check by `programName`)
- [ ] 7.6 Create at least one event per program with valid 3.1 interval and SIMPLE payload
- [ ] 7.7 Run seed script; validate programs appear correctly via `GET /programs`
- [ ] 7.8 Validate: `GET /programs` with `ven-1-client` token returns only "Summer Peak DR" and "HVAC Optimization"
- [ ] 7.9 Validate: `GET /programs` with `ven-3-client` token returns only "EV Managed Charging" and "HVAC Optimization"

## 8. VTN UI — Update for 3.1 wire format

- [ ] 8.1 Update TypeScript types in `VTN/ui/src/api/hooks.ts`: `targets: string[]`, remove deprecated program fields, remove `programId` from report type
- [ ] 8.2 Update `ProgramFormDialog.tsx`: remove fields for `programType`, `country`, `bindingEvents`, `localPrice`, `retailerName`; add `attributes` field
- [ ] 8.3 Update `Vens.tsx`: show `clientID` column in VEN list
- [ ] 8.4 Update enrollment display: show flat `targets` string list for programs
- [ ] 8.5 Update `Reports.tsx`: replace `programId` column with `eventID` column
- [ ] 8.6 Run `npm run build` in `VTN/ui/`; fix TypeScript errors
- [ ] 8.7 Run VTN UI unit tests (`npm test`); fix failures
- [ ] 8.8 Deploy VTN UI on Pi4: `docker compose up --build -d ui`
- [ ] 8.9 Smoke-test in browser: create a program, view VENs with clientID, check reports page

## 9. VEN UI — Update for 3.1 wire format

- [ ] 9.1 Update TypeScript types in `VEN/ui/src/api/hooks.ts`: `targets: string[]`, remove `programId` from report type
- [ ] 9.2 Update `Programs.tsx`: display `targets` as flat string list
- [ ] 9.3 Update `Reports.tsx`: show `eventID` instead of `programId`
- [ ] 9.4 Update `Simulation.tsx`: adapt to new `GET /sim` response shape from redesigned simulator; remove override controls
- [ ] 9.5 Run `npm run build` in `VEN/ui/`; fix TypeScript errors
- [ ] 9.6 Run VEN UI unit tests (`npm test`); fix failures
- [ ] 9.7 Deploy VEN UI on Pi4: `docker compose up --build -d ui`
- [ ] 9.8 Smoke-test in browser: view programs (enrolled), reports (eventID), simulation page

## 10. Integration Tests — Update for 3.1

- [ ] 10.1 Update auth step definitions: replace role-based token acquisition with scope-based (`bl-client`, per-VEN credentials)
- [ ] 10.2 Update provisioning steps: remove old 4-step VEN provisioning sequence; replace with self-registration flow
- [ ] 10.3 Update enrollment steps: use flat `targets: ["ven-1-client"]` instead of `{type:"VEN_NAME", values:["ven-1"]}`
- [ ] 10.4 Update report assertion steps: assert `eventID` present, assert `programId` absent
- [ ] 10.5 Add scenario: "VEN self-registers against the VTN" (happy path + 409 idempotency)
- [ ] 10.6 Run full BDD suite on Pi4; fix failures
- [ ] 10.7 Verify all scenarios pass: enrollment, events, reports, privacy filtering

## 11. Documentation

- [ ] 11.1 Update `docs/project_journal.md` with migration steps, decisions, and key issues encountered
- [ ] 11.2 Update `docs/KEY_LEARNINGS.md` with 3.1-specific learnings (scope model, clientId identity, flat targets)
- [ ] 11.3 Update memory file `MEMORY.md` with new credential names, VEN provisioning sequence, and port/container table
