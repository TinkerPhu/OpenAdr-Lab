Phases are ordered so each is verifiable before the next starts. Per `test-first`, each phase
writes its failing test before its implementation. Prefer Node2 for builds and test runs
(`DOCKER_HOST=Node2`), holding the host lock for the whole sequence.

## 0. Submodule — rebase onto upstream/main and carry our patches

Nothing below phase 0 can be verified until the VTN boots with a token endpoint (D7).

- [x] 0.1 In `TinkerPhu/openleadr-rs`: `git fetch upstream`, branch `rebase/openadr3_1` from `upstream/main`
- [x] 0.2 Re-apply **P-3** `vtn.Dockerfile`: keep our 4-stage cargo-chef + BuildKit cache mounts and the fixed runtime `COPY`; take upstream's `rust:1.94-alpine`, dynamic-openssl deps and `RUSTFLAGS`; **keep `--features internal-oauth`** (D7)
- [ ] 0.3 Decide and record: disable `experimental-websockets` (upstream default; its own comment says object privacy is not implemented) unless a scenario needs it
- [x] 0.4 Re-apply **P-2** report cascade-delete migration against the 3.1 `report` table (`event_id` is now the only object link)
- [x] 0.5 Port **P-1** `EventContent::ends_at()` → `EventRequest::ends_at()` as longest-end-wins
      (D9): max over the event-level `intervalPeriod.duration`, the new top-level `duration` and
      the per-interval ends; undeterminable counts as unbounded and wins. Port all 6 unit tests
      first and watch them fail — each must pass unchanged under the new rule
- [x] 0.6 Add tests for the cases 3.0 could not express: intervals outlasting `duration`,
      `duration` outlasting intervals, and the intentional loss of the event-level short-circuit
- [x] 0.7 Port **P-1** `event.ends_at` migration + index + backfill onto the 3.1 schema
- [x] 0.8 Port **P-1** SQL-side active filtering into 3.1's `retrieve_all_{with,without}_client_id`, keeping the filter **inside** the query that does `OFFSET/LIMIT` (the original bug), and `ends_at` in sync on insert/update
- [x] 0.9 Port **P-1**'s 3 sqlx tests, including `active_filter_combined_with_pagination` (the regression test)
- [x] 0.10 Retire **P-4** — nothing to drop: branching from `upstream/main` means the lab's
      `strip_ven_name_targets` and VEN_NAME reconstruction simply never existed here
- [x] 0.11 **P-4 tests: verified, no port needed.** Upstream's own suite already covers all five
      properties, and porting ours would have duplicated them: redaction via
      `without_targets(event_4(), ["target-1"])` in `filter_target_get_all_ven_client`;
      not-found via `AppError::NotFound` in `get_as_ven_client`; list filter+strip in the same
      test; business sees all 5 with full targets in `filter_target_get_all_bl_client`;
      empty-targets visible to every VEN via `event_5`
- [ ] 0.12 Regenerate the sqlx offline cache; `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` all green in the submodule **alone**
- [ ] 0.13 Push `rebase/openadr3_1`; update the lab submodule pointer; commit
- [ ] 0.14 Verify `git submodule status` on Node1 and Node2 after pull

## 1. VTN core — fixtures, deploy, smoke

- [x] 1.1 Write fixture SQL for the scope model — `VTN/fixtures/01_bl_client.sql`, every scope
      spelled out, no aliases (D2). Upstream's own `fixtures/users.sql` independently confirms
      the reading: their `bl-client` also uses `write_vens_bl`, not the alias
- [ ] 1.2 Create the **ven-1 … ven-20** users via `POST /users` + `POST /users/{id}` from the
      seed script rather than fixture SQL, so the VTN hashes each secret itself and no argon2
      hash is hand-maintained (answers 4.3). Scopes `read_targets read_ven_objects
      write_reports_ven`, client ids unchanged at `ven-N` (D3, D11)
- [ ] 1.3 Update `VTN/docker-compose.yml` to mount the new fixtures
- [ ] 1.3b Drop **only** the `public` schema on `vtn-db-1`; leave `lab_recorder` intact (D1
      scope correction — 1.33M telemetry rows live there and no OpenADR migration touches them)
- [ ] 1.3a **Back up the VTN database before anything destructive** — `pg_dump` of `vtn-db-1`
      (including the `lab_recorder` schema) to a file outside the repo, verified non-empty and
      restorable. This is a hard gate: no wipe happens until the dump exists.
