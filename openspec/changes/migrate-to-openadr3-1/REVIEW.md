# Review of `migrate-to-openadr3-1` against the real 3.1 code

Reviewed 2026-09-18 against `upstream/main` of `OpenLEADR/openleadr-rs`
(`295a298`, 2026-09-14) and the lab working tree at `main` (`20d9cb64`).

The change was written when 3.1 lived on a side branch at `v0.2.0-rc1`. That is no longer
the world it describes. This review lists what is **wrong** (B-n, factually false today),
what is **missing** (G-n, real work the change does not mention), and what is
**stale** (S-n, references to things that no longer exist).

---

## The premise has changed

`Upgrade to OpenADR 3.1 (#313)` merged to `upstream/main` on **2026-03-13**. 3.1 is not a
branch — it is upstream mainline, and has been for six months. Upstream has since shipped
subscriptions, resource groups, MQTT/WebSocket notifiers, and a bl/ven scope split on top
of it.

Our submodule sits at `9588232` (2026-08-13), forked from `823a475` (2026-02-20) — three
weeks *before* 3.1 landed. Divergence is **179 upstream commits ahead, 37 ours behind**.

This reframes the whole change: it is not "adopt a branch", it is **rebase our fork onto a
mainline that moved six months and one major version**.

---

## B — Bugs (statements that are false against the current code)

### B-1 The scope names in D2 and task 3.1 are wrong, and one is actively dangerous

D2 gives the BFF `{read_all, write_vens, write_programs, write_events, write_users}`.
The real enum (`openleadr-vtn/src/jwt.rs:123`) splits business from VEN:

```
WriteReportsBl        WriteReportsVen        (alias: "write_reports")
WriteSubscriptionsBl  WriteSubscriptionsVen  (alias: "write_subscriptions")
WriteVensBl           WriteVensVen           (alias: "write_vens")
```

`"write_vens"` is an **alias for `WriteVensVen`** — the VEN-side scope. `api/ven.rs:79`
branches on it:

- `WriteVensBl` → takes `clientID` from the request body (what the BFF needs)
- `WriteVensVen` → **overwrites `clientID` with the caller's own token `sub`** (`api/ven.rs:93`)

A BFF configured exactly as D2 specifies would create every VEN object with
`clientID = "bl-client"`. The unique index `ven_client_id_unique` then rejects VEN #2, and
the ones that do get created are addressable only as the BFF. The BFF needs
**`write_vens_bl`**, spelled out — never the alias.

### B-2 Our Dockerfile patch would silently strip `/auth/token` on 3.1

`Scope::WriteUsers` is `#[cfg(feature = "internal-oauth")]`, and `state.rs:396` gates
`POST /auth/token` and the entire `/users` tree behind the same feature. On upstream/main
that feature is **not in `default`**:

```
default = ["postgres", "compression-*", "experimental-websockets"]   # upstream 3.1
default = ["postgres", "live-db-test", "internal-oauth"]             # our 3.0 fork
```

Upstream compensates in its own Dockerfile, which passes the flag explicitly:

```dockerfile
cargo build --release --bin openleadr-vtn --features internal-oauth   # upstream/main
```

**Our fork replaced that Dockerfile** with the cargo-chef/BuildKit version (P-3 below), which
builds with a bare `cargo build --release --bin openleadr-vtn` — correct today only because
our fork also carries `internal-oauth` in `default`. Rebase onto 3.1 and that prop is gone:
our Dockerfile wins, the feature is off, and the VTN comes up **with no token endpoint at
all**. The BFF, all 20 VENs, the seed script and every BDD auth step authenticate through it.

