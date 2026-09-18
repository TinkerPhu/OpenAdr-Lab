## Why

The OpenADR Alliance published version 3.1.0, which is not backwards-compatible with 3.0.1.
Upstream `openleadr-rs` merged 3.1 into **`main`** on 2026-03-13 (`Upgrade to OpenADR 3.1`,
#313) and has shipped six further months of work on top of it. Our submodule forked from
`823a475` (2026-02-20), three weeks before that merge, and now sits **179 upstream commits
behind**.

Staying on 3.0 keeps the lab on a dead fork point: it costs us every upstream fix, blocks
subscriptions and native MQTT notifiers, and forces us to maintain privacy patches (#372,
#374) that 3.1's `clientID` model supersedes outright.

This change is a **rebase of our fork onto `upstream/main`**, not a branch switch.
See `REVIEW.md` for the evidence behind each statement below.

## What Changes

**Submodule and build**

- **BREAKING** Rebase `TinkerPhu/openleadr-rs` onto `upstream/main` (3.1); update the submodule pointer
- **BREAKING** `internal-oauth` is no longer an upstream default feature — the VTN build must
  request it explicitly, or `POST /auth/token` and the whole `/users` tree do not exist
- Re-port the three lab patches upstream does not have: the `?active=` event filter (GB-04,
  a safety *and* efficiency fix), report cascade-delete on event deletion, and the
  cargo-chef/BuildKit Docker build
- Retire the VEN_NAME target reconstruction patches (#372, #374) — 3.1 implements target hiding
  natively — while porting their five privacy tests forward

**Wire format**

- **BREAKING** Auth model replaced: roles (business/ven-manager/user-manager) → scopes, with a
  business/VEN split (`write_vens_bl` vs `write_vens_ven`, same for reports and subscriptions)
- **BREAKING** Targets flatten: `[{type:"VEN_NAME", values:["ven-1"]}]` → `["ven-1"]`
- **BREAKING** `ven_program` enrollment table dropped; enrollment is implicit via `targets[]`
- **BREAKING** `VEN` gains a mandatory `clientID` linking it to its OAuth credential
- **BREAKING** Reports lose `programID` and `venID`; `eventID` becomes the only object link and
  `clientID` is VTN-provisioned from the token
- **BREAKING** Programs drop `programType`, `country`, `principalSubdivision`, `retailerName`,
  `retailerLongName`, `programLongName`, `bindingEvents`, `localPrice`, `businessId`; gain `attributes`
- **BREAKING** Events gain `duration`; `intervals` becomes **optional**; `targets` becomes a
  non-optional flat array; `EventContent` → `EventRequest`
- `ReportDescriptor` gains a required `reportIntervals` (`INTERVALS | SUB_INTERVALS | OPEN_INTERVALS`)
- `EventType`, `ReportType`, `ReadingType` and `Unit` are **unchanged** — the GB-50 contract
  design carries over intact

**Lab**

- BFF simplifies from dual-credential to a single business credential with explicit `*_bl` scopes
- VEN self-registers via `VenVenRequest` on startup, using its own token
- Seed script and fixtures updated for scopes, clientId identity and flat targets — for all
  **20** VENs across Node1 and Node2
- Integration tests updated for the new auth, enrollment and report wire format
- Both UIs updated for the 3.1 wire format

## Non-Goals

- **Adopting subscriptions / native MQTT notifiers.** Upstream now ships `/subscriptions` plus
  `/notifiers/{ws,mqtt,push-mqtt}`, and `paho-mqtt` is a hard dependency of `openleadr-vtn`.
  Polling is retained *for this change*, but the fleet-monitor MQTT decision must be revisited
  once the rebase lands — it is deferred, not dismissed.
- Resource groups (upstream targeting concept the lab does not model yet)
- Compact representation of serial price data (issue #238)
- mDNS discovery (issue #315)
- Upstream PRs for our lab-specific additions
- Rewriting the VEN simulator. `VEN/src/simulator/` is the physics core and is untouched by
  3.1 — only `VEN/src/controller/openadr_interface.rs` reads event payloads.
- Migration of live data (fresh deploy only)

## Capabilities

### New Capabilities

- `vtn-core-31`: Rebase the fork onto `upstream/main`, fix the build feature set, run the migration
- `fork-patches-31`: Re-port the `active` event filter and report cascade-delete onto 3.1, with
  their regression tests; retire the obsolete VEN_NAME privacy patches
- `scope-auth`: Scope-based auth — fixture SQL, BFF single-credential refactor, VEN token scopes
- `client-id-ven-identity`: `clientID` as VEN identity; VEN self-registration; provisioning sequence
- `flat-targets`: Flat `text[]` targets across programs, events, vens, resources; privacy via clientId
- `program-schema-31`: 3.1 program wire format (drop deprecated fields, add `attributes`)
- `event-schema-31`: 3.1 event wire format (`duration`, optional `intervals`, flat targets) and the
  single consolidated interval-timing rule that follows from it
- `report-schema-31`: 3.1 report wire format (`eventID` only, `clientID`, `reportIntervals`)
- `seed-script-31`: Seed and fixtures for the 3.1 auth model and flat targets, at fleet scale (20 VENs)
- `integration-tests-31`: BDD scenarios for the new auth, enrollment, and report assertions
- `vtn-ui-31`: VTN UI for the 3.1 wire format
- `ven-ui-31`: VEN UI for the 3.1 wire format

### Modified Capabilities

<!-- No existing openspec specs exist yet; all capabilities above are net-new. -->

## Impact

- **openleadr-rs** (submodule): fork rebased onto `upstream/main`; #372/#374 dropped; the
  `active`-filter and cascade-delete patches re-applied; `vtn.Dockerfile` build features corrected
- **VTN Docker Compose** (`VTN/`): new fixture SQL; migrations `20260213100612_openadr_3.1.sql`
  onward run on first boot
- **BFF** (`VTN/bff/`): credential model — `config.rs`, `vtn_client.rs`, `routes/`
- **VTN UI** (`VTN/ui/`): `api/hooks.ts`, `Programs.tsx`, `Vens.tsx`, `Reports.tsx`,
  `ProgramFormDialog.tsx`, `EventFormDialog.tsx`
- **VEN** (`VEN/src/`): `controller/vtn_port.rs` (DTOs), `vtn.rs`, `controller/reporter.rs`,
  `controller/openadr_interface.rs` (event interval timing)
- **VEN profiles / compose** (`VEN/profiles/`, `VEN/docker-compose.yml`,
  `VEN/scale_out/node2/docker-compose.yml`): clientId per VEN, across both hosts
- **VEN UI** (`VEN/ui/`): `api/hooks.ts`, `Programs.tsx`, `Reports.tsx`
- **Seed / provisioning** (`scripts/seed_vtn.py`, `tests/provision_ven.py`)
- **Integration tests** (`tests/features/`): auth, provisioning, enrollment and report steps

> **Trap:** `VEN_NAME` is also an environment variable (`VEN/docker-compose.yml`,
> `VEN/scale_out/node2/docker-compose.yml`, `VEN/src/config.rs`, `weather.rs`,
> `measurement.rs`). Of 115 occurrences across 35 lab files, many are the env var and must
> **not** be touched by the target-type migration.
