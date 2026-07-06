# Phase 2 — Fleet Enablement

> **Goal:** go from 3 hand-seeded VENs to `./fleet.sh up N` with a stable VTN under
> N-agent load.
> **Items:** BL-03 (backoff), UC:pagination, UC:RFC7807 + BL-25 (error variants),
> UC:§6.7 (self-registration), GB-06, GB-07 (fleet generator), GB-09 (poll intervals).
> **Prerequisites:** Phase 0 WP0.2 (uniform naming/UUIDs) merged.
> **Exit demonstration:** `./fleet.sh up 10` on Pi4 → 10 healthy VENs polling,
> planning and reporting; `./fleet.sh down` tears them off cleanly; VTN CPU and the
> recorder's `ven_snapshots` confirm stability over ≥ 1 h.
> **Total effort:** ~2–3 weeks.

## WP2.1 — BL-03: Exponential backoff + jitter (M)

1. Extract a `Backoff` helper into `VEN/src/tasks/shared.rs` (exists already — keep
   the file ≤ 200 lines; new `tasks/backoff.rs` if it won't fit):
   on success → base interval (30 s); on failure → double up to 900 s cap; ±10 %
   jitter from a seedable RNG (determinism rule: injectable clock *and* injectable
   RNG seed so tests are exact).
2. Unit tests first: `test_backoff_doubles_on_consecutive_failures`,
   `test_backoff_resets_on_success`, `test_backoff_caps_at_max`,
   `test_backoff_jitter_within_10_percent`.
3. Apply to the three poll loops (`tasks/poll_programs.rs`, `poll_events.rs`,
   `poll_reports.rs`). Integration check (resilience suite): stop VTN container,
   observe VEN log intervals grow; restart VTN, observe immediate 30 s reset —
   add this as a scenario to the existing resilience suite rather than a manual step.

## WP2.2 — Pagination in `vtn.rs` (S–M)

1. Generic helper: repeat GET with `skip`/`limit` (configurable page size, default
   e.g. 50) until a short page returns; accumulate.
2. Apply to all collection GETs (programs, events, reports). Reuse the pattern proven
   in the Phase-1 recorder (WP1.7).
3. Adapter-contract tests with the existing fake-VTN test double: 120 events across
   3 pages → all 120 returned; empty collection → one request, empty vec.
4. Guard: log a warning if a single poll exceeds e.g. 20 pages (runaway protection).

## WP2.3 — RFC 7807 problem parsing + BL-25 error variants (M)

1. `ProblemDetails { type, title, status, detail, instance }` struct; on non-2xx,
   attempt problem-JSON parse, fall back to raw body. Structured log line either way.
2. Wire the two reserved `DomainError` variants at their real boundaries (BL-25):
   - `VtnUnreachable`: connect/timeout-class failures in `vtn.rs`, distinct from
     HTTP-level errors — the backoff helper (WP2.1) and fleet debugging both key off it.
   - `PlanInfeasible`: returned when `SolverPort::solve` reports infeasibility,
     surfaced through the planning route instead of a generic 500.
   (`ProfileInvalid` stays reserved — hot-reload doesn't exist; note kept in BL-25.)
3. Tests: force each condition (fake VTN timeout; mock `SolverPort` returning
   infeasible — mock exists in `services/test_support/mock_solver_port.rs`), assert
   the typed variant reaches the caller.

## WP2.4 — VEN/resource self-registration (M–L)

Removes per-VEN manual seeding — the key unlock for arbitrary fleet sizes.

1. Startup sequence (flag `registration.self_register: true` in profile; default true
   for fleet instances, false keeps today's behaviour for the seeded trio):
   `GET /vens?venName={name}` → if absent `POST /vens`; then reconcile resources:
   `POST /resources` per local asset not present on the VTN.
2. Requires `write_vens` scope on the VEN credential — check what the openleadr-rs
   fork's auth setup grants; if VEN credentials are read-only today, the seed script
   must mint fleet credentials with `write_vens` (VTN-side change, seed-only, no fork
   code change expected).
3. Idempotency is the invariant to test: second startup registers nothing, changes
   nothing (adapter-contract test against fake VTN; BDD scenario against real stack:
   `ven registers itself on first boot`).
4. Failure mode: registration failure → retry with WP2.1 backoff; VEN keeps operating
   read-only meanwhile (don't block the control loop on registration).

## WP2.5 — Fleet generator + GB-09 + GB-06 (M–L)

1. `fleet.sh` (repo root, next to `run_all_tests.sh`) with subcommands:
   - `up N` — render N profiles from a template, generate
     `docker-compose.fleet.yml` override, `docker compose up -d`, health-check loop.
   - `down` — stop + remove fleet services (never touching non-project containers —
     hard rule) ; `--purge` also removes `/data` volumes.
   - `status` — per-VEN health from `/system` route + recorder `ven_snapshots`.
2. Profile templating: `VEN/profiles/fleet_template.yaml` + a generator (suggest
   Python, `scripts/gen_fleet_profiles.py`, since the E2E tooling is already Python):
   UUID per instance (GB-03 scheme), venName `fleet-ven-{n:03}`, randomized-but-seeded
   asset mix (battery size, EV presence, base load scale) so the fleet is diverse and
   reproducible (`--seed`).
3. GB-09: `poll.interval_s` (and report interval) become profile keys; the generator
   staggers poll *offsets* across instances so N VENs don't align their polls — add a
   startup delay jitter derived from the instance index.
4. GB-06: `scripts/db_reset.sh` — drop + re-seed VTN Postgres (openleadr-rs schema and
   `lab_recorder` schema), idempotent, invoked by `fleet.sh up --fresh`.
5. Resource budget check on Pi4: measure per-VEN container RAM/CPU with 10 instances;
   if the Pi4 saturates, document the max fleet size and add a `--host` option later
   (out of scope here).
6. Verification = the exit demonstration; also run the full E2E suite once with the
   fleet up to prove the original 3-VEN scenarios still pass alongside it.

## Order & risks

```
WP2.1 → WP2.2 → WP2.3   (client robustness, sequential — same file vtn.rs)
WP2.4                    (after WP2.3 — uses typed errors + backoff)
WP2.5                    (last — needs WP2.4 for seedless bring-up)
```

Risks: (a) `write_vens` scope may need fork-side auth config — spike early in WP2.4;
(b) Pi4 capacity for N containers unknown — measure at N=5 before promising N=10;
(c) poll stampede on VTN restart — the jittered offsets in WP2.5 plus backoff jitter
in WP2.1 are the mitigation; verify in the resilience suite with the fleet up.

Bookkeeping: mark BL-03, BL-25 (2 of 3 variants), GB-06, GB-07, GB-09 resolved;
cert-backlog rows §11 (pagination, problem parsing) and §8 (self-registration) move
to Partial/Full; journal + `/wiki-sync` ([[reliability-and-config]], new fleet page).