So this is not an upstream oversight — it is a collision between two of our own patches, which
is exactly the kind of thing a rebase hides. Task 2.5/2.6 ("validate `POST /auth/token`
returns 200") would be the first casualty, with no diagnosis in the plan.

Fix: carry `--features internal-oauth` into the rebased Dockerfile, and pin it with a smoke
check before anything else runs.

### B-3 Open question Q1 is already answered, and the answer is conditional

Q1 asks whether the 3.1 branch has a user management API. It does —
`/users`, `/users/{id}`, `/users/{user_id}/{client_id}` (`state.rs:400-410`), all guarded by
`Scope::WriteUsers` — **but only under `internal-oauth`** (see B-2). So the seed script *can*
create users via API rather than fixture SQL, provided we build the feature in. Tasks 2.1/2.2
assume fixture SQL; that is a choice now, not a constraint, and it should be made
deliberately rather than by default.

### B-4 D5 contradicts the code it proposes to delete

D5: "delete the existing `VEN/src/simulator/` and `VEN/src/reactor/` directories and rewrite
from scratch … the existing physics models were correct but tightly coupled to 3.0 event
structures."

`VEN/src/reactor/` does not exist. `VEN/src/simulator/` is the physics core behind the MILP
planner and the asset-competence model — it is not coupled to OpenADR event structures at
all; `VEN/src/controller/openadr_interface.rs` is the only thing that touches event payloads.
D5 proposes deleting the one part of the VEN that 3.1 does not affect, and it is the single
largest work item in the plan (tasks 5.1–5.9, plus 9.4).

D5 also says "remove `POST /sim/override`". That endpoint is used by the VEN UI and by the
BDD suites. Removing it violates `no-half-built-features` and would break scenarios unrelated
to the migration.

**D5 should be deleted from this change outright**, not rescoped.

### B-5 The report body inversion is understated

Task 4.4 says "remove `programId`, set `eventID` from triggering event". The actual 3.1
shape is stricter than that reads:

| | 3.0 (`ReportContent`) | 3.1 (`ReportRequest`) |
|---|---|---|
| `programID` | required | **gone** |
| `eventID` | required | required |
| `clientID` | — | on `Report`, VTN-provisioned from token |
| `venID` | column | **gone** |

Our `OadrReportBody` (`VEN/src/controller/vtn_port.rs:119`) has this exactly inverted:
`programID` mandatory, `eventID` an `Option`. Today the VEN can and does emit reports with
no `eventID`. Under 3.1 **every report must name an event** — there is no program-level
reporting. That is a behavioural constraint on the fleet-monitor design (which is why the
standing `fleet-monitoring` program needs a standing *event*), and it deserves to be stated
as a constraint rather than buried in a field-rename task.

### B-6 D4's "empty targets means open to all" is asserted, not verified

The filtering SQL lives in `data_source/postgres/{program,event}.rs` and the API branches on
`ReadAll` vs `ReadTargets` (`api/program.rs:34`). The `read_all` holder bypasses target
filtering entirely — so the BFF sees everything regardless. The claim as written is
plausible but this change never cites the query it depends on, which is exactly the inventory
`one-concept-one-function` requires of a design. It needs verifying against the actual
`retrieve_all` before any privacy scenario is written against it.

---

## G — Gaps (work the change does not account for)

### G-1 Fork patch audit — what we carry, and what happens to each

Full diff of the fork against the merge base (`823a475..9588232`) is **14 files, +576/-163**,
and it contains four functional patches plus their sqlx cache and fixtures. The change
mentions none of them except a one-line claim that #372/#374 become obsolete. Each is audited
against `upstream/main` below.

**P-1 — GB-04 active-event filter — SAFETY + EFFICIENCY — absent upstream — RE-PORT**

Not one flag; a three-layer patch:

- `migrations/20260813000000_event_ends_at.sql` — `event.ends_at timestamptz` + index, with a
  backfill for the common case
- `openleadr_wire::event::EventContent::ends_at()` — the single authority for "when does this
  event stop being active", with **6 unit tests** covering event-level duration, open-ended,
  no-timing, missing per-interval timing, any-open-ended-interval, and latest-interval-end
- `data_source/postgres/event.rs` — filtering moved into SQL (`$10`), `ends_at` kept in sync on
  insert and update, plus 3 sqlx integration tests

The efficiency half: the VTN no longer fetches every event row to filter in Rust. The safety
half is the one that matters more — the patch's own migration comment records it:

> `?active=` + pagination [was] silently wrong: LIMIT/OFFSET picked a page *before* the active
> filter was applied, so a page could come back short or with rows from the wrong page.

Upstream/main still has no `active` param (`api/event.rs` `QueryParams` has only
`programID`, `targets`, `skip`, `limit`) and still orders + paginates in SQL with no active
predicate. **Losing this in the rebase silently reintroduces a pagination correctness bug**,
and nothing in the current plan would catch it.

Re-porting is *not* mechanical: 3.1 makes `intervals` optional and adds a top-level `duration`
(G-5). `ends_at()` must grow that case — which is precisely why it should stay one function
(D9).

**P-2 — report cascade-delete (`89fdb90`) — SAFETY — absent upstream — RE-PORT**

`migrations/20260322000000_report_event_cascade_delete.sql`. `report.event_id` had no
`ON DELETE CASCADE`, so `DELETE /events/{id}` returned a 409 FK violation once any VEN had
reported against that event. Still absent on `upstream/main`, and 3.1 makes it *more*
pressing: `eventID` becomes the report's only object link (B-5), so every report is now
FK-bound to an event.

**P-3 — `vtn.Dockerfile` cargo-chef + BuildKit cache mounts — EFFICIENCY — RE-APPLY WITH CARE**

Four-stage build (planner → cook → builder → runtime) with two-layer caching. This is the
patch that turns the ~25 min rebuild into an incremental one, and R1/R2 of the design lean on
it. It also fixes upstream's broken runtime `COPY` path.

Three collisions to resolve when re-applying, none of them mentioned in the change:

| | ours (`9588232`) | upstream/main |
|---|---|---|
| base image | `rust:1.92-alpine` | `rust:1.94-alpine` |
| openssl | static (`openssl-libs-static`) | dynamic (`cmake g++ make openssl3-dev`, `RUSTFLAGS=-Ctarget-feature=-crt-static`) |
| features | *(none — relies on our `default`)* | `--features internal-oauth` |

The features row is B-2: take upstream's flag, our caching structure.

**P-4 — VEN_NAME target reconstruction + stripping (#372, #374) — OBSOLETE — DROP**

Confirmed superseded, with a spec citation in upstream's own code. 3.1 implements target
hiding natively in `retrieve_all_with_client_id` (`data_source/postgres/event.rs`):

> Target hiding: For program and event objects, a VTN will only include requested targets in a
> response. This prevents VENs from learning targets that have not been explicitly assigned to
> them by BL. — spec v3.1.1, Definition.md

It filters on `e.targets && $3 OR array_length(e.targets,1) IS NULL` and then redacts with
`e.targets = intersection(&e.targets, &ven_targets)`. That is our `strip_ven_name_targets`
behaviour, done natively and by the spec. Drop ours — but port the *tests*: the five privacy
cases in our diff (`ven_in_targets_sees_event_stripped`, `ven_not_in_targets_gets_not_found`,
`ven_list_filters_and_strips`, `business_sees_full_targets`,
`ven_sees_event_with_null_targets`) are the assertions that prove the property still holds
after the migration. Dropping an implementation is fine; dropping its safety tests is not.

As a bonus, this same query confirms D4's unverified claim (B-6): empty targets **do** mean
visible to all.

### G-2 The change is sized for 3 VENs. The fleet is 20.

Tasks 2.1, 2.2, 6.1, 6.2, 7.2–7.9 and D3's provisioning sequence all name `ven-1..ven-3`.
`scripts/seed_vtn.py` provisions **20** (ven-1..3 on Node1, ven-4..20 on Node2), and
`VEN/scale_out/node2/docker-compose.yml` alone carries 18 `VEN_NAME` references. Every
fixture, credential, self-registration and validation step multiplies by ~7, across two
docker hosts the change never mentions. Node2 does not appear anywhere in the plan.

### G-3 D3's `client_id` naming is a needless 20-VEN rename

D3 and tasks 7.2–7.4 use client ids `ven-1-client`, `ven-2-client`… The seed script already
uses `client_id: "ven-1"` for all 20. 3.1 requires a clientID; it does not require renaming
one. Keeping `ven-N` makes the migration a schema change instead of a schema change *plus* a
fleet-wide identifier churn that touches fixtures, profiles, compose files, BDD steps and
docs. Drop the `-client` suffix from the design.

### G-4 Six months of upstream features are invisible to this change

Landed on `upstream/main` after 3.1, all absent from the proposal:

- **Subscriptions** (`20260218122421`, `20260506070840`) — `/subscriptions` CRUD with
  `objectOperations`, and notifier transports at `/notifiers/ws`, `/notifiers/mqtt`,
  `/notifiers/push-mqtt` (`state.rs:390-395`).
- **Resource groups** (`20260408082754`, plus a recursive-family view) — a whole targeting
  concept the lab does not model.
- **bl/ven report scope split** (`20260824131606`) — post-dates the 3.1 migration file, so the
  `scope` enum the change quotes from `20260213100612_openadr_3.1.sql` is already outdated.
- `experimental-websockets` is **on by default**, with the upstream comment
  *"object privacy is not yet implemented"* — i.e. a default-enabled transport that bypasses
  the privacy filtering this lab cares about. Needs an explicit decision to disable.

The Non-Goals say "MQTT pub/sub support (polling remains; MQTT deferred to a future change)".
That was true of a branch at `v0.2.0-rc1`. On today's mainline **the VTN speaks MQTT natively**
(`paho-mqtt` is a hard dependency of `openleadr-vtn`, not optional). This directly bears on
the fleet-monitor phase-0 decision to run reports and a hand-rolled
`openadr-lab/fleet/<ven>/…` side channel in parallel — a native subscription/notifier path
may replace part of it. That decision should be revisited after the migration, not locked in
before it.

### G-5 Event schema changes beyond targets are unlisted

The proposal lists breaking changes for programs, reports, VENs and targets but **not events**,
yet `EventRequest` changed materially:

- `EventContent` → `EventRequest` (same rename for `ProgramContent`/`ReportContent` → `…Request`)
- **new** `duration: Option<Duration>` (also a new `event.duration` DB column)
- `intervals: Vec<EventInterval>` → **`Option<Vec<EventInterval>>`**
- `targets: Option<TargetMap>` → `Vec<Target>` (non-optional, defaults empty)

The `intervals` optionality matters most: a 3.1 event may legitimately arrive with a
`duration` and **no intervals**. Every interval parser in the VEN assumes intervals exist.
That is precisely the GB-48 shape ("when does interval *i* run", decided in seven places) and
needs one consolidated answer, not seven adapted ones.

`EventType`, `ReportType`, `ReadingType` and `Unit` are **unchanged** in 3.1 — `Unit` still
has `KWH` and `KW` and still has no watt. The GB-50 contract design survives 3.1 intact.

### G-6 `ReportDescriptor` gained a required field

3.1 adds `report_intervals: ReportIntervals` (`Intervals | SubIntervals | OpenIntervals`,
default `Intervals`) and `targets` becomes `Option<Vec<Target>>`. Under `wire-contracts` this
is not optional for us: a report descriptor we emit must declare which of the three it means.
No task covers it.

### G-7 `programType` removal invalidates the GB-50 profile identifier

3.1 drops `programType` and adds `attributes: Option<Vec<ValuesMap>>`. The wire-contracts work
(GB-50) had been sketched around a program-level identifier for the lab profile; that field is
gone. `attributes` is the replacement vehicle, and the Program Attribute Enumerations table in
our 3.1 markdown copy is effectively empty — so the profile pointer becomes a namespaced
private attribute. Task 8.2 says "add `attributes` field" to a UI form with no statement of
what goes in it. This is the one place where the migration and GB-50 must be designed together.

### G-8 No rollback, no verification gate, no test-first

"Rollback: not applicable" is wrong — the fallback is a submodule pointer revert plus a DB
restore, and it should be written down. More importantly the plan has **73 tasks, 0 done,
across 11 capability specs**, with validation steps interleaved as manual curl checks and no
mention of `run_all_tests.sh`, the node1/node2 locks, or a green-before-merge gate. Under
`test-first` the BDD updates (task 10) cannot come last — the scenarios that define the new
auth and enrollment behaviour are what tell us the migration worked.

---

## S — Stale references

| Where | Says | Reality |
|---|---|---|
| tasks 1.1–1.3, D6, proposal | switch to branch `openadr3_1` | 3.1 is `upstream/main`; the branch is a 2026-03-13 dead end |
| proposal, design Context | `v0.2.0-rc1` | upstream is past `v0.2.6`; a `v2.0.0-alpha.1` tag exists off-mainline |
| task 4.1, Impact | `VEN/src/models.rs` | does not exist — DTOs are `VEN/src/controller/vtn_port.rs` |
| task 4.4, Impact | `VEN/src/reporter.rs` | does not exist — `VEN/src/controller/reporter.rs` |
| D5, task 5.1, Impact | `VEN/src/reactor/` | does not exist |
| tasks 2.1–2.3 | `VTN/fixtures/*.sql` | no `VTN/fixtures/` directory |
| tasks 11.1, 11.2 | `docs/project_journal.md`, `docs/KEY_LEARNINGS.md` | moved to `docs/history/` and `docs/reference/` |
| task 11.3 | "update memory file MEMORY.md" | not a repo artifact; workflow rule 3 wants this waved into `docs/` instead |
| D3 | VEN provisioning is a "4-step 3.0 sequence" in a script | now `tests/provision_ven.py`, and credential-ordering is load-bearing (GB-49) |

One trap worth naming: **`VEN_NAME` is also an environment variable** in
`VEN/docker-compose.yml`, `VEN/scale_out/node2/docker-compose.yml`, `VEN/src/config.rs`,
`weather.rs` and `measurement.rs`. Of 115 `VEN_NAME` occurrences across 35 lab files, a large
share are the env var and must **not** be touched by the target-type migration. A blanket
search-and-replace would break VEN startup on both hosts.

---

## What I'd do with this change

1. **Rewrite the premise** — this is a fork rebase onto `upstream/main`, not a branch switch.
   D6 and tasks 1.1–1.4 need replacing wholesale.
2. **Delete D5 and tasks 5.1–5.9** (and 9.4's override removal). The simulator is out of scope.
   That removes the largest and least justified block in the plan.
3. **Add a phase 0**: build-feature audit (B-2), and re-port the `active` filter and
   report-cascade patches (G-1) with their regression tests, *before* any lab-side change.
4. **Correct the scope table** (B-1) to the bl/ven split, with `write_vens_bl` spelled out and
   the alias trap noted.
5. **Rescale to 20 VENs across Node1 and Node2** (G-2) and drop the `-client` rename (G-3).
6. **Add an event-schema section** (G-5, G-6) with `duration`/optional-`intervals` as a single
   consolidated parser decision.
7. **Fold GB-50 into the program-schema capability** (G-7) — `attributes` is where the lab
   profile pointer must live, and that design cannot be deferred past this migration.
8. **Re-open the MQTT non-goal** (G-4) after the rebase, before committing fleet-monitor
   phase 0 to a hand-rolled side channel.

Items 1–4 are prerequisites: without B-2 the stack does not boot, and without G-1 the
migration silently regresses GB-04.
