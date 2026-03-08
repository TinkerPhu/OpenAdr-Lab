## Why

The OpenADR Alliance published version 3.1.0, which is not fully backwards-compatible with 3.0.1.
The upstream `openleadr-rs` library already has an `openadr3_1` branch with all-green CI (`v0.2.0-rc1`).
Staying on 3.0 leaves the lab behind the spec and requires maintaining our own privacy fixes (#372, #374)
that are now superseded by the cleaner 3.1 native model.

## What Changes

- **BREAKING** Switch `openleadr-rs` submodule from `dev` (3.0 + our patches) to `openadr3_1` branch
- **BREAKING** Auth model replaced: role-based (business/ven-manager/user-manager) → scope-based (`read_all`, `read_targets`, `read_ven_objects`, `write_*`)
- **BREAKING** Target representation changed: `[{type:"VEN_NAME", values:["ven-1"]}]` → flat `text[]` of clientIds
- **BREAKING** `ven_program` enrollment table dropped; enrollment is now implicit via program `targets[]`
- **BREAKING** `VEN` object gains mandatory `clientID` field linking it to its OAuth credential
- **BREAKING** Reports lose `programId` and `venId`; gain `clientId`; `eventId` replaces program link
- **BREAKING** Program schema drops deprecated fields (`programType`, `country`, `retailerName`, `bindingEvents`, `localPrice`, `businessId`); gains `attributes`
- BFF simplifies from dual-credential to single `bl-client` credential with write scopes
- VEN provisioning switches to self-registration flow (`VenVenRequest` via VEN's own token)
- VEN simulator and reactor redesigned from scratch (new 3.1-compatible device models)
- Seed script rewritten for new scope/user/VEN provisioning model
- Integration tests updated for new auth flow, enrollment model, and report wire format
- Both UIs updated for 3.1 wire format changes

## Non-Goals

- MQTT pub/sub support (polling remains; MQTT deferred to a future change)
- Compact representation of serial price data (issue #238; future change)
- mDNS discovery support (issue #315; future change)
- Upstream PRs for our 3.1-specific lab additions
- Migration of live production data (fresh deploy only)

## Capabilities

### New Capabilities

- `vtn-core-31`: Switch submodule to `openadr3_1`, update Docker build, run DB migration
- `scope-auth`: New scope-based auth model — user fixture SQL, BFF single-credential refactor, VEN token scopes
- `client-id-ven-identity`: VEN object now carries `clientID`; VEN self-registers; provisioning sequence updated
- `flat-targets`: Flat `text[]` targets across programs, events, vens, resources; privacy filtering via clientId match
- `program-schema-31`: Updated program wire format (drop deprecated fields, add `attributes`)
- `report-schema-31`: Updated report wire format (drop `programId`/`venId`, add `clientId`)
- `ven-simulator-31`: Redesigned VEN simulator and reactor for 3.1 (new device models, clean FSM, profile format)
- `seed-script-31`: Rewritten seed script for 3.1 auth model, clientId-based VEN provisioning, flat targets
- `integration-tests-31`: Updated BDD scenarios for new auth, enrollment, and report assertions
- `vtn-ui-31`: VTN UI updated for 3.1 wire format (program form, VEN list, enrollment display, reports)
- `ven-ui-31`: VEN UI updated for 3.1 wire format (programs, reports, device state)

### Modified Capabilities

<!-- No existing openspec specs exist yet; all capabilities above are net-new. -->

## Impact

- **openleadr-rs** (submodule): Branch switch + submodule pointer update; our PRs #372/#374 become obsolete
- **VTN Docker Compose** (`VTN/`): New fixture SQL; migration `20260213100612_openadr_3.1.sql` runs on first boot
- **BFF** (`VTN/bff/`): Full rewrite of credential model; `vtn_client.rs`, `config.rs`, `routes/`
- **VTN UI** (`VTN/ui/`): `api/hooks.ts` type updates; `Programs.tsx`, `Vens.tsx`, `Reports.tsx`, `EventFormDialog.tsx`
- **VEN** (`VEN/src/`): `models.rs`, `reporter.rs`, `vtn.rs` for 3.1 types; full rewrite of `simulator/` and `reactor/`
- **VEN profiles** (`VEN/profiles/`): Add `client_id` field per VEN
- **VEN Docker Compose** (`VEN/`): Add `CLIENT_ID` env var per VEN instance
- **VEN UI** (`VEN/ui/`): `api/hooks.ts` type updates; `Programs.tsx`, `Reports.tsx`
- **Seed script** (`scripts/seed_vtn.py`): Complete rewrite
- **Integration tests** (`tests/`): Provisioning, enrollment, and report step definitions rewritten