- [ ] 1.4 Deploy VTN (⚠️ wipes DB); confirm migrations `20260213100612_openadr_3.1.sql` onward applied
- [ ] 1.5 **Smoke (D7)**: `POST /auth/token` with `bl-client` returns 200. If this 404s, the build lost `internal-oauth` — stop and fix 0.2
- [ ] 1.6 Smoke: `POST /auth/token` for a sample of VEN credentials across both hosts
- [ ] 1.7 Smoke: `GET /programs` with the `bl-client` token returns 200 and an empty array
- [ ] 1.8 Assert the schema: `targets text[]` on program/event/ven/resource; `ven.client_id NOT NULL` + unique index; no `ven_program`; no role tables

## 2. BFF — single credential

- [x] 2.1 `config.rs`: drop `VTN_VEN_MGR_*`; single `VTN_BL_CLIENT_ID` / `VTN_BL_CLIENT_SECRET`
- [x] 2.2 One client, one token cache — `vtn_client.rs` needed no change; the second client was
      only ever a second `VtnClient::new` in `main.rs`
- [x] 2.3 Remove dual-client switching — `main.rs` (AppCtx), `recorder.rs` (ven snapshots) and
      `routes/vens.rs`; zero `ven_mgr` references remain
- [x] 2.4 Update BFF env vars in `VTN/docker-compose.yml` and `tests/docker-compose.test.yml`
- [ ] 2.5 `cargo test -p` the BFF; fmt + clippy green
- [ ] 2.6 Deploy; verify `GET /api/{programs,events,vens,reports}` all 200

## 3. VEN app — wire boundary and self-registration

Simulator untouched (D5). `POST /sim/override` stays.

- [ ] 3.1 Adopt `openleadr-wire` in the VEN (D12), **incrementally, events first**. Measured
      2026-09-18: 14 files reference `vtn_port`, with ~184 field-access sites, so this is
      sequenced rather than done in one pass:
      - [ ] 3.1a Add `openleadr-wire` as a path dependency with **default features** (no sqlx,
            verified: the crate builds with sqlx compiled zero times)
      - [ ] 3.1b Swap the event types first — `OadrEvent`/`OadrInterval`/`OadrIntervalPeriod`/
            `OadrPayload` → wire `Event`/`EventInterval`/`IntervalPeriod`/`EventValuesMap`. This is
            where most of the 184 sites are and where the churn is mechanical but wide: fields move
            behind `.content` and go snake_case (`event.programID` → `event.content.program_id`)
      - [ ] 3.1c Update `services/test_support/mock_vtn.rs` fixtures — the wire `Event` requires
            `id`, `createdDateTime` and `modificationDateTime`, which the lenient DTO did not
      - [ ] 3.1d Reports last: `OadrReportBody` → wire `ReportRequest` — drops `programID`, makes
            `eventID` required (R7), and brings `reportIntervals` and payload descriptors with it
- [ ] 3.2 `controller/event_timing.rs` (the existing authority): extend `timed_intervals` for
      3.1 — top-level `duration` and absent `intervals`; surface, never silently default
      (`wire-contracts`). Align its event-level end with `EventRequest::ends_at()` (D9)
- [ ] 3.3 Confirm the three callers (`openadr_interface.rs`, `rate_schedule.rs`, `reporter.rs`)
      still go through it, and that no new copy appeared. `report_intervals.rs` builds outgoing
      report intervals and is a different concept — leave it alone (D9)
- [ ] 3.4 `vtn.rs`: `POST /vens` self-registration on startup with `VenVenRequest`; treat 409 as already-registered, log INFO (R4)
- [ ] 3.4a Fold **GB-49** in here rather than fixing it twice: after obtaining a token, decode
      its `scope`/`roles` claim and fail the health check if the expected VEN scopes are absent,
      instead of polling successfully and seeing an empty world. Doing this against 3.0 roles
      first would mean rewriting it for scopes immediately afterwards
- [ ] 3.5 `controller/reporter.rs`: set `eventID` from the triggering event; drop `programID`
- [ ] 3.6 Declare `reportIntervals` on every report descriptor we emit — answer Q4 rather than inheriting the default (`wire-contracts`)
- [ ] 3.7 Add `client_id` to the VEN YAML profiles (all 20) and `CLIENT_ID` to both compose files
- [ ] 3.8 `wsl cargo test -p ven-app -j 2` green; fmt + clippy; `scripts/audit_file_sizes.py` passes
- [ ] 3.9 Deploy VENs on Node1 and Node2; verify each self-registered with **its own** `clientID`, not `bl-client` (R5)

