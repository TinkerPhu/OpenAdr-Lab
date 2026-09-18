## Context

Upstream `openleadr-rs` merged OpenADR 3.1 into `main` on 2026-03-13 (#313) and has shipped
six months of work since — subscriptions, resource groups, MQTT/WebSocket notifiers, and a
business/VEN scope split. Our submodule (`9588232`) forked at `823a475`, three weeks before
that merge: **179 upstream commits ahead, 37 ours behind**.

This design covers the rebase across all layers: submodule/build, VTN core, BFF, VEN app,
both UIs, and test/seed infrastructure. Evidence for each decision is in `REVIEW.md`.

The migration is a fresh-deploy strategy — no live data migration. Postgres is wiped and
re-created from the 3.1 schema on first boot; seed data is re-loaded.

## Goals / Non-Goals

**Goals:**
- Run the full lab stack against `upstream/main` (3.1) with all four test suites green
- Adopt the scope model natively, with the business/VEN split spelled out (no aliases)
- Replace VEN_NAME targeting with clientId targeting throughout
- Self-registering VEN flow (`POST /vens` with the VEN's own token)
- Carry our three still-needed fork patches forward with their tests (P-1..P-3), and keep P-4's safety tests
- Both UIs reflect the 3.1 wire format

**Non-Goals:**
- Subscriptions / native MQTT notifiers (deferred for this change, then re-decided — see Q3)
- Resource groups
- Rewriting the VEN simulator (see D5)
- Compact price representation (#238), mDNS (#315)
- Upstream PRs for lab-specific additions during this migration

## Decisions

### D1: Fresh deploy over live migration

**Decision**: Wipe the database and re-seed. Do not write SQL to transform 3.0 data to 3.1.

**Rationale**: The 3.1 migration drops 6 tables and changes 4 column types. `targets` goes from
JSONB to `text[]`; `ven_program` is dropped; the user/role tables are replaced by a `scope[]`
column. There is no meaningful row-level mapping from `{type, values}` JSONB to flat `text[]`.
A fresh seed reaches a known-good 3.1 state in minutes.

**Alternative considered**: A transform script. Rejected — high risk, no lab value.

**Scope correction — `lab_recorder` is NOT wiped.** "Wipe the database" was written before
anyone counted what is in it. The live `vtn-db-1` is **1.4 GB**, and the bulk of it is the
lab's own recorder schema, not OpenADR objects:

| schema | contents |
|---|---|
| `public` | openleadr's tables — 6 programs, 12 events, 21 reports, 25 VENs |
| `lab_recorder` | **1,337,302** `reports_received` rows, 301 `events_published`, 26 `ven_snapshots` |

`lab_recorder` is ours. No OpenADR migration touches it, 3.1 changes nothing about it, and it
is months of irreplaceable telemetry — the baseline any fleet-monitor work will be measured
against. Dropping the volume to get a clean 3.1 schema would destroy it as collateral.

So the destructive step is scoped to **the `public` schema only**: drop it (taking
`_sqlx_migrations` with it) and let the migrations rebuild it from scratch, leaving
`lab_recorder` in place. A full `pg_dump` and a separate `lab_recorder`-only dump are taken
first regardless, so the data survives even if the scoped drop goes wrong.

---

### D2: One business credential for the BFF, with `*_bl` scopes spelled out

**Decision**: Replace the dual-credential BFF (any-business + ven-manager) with a single
`bl-client` credential holding:

```
read_all  write_programs  write_events  write_vens_bl  write_reports_bl  write_users
```

**Every scope is written in full. The aliases are never used.**

**Rationale / the trap this avoids**: 3.1 splits three scopes by actor
(`openleadr-vtn/src/jwt.rs:123`):

| business | VEN | alias `write_*` resolves to |
|---|---|---|
| `write_vens_bl` | `write_vens_ven` | **`write_vens_ven`** |
| `write_reports_bl` | `write_reports_ven` | **`write_reports_ven`** |
| `write_subscriptions_bl` | `write_subscriptions_ven` | **`write_subscriptions_ven`** |

`api/ven.rs:79` branches on which one the caller holds:

- `WriteVensBl` → `clientID` is taken from the request body (what the BFF needs)
- `WriteVensVen` → `clientID` is **overwritten with the caller's own token `sub`** (`api/ven.rs:93`)

A BFF granted the bare alias `write_vens` would therefore create every VEN object with
`clientID = "bl-client"`; `ven_client_id_unique` rejects the second one, and the first is
addressable only as the BFF. This is why the fixture and the config must both name
`write_vens_bl` explicitly.

`read_all` and `read_targets` are mutually exclusive branches in every read handler
(`api/program.rs:34`, `api/event.rs:35`, `api/report.rs:36`, `api/ven.rs:34`): a `read_all`
holder bypasses target filtering entirely. That is correct for the BFF and wrong for a VEN.

**Alternative considered**: Keep dual credentials for organizational separation. Rejected —
the scope model is the right abstraction, and the split is now inside the scope names.

---

### D3: VEN identity is the existing `ven-N` client id

**Decision**: Each VEN authenticates with its own credential
(`read_targets`, `read_ven_objects`, `write_reports_ven`) and calls `POST /vens` with a
`VenVenRequest` on first startup. Its `clientID` stays **`ven-N`** — the id the fleet already
uses. No rename.

**Rationale**: 3.1 defines `VenVenRequest` (no `clientID` in the body — derived from the JWT
`sub`) against `BlVenRequest` (business layer sets `clientID` explicitly). Self-registration is
the spec's intended identity mechanism and removes a provisioning step.

`scripts/seed_vtn.py` already provisions `client_id: "ven-1" … "ven-20"`. 3.1 requires *a*
clientID; it does not require a new one. Renaming to `ven-N-client` would churn fixtures,
profiles, two compose files, BDD steps and docs across 20 VENs for no protocol benefit.

**VEN provisioning sequence (3.1)**:
```
1. bl-client creates the user with VEN scopes      (POST /users, or fixture SQL)
2. bl-client creates the user_credential            (client_id = "ven-7", secret)
3. VEN authenticates: POST /auth/token              → token, sub = "ven-7"
4. VEN calls POST /vens { venName: "ven-7" }        → VTN sets VEN.clientID = "ven-7"
5. bl-client sets program targets: ["ven-1","ven-7"]
   → each VEN sees only its enrolled programs via read_targets
```

Credential ordering stays load-bearing: the role/scope must exist *before* the credential is
issued, or the VEN caches a scope-less token and goes silently blind (GB-49,
`tests/provision_ven.py`).

**VEN profile update**: add `client_id` to the YAML profiles. `venName` remains a human label.

---

### D4: Flat text[] targets — clientId as the privacy address

**Decision**: Program, event, VEN and resource `targets` are `Vec<Target>` (newtype over a
plain string), non-optional, defaulting to `[]`. An empty array means "open to all VENs".

**Wire format**:
```json
// 3.0                                          // 3.1
"targets": [{"type":"VEN_NAME","values":["ven-1"]}]   →   "targets": ["ven-1"]
```

**Impact**: `TargetMap` and `TargetType` disappear. All code constructing or parsing
`{type, values}` is removed.

**Verified, not assumed** (the previous revision asserted this without citing a query):
`retrieve_all_with_client_id` in `openleadr-vtn/src/data_source/postgres/event.rs` filters on
`e.targets && $3 OR array_length(e.targets, 1) IS NULL` — the second disjunct is "empty targets
are visible to all" — and then redacts the returned list with
`intersection(&e.targets, &ven_targets)`, which is 3.1's native target hiding (D8, P-4).

---

### D5: The VEN simulator is out of scope

**Decision**: Do not touch `VEN/src/simulator/`. Do not remove `POST /sim/override`.

**Rationale**: The previous revision proposed deleting `VEN/src/simulator/` and
`VEN/src/reactor/` and rewriting both from scratch. `VEN/src/reactor/` does not exist.
`VEN/src/simulator/` is the physics core behind the MILP planner and the asset-competence
model, and it does not read OpenADR event structures at all — `VEN/src/controller/
openadr_interface.rs` is the only place event payloads are interpreted. The rewrite was the
largest block in the plan and addressed a coupling that isn't there.

`POST /sim/override` is used by the VEN UI and the BDD suites; removing it would break
scenarios unrelated to this migration and violate `no-half-built-features`.

What *does* change in the VEN is the wire boundary: `controller/vtn_port.rs`,
`vtn.rs`, `controller/reporter.rs`, and the interval-timing rule in D9.

---

### D6: Rebase the fork onto `upstream/main`

**Decision**: Rebase `TinkerPhu/openleadr-rs` onto `upstream/main` on a branch
`rebase/openadr3_1`, carrying forward only the patches that survive (D8). Point the lab
submodule at it.

**Rationale**: 3.1 is upstream mainline and has been since 2026-03-13. The `openadr3_1`
branch is a dead end whose last commit is "Release 0.2.0" (2026-03-13); `v0.2.0-rc1` is long
superseded (upstream is past `v0.2.6`). Targeting the branch would adopt 3.1 and simultaneously
strand us six months behind again.

```bash
cd openleadr-rs
git fetch upstream
git checkout -b rebase/openadr3_1 upstream/main
# re-apply the surviving patches (D8), then:
git push origin rebase/openadr3_1
```

---

### D7: Keep `--features internal-oauth` when re-applying our Dockerfile

**Decision**: the rebased `vtn.Dockerfile` keeps our four-stage cargo-chef/BuildKit structure
**and** upstream's `--features internal-oauth` flag, on upstream's base image and linking flags.

**Rationale**: `openleadr-vtn/src/state.rs:396` gates `POST /auth/token` and the entire `/users`
tree behind `internal-oauth`, and `Scope::WriteUsers` does not compile without it. Upstream
does not carry the feature in `default` — it passes the flag in its Dockerfile instead:

```
default = ["postgres", "compression-*", "experimental-websockets"]   # upstream 3.1
default = ["postgres", "live-db-test", "internal-oauth"]             # our 3.0 fork
```

Our fork **replaced that Dockerfile** (P-3) with a version that builds bare — correct today
only because our fork's `default` carries the feature. Rebasing removes that prop: our
Dockerfile wins, the feature goes off, and the VTN boots with no token endpoint, taking the
BFF, all 20 VENs, the seed script and every BDD auth step with it.

This is a collision between two of our own patches, which is the kind of thing a rebase hides.
Resolve it explicitly, taking one column from each side:

| | ours (`9588232`) | upstream/main | take |
|---|---|---|---|
| build structure | 4-stage cargo-chef + cache mounts | single stage | **ours** |
| base image | `rust:1.92-alpine` | `rust:1.94-alpine` | **upstream** |
| openssl | static | dynamic (`cmake g++ make openssl3-dev`, `RUSTFLAGS=-Ctarget-feature=-crt-static`) | **upstream** |
| features | *(none — relies on `default`)* | `--features internal-oauth` | **upstream** |
| runtime `COPY` | fixed path | `/app/openleadr-vtn/openleadr-vtn` (broken) | **ours** |

Nothing else in the plan can be verified until this is right, so it is the first task and gets
its own smoke check.

**Also decide**: `experimental-websockets` is in the upstream default set, and upstream's own
comment on it reads *"object privacy is not yet implemented"*. Disable it unless a scenario
needs it — a default-on transport that skips privacy filtering is not something the lab should
ship by accident.

---

### D8: Fork patch disposition

**Decision**: The fork's four functional patches are carried, adapted or retired individually,
per the audit in `REVIEW.md` §G-1. None of this was in the previous revision, which mentioned
only #372/#374.

The fork diff against the merge base is **14 files, +576/-163**. What it contains:

| # | Patch | Kind | On `upstream/main`? | Disposition |
|---|---|---|---|---|
| P-1 | GB-04 active-event filter (`ends_at` column + `EventContent::ends_at()` + SQL filtering) | safety + efficiency | **No** | **Re-port, adapted** (see below) |
| P-2 | Report cascade-delete on event delete (`89fdb90`) | safety | **No** | **Re-port** as-is |
| P-3 | `vtn.Dockerfile` cargo-chef + BuildKit cache mounts | efficiency | **No** | **Re-apply**, merged per D7 |
| P-4 | VEN_NAME target reconstruction + stripping (#372, #374) | safety | superseded natively | **Retire the code, port the tests** |

**P-1 is the one to be careful with.** It is not a flag — it is a migration
(`event.ends_at` + index + backfill), one authority function
`openleadr_wire::event::EventContent::ends_at()` with 6 unit tests, and SQL-side filtering with
3 sqlx tests. Its own migration comment records the bug it fixed: `?active=` combined with
pagination was *silently wrong*, because `LIMIT/OFFSET` ran before the then-Rust-side filter,
so a page could come back short or carry rows from the wrong page. Upstream/main still has no
`active` param at all and still paginates in SQL with no active predicate — so dropping this in
the rebase reintroduces a pagination correctness bug that no current test names.

The adaptation: 3.1 makes `intervals` optional and adds a top-level `duration` (D9).
`ends_at()` must gain that case. It stays **one** function — that is exactly why D9 consolidates
there rather than teaching each caller the new shape.

**P-4 is retired but not forgotten.** 3.1 implements target hiding natively in
`retrieve_all_with_client_id`, citing the spec in upstream's own code ("*Target hiding: … This
prevents VENs from learning targets that have not been explicitly assigned to them by BL*",
spec v3.1.1 Definition.md): it filters on
`e.targets && $3 OR array_length(e.targets,1) IS NULL` and then redacts via
`intersection(&e.targets, &ven_targets)`. That is our `strip_ven_name_targets` behaviour, done
by the spec. So the implementation goes — but the **five privacy tests come with us**
(`ven_in_targets_sees_event_stripped`, `ven_not_in_targets_gets_not_found`,
`ven_list_filters_and_strips`, `business_sees_full_targets`, `ven_sees_event_with_null_targets`),
rewritten against clientId targets. Retiring an implementation is fine; retiring the assertions
that prove the safety property still holds is not.

That same query is also the evidence D4 was missing: empty targets **do** mean visible to all.

All re-ports land in phase 0, *before* any lab-side change, so a failure is attributable to the
rebase rather than to the migration.

---

### D9: Event `duration` and optional `intervals` get one timing rule

**Decision**: The 3.1 event changes are absorbed in **one** place —
`VEN/src/controller/openadr_interface.rs` — and every caller uses it.

3.1 changes `EventRequest` beyond targets:

- new `duration: Option<Duration>` (and an `event.duration` DB column)
- `intervals: Vec<EventInterval>` → **`Option<Vec<EventInterval>>`**
- `targets: Option<TargetMap>` → `Vec<Target>`
- `EventContent` → `EventRequest` (same rename for `ProgramContent`, `ReportContent`)

**Rationale**: a conformant 3.1 event may arrive with a `duration` and **no intervals**. "When
does interval *i* of this event run" already had seven parsers with four rules once — that was
GB-48, the canonical `one-concept-one-function` failure in this project. Adding a second
legitimate shape (duration-only) to seven adapted copies would recreate it exactly.

**Inventory of where this concept lives today** — checked against the code, and the earlier
revision of this inventory (including the one in the first draft of this design) was wrong:

| module | role | action |
|---|---|---|
| `VEN/src/controller/event_timing.rs` | **already the single authority** (`timed_intervals`, `OPEN_START`, `OPEN_END`); its docstring states it is "the one answer every event parser uses" | **extend** for 3.1 |
| `VEN/src/controller/openadr_interface.rs` | caller | no timing logic to move |
| `VEN/src/controller/rate_schedule.rs` | caller | unchanged |
| `VEN/src/controller/reporter.rs` | caller | unchanged |
| `VEN/src/controller/report_intervals.rs` | builds **outgoing report** intervals — a different concept | **leave alone** |
| `openleadr_wire::event::EventRequest::ends_at()` | the VTN-side answer, shared once the VEN adopts the wire types (D12) | already done (D8, P-1) |

So GB-48 is already fixed: there is one authority, and three callers use it. This change does
**not** consolidate scattered copies — it extends the existing authority for two new 3.1 shapes
(a top-level `duration`, and `intervals` becoming optional). Treating `report_intervals.rs` as a
copy to merge, as the previous task list did, would have folded together two unrelated concepts.

Under `wire-contracts`, the duration-only case is read from what the event declares — never
defaulted silently. If we cannot place an interval, that is surfaced, not guessed.

**The rule when sources disagree**: an event's end instant is the **longest** of every end it
declares. 3.1 lets an event carry up to three: the event-level `intervalPeriod.duration`, the
new top-level `duration`, and the per-interval periods. An end that cannot be determined
(a missing duration, an interval with no period) counts as **unbounded**, and unbounded is the
longest of all — so it yields "always active", as today.

```
ends_at = max(all determinable candidate ends)      // None (unbounded) wins outright
        = None if any relevant candidate is unbounded, or if there are no candidates at all
```

*Why longest rather than "the top-level `duration` is authoritative":* `ends_at()` governs
**visibility and retention**, not control — dispatch is decided per interval by this same
authority. An over-long end therefore leaves an event visible with no applicable interval at
`now`, which is inert. An over-short end removes an event from `?active=true` **while its
intervals are still dispatching**, so the VEN stops seeing it on the next poll and drops a live
obligation. The costs are asymmetric; take the side that cannot lose control in flight.

This is not a tie-break bolted onto the existing logic — it is the generalisation the six
`ends_at` unit tests (D8, P-1) already encode, with `None` as the point at infinity. Each of
them reproduces under it unchanged, and the 3.1 top-level `duration` is simply one more
candidate.

**One intentional behaviour change**: today an event-level `intervalPeriod` short-circuits and
the intervals are never consulted. Under longest-wins, intervals extending past it now win.
That is the rule working as intended, but it differs from the 3.0 code, so it gets its own
test rather than arriving silently.

A disagreement between declared ends is still surfaced (`wire-contracts`: we apply a documented
rule *and say that we did*) — it usually means a malformed or ambiguous event worth seeing.

---

### D10: `attributes` carries the lab profile pointer (GB-50)

**Decision**: The wire-contract work lands **inside** this change, in
`program-schema-31`, not after it.

**Rationale**: 3.1 drops `programType` and adds `attributes: Option<Vec<ValuesMap>>`. GB-50's
design had been sketched around a program-level identifier for the lab profile — that field no
longer exists, so `attributes` is the only spec-native vehicle left, and the Program Attribute
Enumerations table in our 3.1 markdown copy is effectively empty. The profile pointer therefore
becomes a namespaced, versioned private attribute, additively, per `wire-contracts`
("protocol fields first … only what the protocol cannot express goes into a namespaced,
versioned extension").

Deferring this means shipping a second wire migration later for the same objects. Task 8.2 in
the previous revision said "add an `attributes` field" to a UI form with no statement of what
goes in it; this decision is what fills that in.

The good news: `Unit`, `EventType`, `ReportType` and `ReadingType` are **byte-identical**
between 3.0 and 3.1. `Unit` still has `KWH` and `KW` and still has no watt. Every GB-50
conclusion about quantities and units survives the migration unchanged.

---

### D11: Fleet scale is 20 VENs across two hosts

**Decision**: Every fixture, credential, self-registration and validation step covers
**ven-1 … ven-20**: ven-1..3 on Node1, ven-4..20 on Node2.

**Rationale**: the previous revision was written for three VENs and named only Node1.
`scripts/seed_vtn.py` provisions 20, and `VEN/scale_out/node2/docker-compose.yml` alone carries
18 `VEN_NAME` references. Node2 has its own docker host lock and its own checkout; a migration
plan that doesn't mention it leaves 17 VENs on a schema the VTN no longer serves.

### D12: The VEN adopts `openleadr-wire` types

**Decision**: The VEN depends on the submodule's `openleadr-wire` crate instead of its
hand-rolled DTOs in `VEN/src/controller/vtn_port.rs`.

**Rationale**: D9 requires one authority for event interval timing, but `ends_at()` lives in
`openleadr-wire` (VTN side) while the VEN parses its own minimal structs — two processes, so
"one function" cannot hold literally while the DTOs stay hand-rolled. Adopting the wire crate
makes it literal, and it is also what `wire-contracts` needs: `payloadDescriptors` and
`reportDescriptors` are invisible to the current DTOs, so the VEN cannot read the contract that
accompanies a value (GB-50). `vtn_port.rs`'s own header already anticipates this — it says the
minimal surface exists because "OpenADR 3.1 introduces breaking field/type name changes".

**Feasibility gate**: `openleadr-wire` carries `sqlx::Type` derives (`Target`, `ClientId`,
`VenId`). Before committing to this, verify they are feature-gated and that depending on the
crate does not pull `sqlx` into the VEN build. If they leak, fall back to hand-rolled DTOs with
the timing rule mirrored and pinned by a **shared test-vector fixture**, so the two copies
cannot drift silently — and record the exception in `TECHNICAL_DEBTS.md`.

**Layering**: `openleadr-wire` is an external protocol crate, so it enters at the infra/adapter
ring (`vtn.rs`, `controller/vtn_port.rs`), not in `entities/`. The `ven-architecture` dependency
rule still applies.

---

### D13: The VEN takes `openleadr-wire` as a git dependency, not a path dependency

**Decision**: `openleadr-wire = { git = "https://github.com/TinkerPhu/openleadr-rs", branch = "rebase/openadr3_1" }`
in `VEN/Cargo.toml`, with `Cargo.lock` pinning the exact commit.

**Rationale — the obvious approach does not build.** D12 has the VEN depend on the submodule's
wire crate, and the natural spelling is a path dependency
(`path = "../openleadr-rs/openleadr-wire"`). That cannot work in the container: the VEN's Docker
build context is `VEN/` (`context: .` in `VEN/docker-compose.yml`), and the submodule lives at
the repository root, **outside** it. `VEN/Dockerfile` copies only `Cargo.toml`, `Cargo.lock`,
`src/` and `profiles/`, so a path outside `VEN/` is unreachable at build time.

**Alternatives considered**:

| option | verdict |
|---|---|
| Move the build context to the repo root | Works, but pushes an 861 MB context (and a new `.dockerignore` regime) through 20 VEN service builds across two hosts, and edits three compose files. Real cost, no benefit over a git dep. |
| Depend on crates.io `openleadr-wire` | Not viable: the published 0.2.6 still pulls `sqlx` unconditionally — that is exactly what P-5 fixes. Available only once P-5 is upstream and released. |
| Vendor the crate into `VEN/` | A copy that silently drifts from the submodule. Rejected. |
| **Git dependency on our fork** | **Chosen.** Cargo already fetches dependencies over the network in the dependency-cache layer, so nothing about the Dockerfile changes; `Cargo.lock` pins the commit, so builds stay reproducible. |

**Consequence to accept**: a wire-crate change now needs a push plus `cargo update -p
openleadr-wire` in the VEN, rather than being picked up from the working tree. That is a mild
cost and arguably better hygiene — the VEN builds against a committed, pinned revision rather
than whatever the submodule happens to be checked out at.

**Switch to crates.io** once P-5 is upstreamed and released, and drop the git dependency then.

**Verified 2026-09-18** in a throwaway crate carrying nothing but this dependency: it resolves,
pins commit `d86b23fc`, and `EventRequest::ends_at()` is callable from outside the workspace. Its
`Cargo.lock` has **139** dependencies and **zero** occurrences of `sqlx` — no Postgres driver, no
rustls, no tokio. That is P-5, D12 and D13 confirmed together: the VEN can take the wire types
without a database stack and without touching its Docker build context.

---

## Risks / Trade-offs

**R1: Six months of upstream drift, not one branch switch**
→ 179 commits including subscriptions, resource groups and notifier transports. The rebase may
surface conflicts well beyond the 3.1 schema. Mitigation: phase 0 rebases and builds the
submodule *alone*, green, before any lab code changes.

**R2: Long rebuild after the rebase, and a heavier one than before**
→ New migrations and a changed Cargo.lock mean a full rebuild. 3.1 also makes `paho-mqtt-sys` a
hard dependency of `openleadr-vtn`, which compiles the bundled Paho C library with SSL through
cmake — so the VTN build now needs `cmake`, `g++` and `make` present, and takes noticeably
longer than the 3.0 build did. Upstream's Dockerfile already installs them, which is one more
reason D7 takes upstream's toolchain line rather than our old `openssl-libs-static` one.
Confirmed the hard way on 2026-09-18: a test container without cmake fails at
`paho-mqtt-sys v0.10.3` with "is `cmake` not installed?", ~200 crates into the build.
Mitigation: prefer Node2 (`DOCKER_HOST=Node2`), hold the host lock for the whole sequence, run
detached, and expect the first build after the rebase to be slow even with the cargo-chef cache.

**R3: Silent loss of the `active` filter**
→ The highest-consequence regression risk in this change (D8). Mitigation: re-port with the
regression test first, and assert it in a BDD scenario before touching lab code.

**R4: VEN self-registration race**
→ 20 VENs may `POST /vens` at once. `ven_client_id_unique` makes the second attempt a 409.
Mitigation: treat 409 as "already registered" and continue; log at INFO.

**R5: Scope alias mis-grant**
→ Granting `write_vens` instead of `write_vens_bl` silently produces VENs owned by the BFF
(D2). Mitigation: fixtures name scopes in full; a BDD scenario asserts each VEN's `clientID`
equals its own credential, not `bl-client`.

**R6: Blanket VEN_NAME rename breaks VEN startup**
→ `VEN_NAME` is also an env var in both compose files and three VEN source files. Mitigation:
no repo-wide search-and-replace; migrate target construction sites individually.

**R7: Report schema inversion**
→ Our `OadrReportBody` has `programID` mandatory and `eventID` optional — exactly inverted from
3.1, where `eventID` is the only object link. Every report must now name an event. Mitigation:
the standing `fleet-monitoring` program needs a standing *event*; assert `eventID` presence at
the boundary.

**R8: The 3.1 VTN panics at startup if MQTT is configured but unreachable**
→ 3.1 loads subscription notifiers during state construction
(`openleadr-vtn/src/state.rs:325`) and `.expect("failed to retrieve subscriptions from
database")` turns any broker failure into a panic — the observed error is
`Mqtt(TcpTlsConnectFailure)`. MQTT is opt-in: it activates only when **all** of `MQTT_URL`,
`MQTT_USERNAME` and `MQTT_PASSWORD` are set, and setting only some of them is itself a panic
("Incomplete MQTT configuration").

The trap is that upstream **tracks a `.env`** at the repository root with
`MQTT_URL=mqtt://localhost:1883`, and the VTN reads it through `dotenvy`. Anything that runs
with the repo as its working directory picks those up silently. That is what made the whole
test suite fail on first run here: 201 tests could not build app state until a broker was
reachable, with no hint in the failure that MQTT was involved.

→ Mitigation for the deploy: decide explicitly. Either point `MQTT_URL` at the lab's existing
Mosquitto on Node1, or leave all three unset so the notifier stays disabled — and make sure the
container's environment, not a stray `.env`, is what decides. Revisit when the subscription/MQTT
non-goal is re-opened (Q3).

## Migration Plan

Phases are ordered so each one is verifiable before the next begins. `tasks.md` holds the
step-level detail.

- **Phase 0 — Submodule**: rebase onto `upstream/main`; fix build features (D7); re-port the two
  surviving patches with their tests (D8). Submodule CI green *before* any lab change.
- **Phase 1 — VTN core**: fixtures for the scope model (D2, D11); fresh deploy; migration applied.
- **Phase 2 — BFF**: single credential with explicit `*_bl` scopes.
- **Phase 3 — VEN app**: DTOs, self-registration, report body, and the D9 interval-timing
  consolidation. Simulator untouched (D5).
- **Phase 4 — Seed and provisioning**: 20 VENs, flat targets, `attributes` profile pointer (D10).
- **Phase 5 — UIs**: both UIs for the 3.1 wire format.
- **Phase 6 — Tests and docs**: full suite on Node2; wave the change into `docs/` per workflow
  rule 3.

Per `test-first`, each phase writes its failing scenario before its implementation. Phase 6 is
where the suite must be *green*, not where the scenarios are first written.

### Rollback
Revert the submodule pointer to `9588232` and redeploy; the database must be restored or
re-seeded, since the 3.1 migration is not reversible in place. Worth writing down even for a
lab: the previous revision said "not applicable", which is not the same as "cheap".

## Open Questions

- **Q1**: *(answered)* Upstream has `/users`, `/users/{id}` and `/users/{user_id}/{client_id}`
  (`state.rs:400-410`), guarded by `Scope::WriteUsers` — but **only under `internal-oauth`**
  (D7). With that feature built in, the seed script *can* create users via API instead of
  fixture SQL. That is now a choice to make deliberately, not a constraint.
- **Q2**: *(answered — D5)* The simulator is not redesigned. The device set is unchanged.
- **Q3**: Do we seed any subscriptions? Deferred with the MQTT non-goal — but note the VEN scope
  set in D3 no longer includes `write_subscriptions_ven`. Add it back only when a subscription
  actually exists to write.
- **Q4**: Which `reportIntervals` value does each of our report descriptors mean
  (`INTERVALS | SUB_INTERVALS | OPEN_INTERVALS`)? 3.1 makes this a required field; under
  `wire-contracts` we must declare it rather than accept the `INTERVALS` default by omission.