## 4. Seed, provisioning and the profile contract

- [ ] 4.1 Rewrite `scripts/seed_vtn.py`: authenticate as `bl-client`, flat `targets: ["ven-1", …]`.
      Remaining `any-business`/`ven-manager` callers to migrate with it (inventory taken
      2026-09-18): `scripts/seed_vtn.py`, `scripts/fleet_status.py`, `scripts/db_reset.sh`,
      `experiments/run_experiment.py` (4 sites), `tests/failure_recovery_test.sh`,
      `tests/features/environment.py`
- [ ] 4.2 Update `tests/provision_ven.py` for scopes, keeping credentials-last ordering (GB-49)
- [x] 4.3 **Decided: API, with a one-user SQL bootstrap.** Only `bl-client` can live in SQL
      (nothing can call `/users` without a token), so it is the single fixture; every VEN user
      is created through the API. Requires the `internal-oauth` build (D7)
- [ ] 4.4 Define the GB-50 profile pointer as a namespaced, versioned private `attributes` entry on the program, plus the published profile document it points at (D10)
- [ ] 4.5 Seed `payloadDescriptors` on programs/events and `reportDescriptors` carrying payload type, units, readingType (`wire-contracts`); no value goes out undeclared
- [ ] 4.6 Run the seed; verify per-VEN visibility for a targeted and an open program
- [ ] 4.7 Verify target hiding: a VEN sees only its own id in a program's `targets`, never another VEN's (D8 P-4)

## 5. UIs

- [ ] 5.1 `VTN/ui/src/api/hooks.ts`: `targets: string[]`; drop deprecated program fields; report `eventID` not `programID`
- [ ] 5.2 `ProgramFormDialog.tsx`: remove `programType`/`country`/`bindingEvents`/`localPrice`/`retailerName`; surface `attributes` including the profile pointer (D10, `ui-transparency`)
- [ ] 5.3 `EventFormDialog.tsx`: flat targets; expose `duration`
- [ ] 5.4 `Vens.tsx` (clientID column), `Programs.tsx` (flat targets), `Reports.tsx` (eventID)
- [ ] 5.5 `VEN/ui`: same type updates; `Programs.tsx`, `Reports.tsx`
- [ ] 5.6 `npm run build` + `npm test` + eslint clean, both UIs
- [ ] 5.7 Deploy both; smoke-test in browser

## 6. Integration tests

- [ ] 6.1 Auth steps: scope-based tokens replacing role-based
- [ ] 6.2 Provisioning steps: self-registration flow; scenario for happy path **and** 409 idempotency
- [ ] 6.3 Enrollment steps: flat `targets: ["ven-1"]`
- [ ] 6.4 Report steps: assert `eventID` present, `programID` absent, `clientID` matches the submitting VEN
- [ ] 6.5 Scenario: each VEN's `clientID` equals its own credential, never `bl-client` (R5)
- [ ] 6.6 Scenario: `?active=` with pagination returns a correct page (the P-1 regression, at BDD level)
- [ ] 6.7 Scenario: target hiding — a VEN cannot read another VEN's target list
- [ ] 6.8 Full suite on Node2: `DOCKER_HOST=Node2 bash run_all_tests.sh` — all four suites green
- [ ] 6.9 Fix the `VEN_NAME` env-var vs target-type distinction wherever a step touched it (R6)

## 7. Documentation and close-out

- [ ] 7.1 Journal the migration in `docs/history/project_journal.md` (what, why, issues)
- [ ] 7.2 Key learnings into `docs/reference/KEY_LEARNINGS.md`: the scope alias trap, the `internal-oauth` Dockerfile collision, clientId identity, flat targets
- [ ] 7.3 Wave mechanism-level facts into `docs/architecture/VTN_ARCHITECTURE.md` and `VEN_ARCHITECTURE.md`; user-observable behaviour into `docs/use-cases/`
- [ ] 7.4 Close GB-50 (or record what remains) now that descriptors are on the wire
- [ ] 7.5 Re-decide the fleet-monitor MQTT question against native subscriptions/notifiers (Q3, proposal Non-Goals)
- [ ] 7.6 Delete this change directory per workflow rule 3; clean up merged checkouts/worktrees on both hosts
