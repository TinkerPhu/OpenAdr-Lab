# Fleet Run Journal

This document is the dedicated historical record for the multi-VEN fleet
experiment work: every live scenario run against the persistent VEN fleet
(`ven-1..3` on Node1, `ven-4..13` on Node2), the tooling built to drive and
analyze those runs (`experiments/run_experiment.py`, `experiments/kpi.py`,
the `experiments/scenarios/*.yaml` catalog), and every behavior bug found
and fixed as a direct result of running them.

Its purpose is **base truth and proof of how the VEN fleet reacts to VTN
commands** — price signals, capacity limits, alerts, dispatch setpoints,
budget-constrained user requests — **in connection with each VEN's own
realistic internal optimization** (its MILP planner, asset mix, persona,
comfort settings). Where `docs/history/project_journal.md` and the rest of
this repository's documentation describe only current state and future
plans, this file is an intentional exception: it preserves the
chronological narrative of what was run, what the fleet actually did, what
looked wrong, and how each real bug was root-caused and fixed — exactly
because that evidentiary trail is the point, not incidental to it. Same
exemption as `docs/history/**` generally (see
`docs/reference/DOCUMENTATION_STYLE.md`).

Entries below were originally written into `docs/history/project_journal.md`
and moved here verbatim (2026-08-19) once fleet-run testing had accumulated
enough of its own history to warrant a dedicated record; nothing was
rewritten in the move beyond this introduction.

---

## SG-1/Phase-4-exit persona re-run of S-1..S-6 (2026-08-10)

Immediate follow-on to the run above: same S-1..S-6 matrix, this time with a 3-VEN
eco/comfort/commuter persona fleet (`fleet.sh up 3 --personas`) layered on top of the base
`ven-1..3`, to check whether persona diversity (WP4.5) produces measurably different fleet
response — the Phase 4 exit criterion. Took three attempts; both real bugs found along the
way are now fixed in the codebase, not worked around.

**Attempt 1** (22:54Z) — two independent bugs, both silent (no crash, just wrong data):
1. `run_experiment.py --personas`' `setup_persona_sessions()` called `POST /ev-session` to
   give each fleet VEN its persona's EV request. That route was retired down to `GET`-only
   when BL-41 replaced the per-device Device-Sessions API with the unified `POST
   /user-requests` — a stale caller nobody updated when the migration happened. Every
   persona session creation failed with 405, silently producing zero persona
   differentiation.
2. `fleet.sh up` lets Docker Compose auto-create the bind-mounted `VEN/data/fleet-ven-*`
   directories, which come out `root:root` — but the `ven-app` container runs as uid 2000
   (matching `ven-1..3`'s directories, which get set up differently). Every persist write
   failed with `Permission denied`; `history.sqlite` was never created for any fleet VEN.

Killed mid-S3 (`kill -INT` did not reliably interrupt the Python process on this box, both
here and later — had to `SIGKILL` and manually delete the dangling VTN program +
event via the API each time cleanup was needed).

**Fixes**: (1) `chown -R 2000:2000` the three `fleet-ven-*` data directories. (2) Committed
+ pushed + pulled a fix to `experiments/run_experiment.py` (commit `dc81d20`) retargeting
`setup_persona_sessions()` at `POST /user-requests` (`asset_id: "ev"`, `deadlines: [...]`
instead of the old `departure_time` field) with `DELETE /user-requests/:id` for teardown.
Verified both fixes independently via direct `curl` against a live fleet VEN before
resuming.

**Attempt 2** (00:03Z) — `fleet.sh up` is idempotent and found the attempt-1 containers
still running, so it left them alone rather than recreating them. That mattered because
`main.rs` only calls `SqliteHistoryStore::open()` once at process boot; those containers had
already tried and permanently failed at their original 22:58:54Z boot (directories were
still root-owned then) and never retry. The chown fix only helps a *fresh* boot. S-1
completed with correct persona sessions but **still** zero history data. Discovered
mid-S2 and `docker restart`ed the three containers to force a fresh boot with the
already-correct permissions — but active user-requests aren't part of the persisted
`state.json`/`sim_state.json`, so the restart silently wiped S-2's in-flight persona
session too (confirmed: `GET /user-requests` returned `[]` right after). Killed the run,
cleaned up the dangling S-2 VTN program.

**Learning for next time**: a `docker restart` of a warm container is not a safe way to
"pick up a directory permission fix" if that container holds any request/session state that
isn't in its persisted snapshot — always prefer a full stop+recreate (or just don't touch
already-broken containers; kill the driver and relaunch fresh so `fleet.sh up` builds and
boots them correctly from scratch).

**Attempt 3** (00:40:19Z) — fully fresh containers, both fixes in place before any scenario
ran. Confirmed clean at every step: S-1 had no "no history store" warning, `history.sqlite`
plus its `-wal` sidecar grew steadily throughout (2.8 MB+ by S-4), and all six scenarios
completed with directory-capture, KPI extraction, and report rendering all working (the
run-dir capture bug from the earlier S-1..S-6 run had already been fixed in this driver
script and held up across all 6 runs here too). `fleet.sh down --purge`'s own data-file
`rm` failed with `Permission denied` (same uid-2000-vs-`pi`-user mismatch, this time on
*deletion* rather than creation) — containers were still removed cleanly, just the data
dirs needed a manual `sudo rm -rf` after. Not fixed in `fleet.sh` itself; noted here as a
minor follow-up (the `--purge` path assumes it can delete files it didn't chown).

**Findings from the report** (`experiments/results/s1-s6-persona-report.md`) — this is the
result that matters, real persona-driven behavioural diversity, exactly the Phase-4 exit
criterion:
- **eco** (`fleet-ven-001`, OPPORTUNISTIC/free-energy-only): near-zero import in every
  single scenario (0.003–0.15 kWh per 30-min window, cost ≈ €0) — textbook "only charge on
  surplus" behaviour.
- **comfort** (`fleet-ven-000`, ASAP/cost-blind): consistently the highest importer across
  all six scenarios (2.9–3.8 kWh, €0.18–0.28) — convenience wins over price exactly as
  designed, and barely responds to any control signal (S-2..S-6 costs stay close to the S-1
  baseline).
- **commuter** (`fleet-ven-002`, BY_DEADLINE + €2 budget ceiling): intermediate and the most
  *reactive* of the three — import swings from 0.007 kWh (S-3, capacity-limited) up to 2.09
  kWh (S-6, combined) depending on the scenario, showing the deadline pressure trading off
  against the budget cap as conditions change.

This is a clean confirmation of SG-1 (fleet diversity produces measurably different
responses) — the three personas are visibly distinguishable on every KPI in every scenario.

**Both open items from the first (non-persona) run reproduced independently here**, which
upgrades them from "maybe a fluke" to "confirmed real, worth fixing before the data path is
trusted further":
- `report_timeliness` is `null` again in all 6 scenarios' `kpis.json` (verified: each
  scenario's `recorder-reports_received.csv` has zero rows with `received_at` inside that
  scenario's actual wall-clock window, same as the first run).
- The S-5 dispatch divergence pattern recurs: `ven-3`/`fleet-ven-002` (its persona-fleet
  counterpart role) shows a markedly larger response than `ven-2` under the exact same
  `DISPATCH_SETPOINT` event in both independent runs (this run: `ven-3` 0.89 kWh / 6.6 kW
  peak vs. `ven-2` 0.24 kWh / 0.5 kW peak — same shape as the first run). Reproducing across
  two independent live runs rules out one-off noise; still not root-caused.

**Follow-up**: file both open items (`report_timeliness` always null; the `ven-3` S-5
dispatch divergence) as BACKLOG entries — they're now confirmed-reproducible, not
speculative. Also worth a small `fleet.sh down --purge` fix so its own cleanup doesn't need
a manual `sudo rm -rf` afterward.

## GB-18 root-caused and fixed: VTN recorder had been dead for 9 days (2026-08-10)

Follow-up investigation into `report_timeliness` always being `null` (both experiment runs
above). Root cause was not a clock/window-matching issue at all: the recorder background
task had been **completely dead since 2026-08-01T11:04:21Z**, silently, for 9 days.

**What happened**: `vtn-bff`'s `main.rs` connected to Postgres once at process startup
(`sqlx::PgPool::connect(&database_url).await?`) with no retry. On 2026-08-01 there was a
brief internal Docker DNS hiccup (`vtn`/DB hostnames briefly unresolvable, visible in the
BFF's own logs as a burst of "dns error: failed to lookup address information" on unrelated
VTN-API polls around 10:50–11:04Z) that happened to land on a `vtn-bff` container restart's
startup connection attempt. That one failed connect logged a single `ERROR` line
("recorder disabled: failed to connect to DATABASE_URL") and the recorder subsystem was
never started for that process's entire lifetime — while the rest of the BFF kept serving
API requests completely normally (`docker ps` showed `vtn-bff-1` healthy for all 9 days).
Confirmed via `SELECT max(received_at), count(*) FROM lab_recorder.reports_received`:
frozen at `2026-08-01 11:03:08`, 119,704 rows, unchanged until this fix.

**Fix** (`fix/vtn-recorder-reconnect`, commit `c4ca5b8`, fast-forward merged to `main`):
- `recorder.rs`: connect + `init_schema` now retry forever with exponential backoff
  (5s → 300s cap), fully inside the recorder's own spawned task — a DB/DNS problem can
  never block or fail BFF startup itself (previously `init_schema(&pool).await?` used `?`
  directly in `main()`, so even a schema-init failure — not just a connect failure — could
  have taken down the whole BFF, not just the recorder).
- Added `RecorderStatus` (connected, last poll/success time, consecutive failures, last
  error) tracked via `Arc<RwLock<_>>`, updated every connect attempt and every poll tick.
- Per `ui-transparency`: this state had **zero** visibility before this fix — nothing but a
  single log line, 9 days ago, that nobody was watching. Surfaced via `GET /api/health`'s
  new `recorder` block and a "Recorder: connected/disconnected (N failed attempts)" line on
  the VTN UI Dashboard's health card.
- Test-first: added pure unit tests for the backoff calculation and the three status-mutation
  helpers (`mark_connect_failure`/`mark_connect_success`/`mark_poll_tick`) before
  implementing them — confirmed red (compile failure, functions didn't exist), then green.
  Dashboard gained 3 new tests (connected / disconnected+failure-count / hidden-when-disabled)
  following the same red→green flow. Full `VTN/bff` (26/26) and `VTN/ui` (67/67) suites green,
  `cargo fmt`/`clippy -D warnings`/ESLint (0 errors) all clean, `cargo audit` shows only
  pre-existing unrelated findings (no new dependencies added).

**Mitigation + verification**: restarted `vtn-bff` immediately (before the code fix existed)
to get data flowing again right away — confirmed archiving resumed instantly. After
deploying the actual fix, redeployed `bff`+`ui` on Node1 (`docker compose up -d bff ui`
unexpectedly also recreated `vtn-db`/`vtn-vtn` — all 4 containers came back healthy within
~30s, recorder row count kept climbing through the recreate with no data loss, so treated as
a non-issue rather than chased further). Verified live: `GET /api/health` now returns a
populated `recorder` block (`connected: true`, fresh `lastPollAt`/`lastSuccessAt`), and the
deployed UI bundle contains the new "Recorder:" status text.

**Still open**: GB-18's *symptom* (report_timeliness null) is now explained and the root
cause fixed, but the 9-day historical gap in `lab_recorder.reports_received` is permanent —
any future analysis spanning 2026-08-01 through 2026-08-10 will have a reporting blind spot
for that window. Not backfillable (the source data was never captured). GB-18 can be closed
in BACKLOG.md; the underlying reliability gap (no retry, no health surface) is what's fixed
here, not just the immediate incident.

## Full 13-VEN fleet deploy + S-1..S-6 experiment run (2026-08-12)

**Trigger**: after the R-18 fix, the user asked to (1) bring Node2's VEN fleet up to
current `main` if it wasn't already, (2) extend the experiment tooling (`experiments/
run_experiment.py`, `kpi.py`) to cover the full 13-VEN fleet (Node1's `ven-1..3` +
Node2's `ven-4..13`) instead of just Node1's 3, (3) design what to record so a
scenario run's results can be judged expected-or-not, then (4) run S-1..S-6
sequentially against the full fleet, unattended (~8h window). Work happened in a
worktree (`worktrees/fleet-13-ven-experiment`, branch `fleet/13-ven-experiment-run`)
rather than the main checkout — the main checkout had unrelated uncommitted WIP for
the `reactive-correction-notifications` openspec change sitting in it that needed to
stay untouched.

### Deploy (Phase A)

Local `main` was 2 commits ahead of `origin/main` (unpushed — included the R-18 fix)
— pushed first so both hosts could pull it. Node1 was 8 commits behind, Node2 was 11
— both fast-forwarded cleanly (`git pull --ff-only`, both hosts had clean working
trees). Rebuilt and redeployed under the standard lease locks: Node1's `VTN/` and
`VEN/` compose projects (ven-1..3 + VTN/BFF/UIs), Node2's `VEN/scale_out/node2/`
compose project (ven-4..13). `bash scripts/capture_ven1_logs.sh` run before the
Node1 `ven-1` rebuild per the standing rule. All 13 VENs came up healthy and
`/vtn/status`-connected on the first try — no repeat of the earlier 401-degraded
transient. Both hosts now on `738003104ecb8127aa15d7c39efd41a339399e32`.

### Tooling extension (Phase B)

`run_experiment.py`/`kpi.py` only ever addressed VENs on the host they ran on
(Node1's `ven-1..3`, reading `history.sqlite` off the local bind mount and
`docker exec`-ing `vtn-db-1` directly) — Node2's `ven-4..13` were invisible to the
scripts. New `experiments/fleet_map.json` gives each of the 13 VENs a host/port/
lan_ip/remote_data_root entry. `snapshot()` now takes an optional fleet map and
routes each VEN through either the existing local file copy or a new `scp`-based
remote pull; the recorder-CSV dump gained a `--pg-host` flag that wraps the
`docker exec ... psql` call in `ssh` when not run locally.

**Orchestration host**: the script's own docstring says it "runs ON the docker host
(Node1)," matching `fleet.sh`'s convention. That assumption broke here — Node1 has
no configured ssh trust to Node2 (`ssh Node1 "ssh Node2 ..."` failed to resolve the
alias, and retrying with agent-forwarding hit `Permission denied (publickey)`), and
setting up new cross-host ssh trust on shared production hosts wasn't something to
do unprompted mid-task. The workstation running this session already had verified
direct LAN HTTP and ssh/scp access to *both* Node1 and Node2, so the run was
orchestrated from there instead — `fleet_map.json` marks every VEN (including
Node1's own `ven-1..3`) as reached via its `Node1`/`Node2` ssh alias, and
`--pg-host Node1` routes the recorder dump the same way. This works but means the
"runs on Node1" docstring is now stale — filed as a backlog item (see below) rather
than fixed by force, since the right fix (restore Node1-to-Node2 ssh trust vs.
formalize off-host orchestration as the supported mode) is a judgment call for the
user.

**Diagnostics recording**: the point of this exercise beyond raw energy/cost KPIs
was to be able to tell *why* a scenario's result looked the way it did. Explored
what the VEN already exposes (`GET /plan`, `PlanReady` SSE) before adding anything —
`solve_status`, free-text `warnings` (severity-tagged), `cost_breakdown` (incl. a
`c_violations_eur` total), `objective_eur`/`friction_eur` are all already on the
persisted `Plan` and served by `GET /plan`; `solver_ms` exists only on the
`plan_ready` SSE event, not the persisted `Plan`. Per the user's explicit choice
(asked directly rather than assumed), this pass reuses what already exists via a
background poller rather than adding new VEN-side Rust instrumentation: a
60s-interval poll of every VEN's `GET /plan` for the run's duration, appended as
JSONL (`{ven}-plan-diagnostics.jsonl`), threaded so it runs alongside the existing
scenario action loop. `kpi.py` gained `plan_diagnostics_summary()` — solve_status
distribution, warning-severity counts, `c_violations_eur` mean/max per VEN, folded
into the existing per-VEN KPI block. `solver_ms` capture (would need a persistent
per-VEN SSE listener) and structured/typed violation fields were explicitly
deferred — backlogged below.

A 3-minute `smoke.yaml` dry run against all 13 VENs (multi-host snapshot + poller)
surfaced one real, unrelated bug before the real runs started: `kpi.py`'s recorder-
CSV reader hit Python's default 128 KiB `csv` field-size limit, because the
recorder dump is the *whole* `lab_recorder.reports_received` table (not filtered to
the run window) and this deployment's accumulated history contains a payload blob
past that limit. Fixed with `csv.field_size_limit(10 * 1024 * 1024)` (not
`sys.maxsize` — that overflows the C `long` `field_size_limit` uses internally on
Windows). The unfiltered-dump root cause itself is backlogged, not fixed here (out
of scope — a bigger change to the recorder query, not a one-line fix).

Committed as `803c8c0` on `fleet/13-ven-experiment-run`.

### The S-1 run got killed twice before it stuck

The harness's own background-task tracking for the local (non-ssh) `run_experiment.py`
process didn't survive across turns — both of the first two S-1 launches came back
`status: killed` at ~90s in, well before the 30-minute scenario window closed, with
no crash in the process's own output (empty log both times). Each left an orphaned
VTN program + event behind (the script's cleanup `finally` block never got the
chance to run) — cleaned up via direct `DELETE /events/{id}` and `DELETE /programs/
{id}` calls both times before retrying. Worked around the same way the project
already documents for remote long builds on separate git clones: launch fully
detached (`nohup ... > log 2>&1 < /dev/null & disown`), then poll the PID
(`Get-Process python`) and log file across turns instead of relying on the
harness's own background-task completion notification. This held for all 5
remaining scenario runs with zero further kills.

### Results — S-1..S-6 against the full 13-VEN fleet

Each run: 30 min real-time window, full fleet snapshot + `kpi.py` (`--baseline` =
S-1) run immediately after. All 6 runs: **13/13 VENs present in every `kpis.json`,
0 poll errors on the diagnostics poller, `solve_status: OPTIMAL` on every one of
390 polled samples across the whole run (13 VENs x 30 polls) in every scenario —
the solver never went infeasible or timed out anywhere in this run.**

| Scenario | Fleet import (kWh) | Fleet cost (EUR) | Peak (kW, any VEN) | Sum shift vs S-1 (kWh) | Warnings | Max violation cost (EUR) |
|---|---|---|---|---|---|---|
| S-1 flat (baseline) | 5.526 | 0.749 | 3.900 | -- | 0 | 0 |
| S-2 price_spike | 5.851 | 0.581 | 3.900 | -0.325 | 0 | 0 |
| S-3 capacity_limit (3 kW/10min) | 1.548 | 0.187 | 2.150 | +3.978 | 2 (ven-3) | 57.76 |
| S-4 alert (grid emergency) | 1.610 | 0.100 | 1.800 | +3.916 | 20 (ven-9/10/11/12) | 1000.00 |
| S-5 dispatch (SIMPLE + setpoint) | 1.955 | -0.013 | 3.900 | +3.571 | 0 | 0 |
| S-6 combined (spike+reservation+alert) | 2.812 | -0.254 | 4.257 | +2.713 | 29 (ven-2/3/9/10/11/12) | 1196.74 |

Capacity limits and the emergency alert clearly shift load and cut fleet cost — a
price signal alone (S-2) barely moved the fleet total, matching the earlier S-1..S-6
run's finding (see the 2026-08-09/10 entry) that capacity/alert control beats price
signals for peak shaving in this fleet.

**The S-3/S-4/S-6 warnings are the diagnostics tooling doing its job, not a bug.**
Every warning reads `"Grid capacity violation in N slot(s) - solver used slack"`,
and every VEN that raised one is asset-mix-inflexible for the constraint in play:
`ven-9` is `base_load`-only (no PV/battery/heater to shed at all — see `VEN/profiles/
ven-9.yaml`), and `ven-10/11/12` are similarly light. When a capacity limit or
emergency alert demands more shedding than a VEN's actual assets can deliver, the
MILP correctly falls back to a slack variable (`solve_status` stays `OPTIMAL`, not
`Infeasible`) and prices the shortfall into `c_violations_eur` rather than
producing an infeasible plan — the fleet's asset-mix diversity (5/13 VENs have PV,
3/13 have battery, per this file's Node2-growth entry) means "the fleet as a whole
met the constraint" (visible in the aggregate KPIs above) can still coexist with
"this specific inflexible VEN individually didn't." This is exactly the kind of
per-VEN signal the `GET /plan` poller was built to surface that pure grid-power
KPIs would have hidden.

**Methodology finding — the S-4 negative-shift confound**: `ven-1`'s
`energy_shifted_kwh` came back **-0.3615 kWh** in S-4 (imported *more* than its S-1
baseline despite the emergency alert). Investigated rather than dismissed: `ven-1`'s
`solve_status` was `OPTIMAL` for all 30 polls with zero warnings/violations in S-4,
so this isn't a shed failure. The scenarios run in real wall-clock time (the sim
clock has no injectable-time acceleration for this harness — see `run_experiment.py`'s
own docstring), and S-1 ran at 21:57 UTC on 2026-08-11 while S-4 ran at 06:21 UTC
the next day — a large natural time-of-day difference in PV/base-load that
`energy_shifted_kwh`'s same-VEN-different-run diff can't distinguish from an actual
event response. `ven-1` is one of the few VENs with PV (per its profile), which is
exactly the asset most sensitive to a 21:57-vs-06:21 UTC time-of-day difference —
consistent with this being the confound, not a regression. Backlogged: a same-
wall-clock paired-control or same-time-of-day-rerun design would remove this
confound for future runs.

**Bookkeeping**: new backlog items filed in `docs/BACKLOG.md` (deferred structured
plan-quality instrumentation, unfiltered recorder-CSV dump size, report_timeliness/
event_impact_kwh nulls because scenarios don't request BASELINE/USAGE reports, the
same-wall-clock-baseline idea, and the stale "runs on Node1" docstring). Tooling
changes + this entry committed and pushed on `fleet/13-ven-experiment-run` — **not**
merged to `main`; the branch is left for the user to review before merging. The
worktree is left in place (not removed) for the same reason.

## GB-28 fix: paired-baseline window for `energy_shifted_kwh` (2026-08-13)

**Trigger**: the user asked which of the newly-filed backlog items was most
pressing; answered GB-28 (renumbered from GB-27 — other agents had claimed
GB-21..24 on `main` in the meantime, resolved by rebasing this branch onto
current `main` and renumbering this session's own items to GB-25..29; see the
rebase note above). User then asked to implement it, planned first, reviewed
before merging.

**Fix**: `run_experiment.py` gained a `run_window()` helper (extracted from the
old single-window `main()` body, no behavior change to the window logic itself)
and a `--paired-baseline` flag (default on). When enabled, every scenario run
now spends one extra `duration_minutes` window with zero events posted,
immediately *before* the scenario's own window, snapshotted to a sibling
`{run_dir}-baseline/` directory. `kpi.py` auto-discovers that sibling directory
for `energy_shifted_kwh` when `--baseline` isn't given explicitly, falling back
to the old behavior (no baseline) if it's absent. This shrinks the baseline-to-
scenario gap from hours (the 8.5h S-1-vs-S-4 gap that surfaced the bug) to the
width of one window (~30 min for S-1..S-6) — a large reduction, not a full
elimination; the docstring/backlog explicitly say so, since a true fix needs
injectable sim time through the whole tick/poll path, out of scope here.

**Design choice — same-VEN adjacent baseline, not a cross-VEN control**:
confirmed via source read that the VTN does support per-VEN event targeting
(`targets: [{"type":"VEN_NAME","values":[...]}]`, enforced server-side in the
embedded `openleadr-rs` fork), so an in-run "control VEN" excluded from the
event was a real option. Rejected — the 13 VENs have deliberately
heterogeneous asset profiles (5/13 have PV, 3 of those have battery, `ven-9` is
base-load-only, no two share a profile), so "VEN A untouched" can't stand in
for "VEN B without the event." A same-VEN comparison is the only
methodologically sound one available without a bigger architectural change.

**Verification**: live smoke-test run (`smoke.yaml`, 3 min × 2 windows) against
the full 13-VEN fleet. `{run_dir}-baseline/` came back with 13/13 VEN
snapshots and its own plan-diagnostics files, exactly like a normal run.
`kpi.py --run {run_dir}` (no explicit `--baseline`) auto-picked up the sibling
baseline and populated `energy_shifted_kwh` for every VEN present.

**Found during verification, not part of this fix**: `kpi.py`'s output was
missing `ven-1` — 12/13 VENs, not 13. Investigated rather than assumed benign:
`ven-1`'s `grid_samples` table hadn't been written to in ~8.8h, and its
`/health` endpoint showed `storage: degraded`. Root cause: the running
`ven-ven-1-1` container (`StartedAt: 2026-08-12T20:51:18Z` — recreated by some
other session, not this one) has its `/data` bind mount pointing at
`/srv/docker/openadr_lab_main-deploy/VEN/data/ven-1`, a path that doesn't exist
anywhere on Node1's filesystem (confirmed via `stat`) — Docker silently mounted
it as an empty directory. Container logs confirm: `sim persist failed: No such
file or directory` / `persist write failed: No such file or directory`
repeating every ~15s since that timestamp, while VTN polling/events/reports
(which don't touch `/data`) kept working fine, masking the problem from a
casual `/health` glance. `ven-1`'s real data (through the last known-good
write) is still intact at the canonical `/srv/docker/openadr_lab/VEN/data/
ven-1/` — not touched, not remediated; flagged to the user rather than
restarted unilaterally, since it's live shared infrastructure another session
may still depend on. `kpi.py` handled the missing VEN gracefully (silent
exclusion, not a crash) — no code change needed for that part; it's exactly
the degrade-gracefully behavior you'd want.

**Bookkeeping**: GB-28 row removed from `docs/BACKLOG.md` (the in-scope,
actionable fix is done; the residual "needs injectable sim time" gap is
already out-of-scope-by-design, not a new open item). The `ven-1` data-mount
incident is not filed as a new backlog item here — left for the user to decide
how to remediate first, since acting on it (or even fully diagnosing it
further) wasn't this session's call to make unilaterally.

## GB-27 fix: reportDescriptors so scenario runs actually get reports (2026-08-13)

**Trigger**: second half of the same "which is most pressing" follow-up — after
GB-28, the user asked to implement GB-27 the same way (plan, implement, review,
merge).

**Root cause** (confirmed via source read, `VEN/src/controller/vtn_port.rs` +
`openadr_interface.rs`): a VEN only submits BASELINE/USAGE reports for an event
that carries a `reportDescriptors` array on the *event* body — program-level
reportDescriptors are spec-legal but not read by this VEN's model at all
(`OadrProgram` has no such field). `experiments/run_experiment.py`'s
`build_event()` never set this key, so no scenario run had ever produced a
reportable obligation; `report_lag_stats()`/`event_impact_kwh()` always
returned `None`/`null` — not because they're broken, but because nothing was
ever archived for them to read.

**Fix**: new `REPORT_DESCRIPTORS` constant (BASELINE `historical: false` +
USAGE, both `frequency: 300` seconds — confirmed via `extract_report_
obligations()` that frequency is seconds, not an ISO 8601 duration, a
documented past gotcha; also confirmed `due_at = now + frequency`, so the
*first* report only fires after a full frequency interval — 300s comfortably
inside every 30-min scenario window, but too long for the 3-min `smoke.yaml`,
which is why verification used a real scenario instead). `build_event()`
gained an optional `report_descriptors` param; `run_window()` attaches it to
the first event of `actions` when present (new `--request-reports` flag,
default on). For the GB-28 paired-baseline window (`actions=[]`, no events at
all otherwise), a synthetic **SIMPLE level=0** event spanning the whole window
carries the descriptors instead — confirmed via `docs/openadr_3_0_specs/
2_OpenADR 3.0 Definition v3.0.1.md` that level 0 means "normal operations" per
spec, and via `VEN/src/controller/milp_planner/inputs.rs`'s `simple_cap`
match arm that any level other than 1/2/3 falls through to the unrestricted
contractual cap — a genuine no-op for planning, unlike posting a real
price/capacity/alert event which would itself become a confound.

**Verification**: live 30-min `s1_flat` run (`--no-paired-baseline`, since
GB-28's own window-pairing mechanics were already verified separately) against
the full 13-VEN fleet. `kpi.py`'s output: `report_timeliness` populated with
130 samples (`median_s: -532.7`, `max_s: 72.4`, `min_s: -1145.5` — the
negative lags are expected, not a bug: `report_lag_s` is computed against
each interval's own timing, and a `historical: false` BASELINE report is a
*forecast* for a future interval, so it's legitimately received before that
interval's nominal time). `event_impact_kwh` populated (non-null) for all
12 VENs present. Investigated the raw recorder rows rather than trusting the
non-null check alone: a first ad-hoc query without time-window filtering
picked up unrelated historical report rows from days earlier (a reminder that
`report_lag_stats`/`_report_energy_kwh` deliberately filter by `received_at`
for exactly this reason) — redone properly windowed, confirmed 60 real
BASELINE and 60 real USAGE payload intervals for `ven-4` inside the actual run
window. Every VEN's `event_impact_kwh` came back exactly `0.0` — expected, not
a red flag: S-1 is the no-control-signal baseline scenario itself, so a
well-calibrated forecast should closely track actual usage when nothing
unusual is happening; this is the "no signal → no impact" sanity result, the
same caveat WP5.4's own proposal flagged going in ("simulated households may
be too regular, making the heuristic-baseline counterfactual look artificially
good"). A scenario with a real event (S-4, S-6) would be the next place to
look for a genuinely non-zero value, not attempted here (out of scope for this
fix's own verification).

**Bookkeeping**: GB-27 row removed from `docs/BACKLOG.md`.

## GB-26 + GB-29 fixes: windowed recorder dump, stale orchestration docstring (2026-08-13)

Two small follow-ups from the same fleet-experiment tooling session, both
trivial per the user's own assessment before implementing.

**GB-26**: `run_experiment.py`'s `snapshot()` dumped the entire
`lab_recorder.reports_received`/`events_published`/`ven_snapshots` tables via
unfiltered `COPY ... TO STDOUT`, so every run paid a dump cost that scaled
with total deployment history, not run length (hit ~122 MB for
`reports_received` alone in an earlier 3-min smoke test). Fixed by adding a
`_TABLE_TIME_COL` map (`reports_received.received_at`,
`events_published.seen_at`) and an optional `t_from`/`t_to` pair on
`snapshot()`; when both are given, the `COPY` query gains a
`WHERE <time_col> >= t_from AND <time_col> < t_to` clause. `ven_snapshots` is
exempt — it's a PK-per-VEN "latest state" table (one row per VEN, overwritten
in place), not an append log, so a time filter there would just risk
excluding a VEN's only row if its last write happened to fall outside the
window. `run_window()`'s call site now passes `t_from=t0` (the window's own
start) and `t_to=datetime.now(timezone.utc)` (the actual moment snapshotting
begins, not the nominal `end` — deliberately a little wider than the nominal
window so nothing arriving during the cleanup/event-deletion step gets
clipped).

**GB-29**: the module docstring claimed the script "Runs ON the docker host
(Node1), same convention as `fleet.sh`" — stale since the 2026-08-12 full
13-VEN run, which had to be orchestrated off-host (a workstation reaching
both Node1 and Node2 over LAN/ssh) because Node1 has no ssh trust to Node2.
Rewrote the docstring to describe both modes: on-host for a single-host run,
off-host via `--fleet-map`/`--pg-host` for a multi-host fleet — matching what
the script has actually done since GB-29 was filed. No behavior change,
docs-only.

**Verification**: both are syntax/doc-level changes with no new runtime
branch beyond the existing `--paired-baseline`/`--request-reports` machinery
already exercised in the GB-28/GB-27 verification runs above; confirmed via
`python3 -c "import ast; ast.parse(...)"` that the file still parses cleanly.
No live re-run performed — the `WHERE` clause only narrows what a pre-existing
non-empty query already returns, and the GB-27 verification run above already
exercised the exact `t_from`/`snapshot()` call path this reuses.

**Bookkeeping**: GB-26 and GB-29 rows removed from `docs/BACKLOG.md`.

## GB-25 fix: persisted plan-quality history (2026-08-13)

**Trigger**: implementing an approved, fully-specified plan for GB-25 — a
VEN's plan quality (solve time, warnings, MILP gap tolerance) was visible only
live: `solver_ms` existed solely on the transient `plan_ready` SSE event,
never on the persisted `Plan`/`GET /plan`; `PlanWarning` was free-text
(`message: String`) with no typed `kind`; no MILP optimality-gap field existed
anywhere; and the one table that would have stored plan history
(`plan_snapshots`) had been dropped as dead code (R-63) — its only writer was
never called from production, so it was always empty.

**Four confirmed design decisions** (fixed before implementation, not
re-litigated during it):

1. **Dedup switches to `kind`.** `services/notify.rs`'s `new_plan_warnings`
   previously deduped newly-surfaced warnings on `PlanWarning.message`; a
   warning's message text can carry per-cycle interpolated numbers (thresholds,
   window times), so two cycles' worth of "the same" warning looked new every
   time its numbers moved. Switched the dedup key to the new typed `kind`.
2. **`WarningKind` covers only what's actually raised today**: a 6-variant enum
   (`SolverInfeasible`, `StaleRateEstimate`, `BudgetShortfall`,
   `CapacityViolation`, `PeakPenaltyExceeded`, plus `Other` as an unused-today
   catch-all) mapped 1:1 onto the 5 real `PlanWarning{...}` construction sites
   in `controller::milp_planner::results` — no speculative variants for
   warnings the codebase doesn't actually raise.
3. **UI location: a new Diagnostics nav page** (`/plan-history`), not a tab
   inside the live Planner page — plan history is a distinct diagnostic
   surface from "what's the plan doing right now."
4. **MILP gap: proxy only, explicitly out of scope to do better.** Persist
   `mip_gap_target` (the solver's configured tolerance, `0.02`) + `solve_status`.
   The `0.02` literal, previously duplicated across `solver_phase1.rs`,
   `solver_phase2.rs`, and `solver_duals.rs`, was extracted to one shared
   `controller::milp_planner::types::MIP_GAP_TARGET` constant reused by all
   three `with_mip_gap` call sites. Querying a real *achieved* MILP gap from
   `good_lp`/`highs` is explicitly out of scope — persisting the configured
   target is only a proxy; filed as follow-up debt (R-65) and backlog item
   GB-31.

**Implementation**: `entities/plan.rs` gained `WarningKind`, `PlanWarning.kind`,
and `Plan.solver_ms`/`Plan.mip_gap_target` (both `#[serde(default)]` so old
persisted/serialized plans still deserialize). `entities/history.rs` gained
`PlanHistorySample` (mirroring the `ForecastAccuracySample` struct already in
that file, per the `forecast_accuracy_samples` pattern documented in
`docs/architecture/VEN_ARCHITECTURE.md` §4.9a — the design that pattern's own
doc comment says was chosen *over* reviving `plan_snapshots`).
`history_store/schema.rs` gained `SCHEMA_V10` (`plan_history` table,
`warning_kinds` stored as a comma-joined TEXT column rather than a join table
— the only consumer is a per-cycle UI summary, not per-warning queries) and a
new `history_store/plan_history.rs` module for its `append`/`query`, wired into
`HistoryPort` and `SqliteHistoryStore` the same way `forecast_accuracy.rs` is.
`services::planning::adopt_if_warranted` stamps `plan.solver_ms` before either
the `PlanReady` SSE emit or `state.set_active_plan`, so the live event, the
persisted `Plan`, and the plan-history row all agree; `mip_gap_target` is
stamped at `Plan` construction time in `results.rs` instead (it's known before
the solve, not after). `services::forecast::finish_plan_cycle` builds and
persists one `PlanHistorySample` per plan cycle (adopted or not — this is a
solve-quality trend, not a dispatch-history view) right alongside its existing
forecast-accuracy write, same best-effort/log-and-continue contract.
`GET /history/plans?from=&to=` mirrors the existing `/history/forecast-accuracy`
route's shape exactly. On the UI side: `PlanHistory.tsx` (solve-time trend
chart + per-cycle table with a warning-kind chip per warning) under a new
"Plan History" Diagnostics nav entry, and `PlanHeaderBar.tsx` (the live
Planner page) now renders the persisted `solver_ms`/`mip_gap_target` and a
kind chip per warning alongside the existing severity chip and message.

**Deviation from the plan's file-size estimate**: `results.rs` crossed the
500-production-line cap by 7 lines once the `kind` stamping and
`mip_gap_target` assembly landed. Fixed by moving the small, self-contained
`active_penalty_rules` helper (pure `PenaltyRuleParams` → `ActivePenaltyRule`
mapping, WP6.3/BL-09, unrelated to this change's own logic) out to
`controller::milp_planner::types`, which every `milp_planner` submodule
already pulls in via `use super::types::*` — no call-site changes needed
beyond the move itself.

**Verification**: `wsl cargo test -p ven-app` — 1003 + 1 passed, 0 failed
(under `wsl_lock.sh` discipline, `-j 2`; the lease expired mid-build once
during the UI-test detour and was re-acquired before continuing, per the
project's shared-WSL convention). `cargo fmt --check` and
`cargo clippy --all-targets --all-features -- -D warnings` both clean.
`scripts/audit_file_sizes.py` passed after the `results.rs` split above.
`cd VEN/ui && npm test` — 547/547 passed; `npm run lint` — 0 errors (the same
pre-existing `react-refresh/only-export-components` warning class every other
page-plus-helper-exports file in this codebase already carries, e.g.
`History.tsx`). A new BDD scenario
("Operator reviews historical plan quality after a plan cycle runs",
`tests/features/ven_history.feature`) plus two smaller `/history/plans`
route scenarios were added; E2E verification via Node1 (`run_all_tests.sh
--e2e`) was not attempted this session — left for the next Node1-lock
window, noted rather than skipped silently.

**Bookkeeping**: GB-25 row removed from `docs/BACKLOG.md`; new low-priority
row GB-31 added for the real-achieved-MILP-gap follow-up (decision 4 above).
`docs/reference/TECHNICAL_DEBTS.md` gained R-65, cross-referencing GB-31,
documenting the gap-proxy-only decision as tracked debt (mirroring how R-63
documented the `plan_snapshots` removal). `docs/architecture/VEN_ARCHITECTURE.md`
gained §4.9b describing the `plan_history` table, its route, and the UI
surfaces, following §4.9a's format.

## Fleet run 2 tooling (Part A) — 2026-08-14

Follow-up to the 2026-08-12 full 13-VEN fleet run: that run's plan-quality
stats were data-poor (solve_status OPTIMAL on all 390 samples, few
warnings), and GB-25 (`solver_ms`/`mip_gap_target`/typed `warning_kinds`,
`GET /history/plans`) has since landed on `main` but the live fleet
deployment still runs pre-GB-25 code. This piece of work (done in worktree
`worktrees/fleet-run-2-tooling`, branch `feat/fleet-run-2-tooling`) builds
the tooling/scenario/profile changes a fresh redeploy + 8-scenario run
(S-1..S-8) will use — no live deploy, no fleet run, done here.

**Profile**: `VEN/profiles/ven-9.yaml` (base-load-only, no controllable
asset) gained a `penalty_rules` block — 0.3 kW threshold, 30-min window,
1 EUR/kW — deliberately below its own 0.5 kW baseline so a PeakPenaltyExceeded
warning is guaranteed once redeployed. Exact shape matched against
`VEN/profiles/penalty_test.yaml`'s existing fixture.

**Scenarios**: `experiments/scenarios/s7_stress.yaml` (tight 1.5 kW capacity
limit overlapping a grid-emergency alert) added, matching `s3_capacity_limit`/
`s4_alert`'s existing field shapes exactly.

`experiments/scenarios/s8_budget.yaml` was added as its **own** scenario
rather than a 4th action folded into S-7 — keeps BudgetShortfall's own stats
separable per-scenario from CapacityViolation/PeakPenaltyExceeded's in the
`warning_kind_counts` KPI (kpi.py's `plan_history_summary`), since S-7's own
actions already produce those two kinds.

**New `budget_shortfall` action type** (`experiments/run_experiment.py`):
unlike every other action type, this bypasses the VTN — a direct
`POST /user-requests` on the target VEN's own HTTP API (no auth header
needed; confirmed by reading the route registration in
`VEN/src/routes/mod.rs` — `/user-requests` isn't behind bearer-token auth)
with a deliberately too-tight budget, to force a real `BudgetShortfall` plan
warning. New helpers `post_user_request`/`delete_user_request` mirror
`post_event`'s shape; `run_window()`'s action loop and `finally` cleanup got
a parallel `created_requests` list alongside `created_events`.

Key correctness finding from reading `VEN/src/controller/milp_planner/inputs.rs`'s
`budget_warning` and `VEN/src/assets/ev_milp.rs`: `budget_eur` only reaches
the MILP budget constraint (and therefore the warning) when the EV session's
`mode` is `MAX_COST` — every other mode ignores `session.budget_eur` entirely.
Also, contrary to the initial plan draft, the MILP's core-energy shortfall
comes from `session.target_soc` (via `core_kwh = (target_soc − current_soc) *
battery_kwh` in `ev_milp.rs`), **not** from `target_energy_kwh` — passing
`target_energy_kwh` alone in the POST body leaves `target_soc` at its default
(0.9) and has no effect on the MILP's core-energy computation for the EV
asset. So the actual POST body sets `mode: "MAX_COST"` and `target_soc`
(0.80, matching ven-11's own `soc_target`) rather than `target_energy_kwh`;
the scenario YAML's `target_soc` field reflects this. Confirmed the
mechanism itself already has coverage — `tests/features/ven_request_modes.feature`
scenario "MAX_COST budget shortfall raises a user notification" exercises the
exact same `mode=MAX_COST` + low `budget_eur` combination end-to-end.

**Plan-history + forecast-accuracy fetch**: `fetch_plan_history`/
`fetch_forecast_accuracy` pull `GET /history/plans`/`GET /history/forecast-accuracy`
per VEN (gated on `--fleet-map`, same as the existing `poll_plan_diagnostics`)
right after `run_window()`'s existing `snapshot()` call, same `t_from`/`t_to`
window. `poll_plan_diagnostics`'s docstring corrected — its "solver_ms would
need an SSE listener" note was stale since GB-25; it's now documented as the
live-progress fallback, not the analysis source of record.

**kpi.py**: new `plan_history_summary()` (reads `{ven}-plan-history.json`,
verified field-for-field against `entities::history::PlanHistorySample` —
`warning_kinds` serializes as SCREAMING_SNAKE_CASE strings e.g.
`"BUDGET_SHORTFALL"`, easy to consume as plain strings) computes
solver_ms/mip_gap_target-sanity/solve_status_counts/warning_kind_counts/cost
stats, all null-tolerant. New `forecast_accuracy_summary()` (reads
`{ven}-forecast-accuracy.json`, verified against `ForecastAccuracySample`)
groups by `(asset_id, lead_kind)` and computes MAE + bias (signed, to catch
systematic over/under-forecast direction), excluding unreconciled
(`actual_kw is None`) rows silently. `main()` tries `plan_history_summary()`
first, falls back to the existing `plan_diagnostics_summary()` (unchanged,
now documented as the fallback) when it returns `None` — the
`k["plan_diagnostics"]` key name is unchanged either way. Self-check
(`python experiments/kpi.py --self-check`) extended with synthetic fixtures
for both new functions, including a deliberately non-constant
`mip_gap_target` (flags a WARN, doesn't error) and a warning_count/
`len(warning_kinds)` mismatch check.

**Verification**: no local Docker available on this workstation (`docker
version` fails), so no local single-VEN stack was brought up. Verified
instead via (a) `python experiments/kpi.py --self-check` — passes, including
the new fixtures' assertions; (b) `python -m py_compile` on both changed
scripts; (c) careful cross-reference of every Rust field name/route/mode
condition this tooling depends on, reading the actual current-`main` source
rather than trusting the plan draft's field-name notes (which is how the
`target_soc` vs `target_energy_kwh` finding above was caught); (d) confirming
existing BDD (`ven_request_modes.feature`) and Rust unit coverage
(`milp_planner/tests/penalty.rs`, `solver.rs`) already exercise the
underlying `budget_warning`/`penalty_rules` mechanisms this tooling drives.
Live-fleet dry-run verification (confirming a real deploy actually fires
BudgetShortfall/PeakPenaltyExceeded through this new tooling) is deferred to
Part B (redeploy + run), not done here.

## Fleet run 2 (Part B): redeploy + S-1..S-8 live run (2026-08-14)

Live redeploy + run following the "Fleet run 2 tooling (Part A)" entry
above. Both hosts were confirmed stale before starting (`GET /plan` on
ven-1/ven-4 had no `solver_ms`/`mip_gap_target`, `/history/plans` 404'd) and
on unrelated leftover branches (`fix/gb-25-plan-history` on Node1,
`fix/sim-persist-plan-context-tests` on Node2, both clean working trees) —
switched both to `main` and pulled before rebuilding.

**Redeploy**: `docker_host_lock.sh` held on both Node1 and Node2
(`-l 600`) for the whole window. Rebuilt + redeployed Node1's VTN and VEN
(ven-1..3) compose projects and Node2's `VEN/scale_out/node2` (ven-4..13).
Verified before running anything: all 13 VENs healthy and VTN-connected,
`solver_ms`/`mip_gap_target` present on `GET /plan`, `GET /history/plans`
returns 200, and `ven-9`'s new `penalty_rules` block is live
(`penalty_rules_active` shows `s7-peak-guard`, threshold 0.3 kW).

**Two live bugs found and fixed forward, both merged to `main` mid-run**:

1. `poll_plan_diagnostics()` crashed (`AttributeError` on `None.get()`)
   whenever `GET /plan` returns a bare `null` — which every VEN does until
   its first plan cycle completes, i.e. every freshly-redeployed VEN. The
   exception silently killed the whole poller thread (all VENs, not just the
   one still warming up) a few seconds into the very first scenario. Fixed
   by treating a `null` plan body as an explicit "no plan yet" record instead
   of letting it raise. Required killing and relaunching the run once (the
   very first S-1 attempt); the resulting orphaned VTN program/event were
   found and deleted manually before relaunching.
2. `report_lag_stats()`/`event_impact_kwh()` only filtered recorder rows by
   `received_at` time window, not by which VTN event/program they actually
   belonged to. Node1's VTN is shared with other pre-existing test programs
   (e.g. a leftover `test-rd-check` fixture); its own periodic report
   traffic landed inside this run's time window purely by coincidence and
   got counted, producing `report_timeliness.min_s` values in the millions
   of seconds (a stale interval reference on that unrelated program's
   reports). Found by eyeballing S-1's `kpis.json` output after the run
   finished — every scenario's `min_s` was wildly wrong in the same way, a
   clear tell it wasn't scenario-specific. Fixed by threading an `event_ids`
   set (sourced from `run.json`'s own `"events"` list, already recorded by
   `run_experiment.py`) through both functions, filtering on each report's
   `payload_json.eventID`. Re-ran `kpi.py` for all 8 scenarios after the fix
   (no need to re-run the live fleet — the recorder CSVs were already
   snapshotted); `min_s` values are now all within the actual run window
   (-925 to -1213 s) across every scenario. Also fixed the same
   non-true-median bug (`s[len(s)//2]` vs `statistics.median()`) caught
   earlier in Part A's review, this time in `report_lag_stats`.

**Run**: all 8 scenarios (S-1..S-8, ~7h45m total — S-1 ran
`--no-paired-baseline` since it's the baseline itself) completed with
`exited rc=0`, no further errors. Orchestrated off-host as a single
sequential detached script (`nohup bash run_all_scenarios.sh`) rather than
launching each scenario as its own tracked process, specifically to avoid
the ScheduleWakeup-coordination race documented in the GB-25 Part B entry
below — one process, one log file, one line of monitoring truth. Progress
was checked periodically against the log plus a direct VTN `/programs`
lookup (the log is fully stdout-buffered when piped to a file, so it goes
quiet for the ~25-55 min a scenario is mid-window and only flushes at exit —
looked "frozen" repeatedly but never actually was).

**Results — did the new diversity/stats actually materialize?** Yes.
Aggregate `warning_kind_counts` across all 8 scenarios:
`PEAK_PENALTY_EXCEEDED: 5664` (ven-9's new `penalty_rules`, firing on every
plan cycle in every scenario exactly as designed — its 0.3 kW threshold sits
below ven-9's own 0.5 kW baseline with no controllable asset to shed it),
`CAPACITY_VIOLATION: 48` (S-3/S-4/S-6/S-7, as before), `BUDGET_SHORTFALL: 5`
(S-8 only — the new `budget_shortfall` action against ven-11 fired
correctly; confirmed independently via `ven-11`'s own `/user-requests`,
`mode: MAX_COST`, `budget_eur: 0.01`, status `CANCELLED` post-cleanup).
`SOLVER_INFEASIBLE`/`STALE_RATE_ESTIMATE`/`OTHER` stayed at 0 — expected
(nothing in this run's scenarios pushes the solver to genuine infeasibility
or a stale-rate condition). Compare to the 2026-08-12 run: `solve_status`
was `OPTIMAL` on all 390 samples with almost no warning diversity at all —
this run's data is substantially richer for exactly the plan-quality
questions GB-25 was built to answer.

`solver_ms`/`mip_gap_target_sanity`/`forecast_accuracy` all populated with
real data for the first time (e.g. ven-1/S-1: solver_ms median 2554.5 ms,
`mip_gap_target` constant at 0.02 as GB-31 documented, `pv:near` forecast
MAE 1.12 kW / bias -1.12 kW — a genuine PV under-forecast signal, not
previously visible anywhere in this tooling).

**Cleanup**: no orphaned VTN programs remained after the run (each
scenario's own `finally` block deletes its events/program); both
`docker_host_lock`s released; both hosts left on `main`. A leftover junk
result directory from the killed first S-1 attempt
(`20260814-1150-s1_flat/`, poller-crash JSONL only, no `run.json`) was
deleted before running `kpi.py` across the real 8.

**Third bug, found reviewing the results themselves**: `event_impact_kwh`
was exactly `0.0` for every VEN in every one of the 8 scenarios — uniform
enough across wildly different conditions (price spikes, capacity limits,
alerts, a deliberate budget shortfall) to be suspicious rather than a real
"well-calibrated forecast" result. Root cause: archived report intervals
carry the fully-qualified ISO 8601 duration form (`"P0Y0M0DT0H5M0S"`), not
the VEN's own compact `"PT5M"` the reporter actually emits — the VTN
round-trip normalizes it. `_parse_iso8601_duration_hours` only recognized a
leading `"PT"`, so every interval silently parsed as zero-length, zeroing
out the whole energy sum regardless of the real power values. Fixed with a
regex-based parser covering the full form; added a regression test (`P0Y...`
vs `PT...` both resolving to the same duration) to `--self-check`; re-ran
`kpi.py` on all 8 result directories (no live re-run needed). `event_impact_kwh`
now shows real, scenario-varied non-zero values (e.g. S-7 ranges -5.21 to
+6.36 kWh across the fleet). Recorded in `KEY_LEARNINGS.md` since it'll trip
up anything else that parses a duration from `lab_recorder`-archived data
rather than straight from the VEN.

**Not done / left open**: GB-24 (Node2 E2E-vs-fleet contention) — the lock
held for this run's duration mitigated it for this run specifically, still
open on the backlog. GB-31 (`mip_gap_target` proxy-not-achieved-gap) —
this run's `mip_gap_target_sanity` check confirms the known limitation,
doesn't address it.

## Reframe fleet-experiment KPIs around stakeholder goals (2026-08-15)

Reviewing fleet-run-2's warning counts with the user (PEAK_PENALTY_EXCEEDED
on every scenario, CAPACITY_VIOLATION on four of eight, one
BUDGET_SHORTFALL) surfaced that the report read as "the fleet is failing"
when most of that volume was actually by design: `ven-9`'s `penalty_rules`
threshold sits below its own fixed base load with no controllable asset to
shed with, `s8_budget.yaml`'s budget is 2-3 orders of magnitude under a real
EV charge's cost, and `s7_stress.yaml`'s capacity cap was chosen specifically
to force a violation — all three added in the previous round specifically to
prove the warning-kind *mechanism* fires end-to-end, a job that's done but
was never visually separated from "how does the fleet behave under plausible
conditions" in the report.

Separately, `kpis.json` never actually answered what each of the three
project stakeholders cares about: the grid operator (capacity-envelope
compliance, both import *and* export — the user flagged export as the more
pressing of the two, since an uncurtailed PV spike risks overvoltage/
appliance damage on top of grid stability, not just import overshoot), the
energy-business side (does the fleet actually track the tariff curve), and
the VEN/household side (what did participating cost in money or comfort).

**Implementation** (`experiments/kpi.py`, `experiments/run_experiment.py`,
scenario YAML — no VEN/VTN Rust changes; every new number is computed from
data already flowing into `history.sqlite`/`{ven}-plan-history.json`):

- `grid_envelope_compliance(db, t_from, t_to, direction)` and
  `compliance_latency_s(db, t_from, t_to, actions, direction)`, each called
  once per `"import"`/`"export"` direction. The latter finally implements
  the signal-to-response KPI this module's own docstring has described,
  unbuilt, since WP3.8.
- `tariff_response_correlation(db, t_from, t_to)` — Pearson correlation plus
  a cheap-vs-expensive-tercile % figure, plain Python (no new dependency).
  Returns `None` under 5 distinct price points, which every existing 30-min
  scenario is — the reason for the new 24h scenario below.
- `run_experiment.py` gained a new `export_capacity_limit` scenario action
  (the VEN already implements `EXPORT_CAPACITY_LIMIT` end-to-end; the
  experiment tooling had simply never exercised it), a per-action
  `run.json["actions"]` start-time log (feeds `compliance_latency_s`), a
  `tier: realistic|stress` passthrough from scenario YAML, and a
  `--start-at <ISO8601>` sleep-until option — needed because the simulator's
  PV tracks real solar position for the lab's actual coordinates
  (`docs/architecture/weather_forecast.md`, Europe/Zurich), so a diurnal
  scenario's scripted steps only land correctly if launched at a
  deliberately chosen wall-clock time, not whenever the script happens to
  run.
- All 8 existing scenarios tagged `tier:` (S-1/2/3/5/6 realistic, S-4/7/8
  stress). Two new scenarios: `s9_diurnal.yaml` (24h duck-curve price shape
  + evening import cap + midday export cap, `tier: realistic`, launched at
  local midnight) and `s10_overexport.yaml` (tight export cap during
  simulated peak PV, `tier: stress`, mirrors S-7's role for the export leg,
  launched pre-solar-noon).
- `kpi.py`'s `main()` restructured each VEN's entry into `raw` (the original
  flat metering numbers) plus four interpretive buckets: `grid_regulation`,
  `energy_business`, `ven_impact` (extended with a `cost_eur_delta` vs.
  baseline, `budget_shortfall_warnings` as the available comfort-shortfall
  proxy, and `compliance_cost_eur` — GB-25's already-collected
  `c_violations_eur`/`c_peak_penalty_eur`/`c_wear_eur` regrouped under the
  VEN's own viewpoint rather than left as raw plan-diagnostic numbers), and
  `mechanism_health` (the pre-existing plan/forecast diagnostics, explicitly
  separated so a stress-fixture's warnings stop reading as a fleet defect).
  `kpis.json`'s new top-level `meta.tier` lets a reader group by tier
  without re-parsing scenario YAML.
- `--self-check` extended with import- and export-direction fixtures for
  the two new grid functions plus a tariff-correlation case, all against a
  real temp SQLite `grid_samples` table (not a mock) to catch schema
  mismatches; all green.

**Verification against real data**: rather than wait for a new live run,
regenerated `kpis.json` for all 8 of fleet-run-2's existing result
directories with the new code — no crashes, backward-compatible with
`run.json` files that predate `"actions"`/`"tier"` (default `[]`/
`"realistic"`). `ven_impact`/`energy_business`/`mechanism_health` all show
real per-VEN numbers as expected.

**GB-33, found during this verification**: `grid_regulation.import`/
`.export` came back `null` for every VEN in every one of the 8 regenerated
runs — including S-3/S-6/S-7, which explicitly posted `IMPORT_CAPACITY_
LIMIT`/`RESERVATION` events. Traced to the data, not the new KPI code:
`grid_samples.import_limit_kw`/`export_limit_kw` (added schema v9) have
**never once been non-null**, on any VEN, across that VEN's entire
history.sqlite (confirmed on ven-1: 0 non-null rows out of 47k+). The
plumbing exists end-to-end (dispatcher tracks it, state stores it, the
history-sampler accumulator reads it) but the interval match never actually
fires at sample time, for either leg — not chased to a precise root cause
mid this Python-only change; filed as GB-33 (`docs/BACKLOG.md`) for its own
Rust debugging session. This is a real limitation on today's headline
finding: the grid-operator-facing KPI the user called the most pressing
question is implemented and self-check-verified, but returns nothing
against live data until GB-33 is fixed.

**Not done / left open**: running `s9_diurnal.yaml`/`s10_overexport.yaml`
live against the fleet (a real time/lock-hold commitment, scheduled as its
own follow-up step); GB-33 itself; a true SoC-deadline-miss comfort metric
(would need new VEN session-outcome instrumentation, `BUDGET_SHORTFALL`
counts used as the available proxy for now); the 30-day-scale test the user
flagged as the longer-term goal beyond the 24h scenario.

## GB-32: BFF `report_lag_s` duration parser — same sibling bug as `kpi.py`'s, server-side

**Trigger:** The `kpi.py` duration-parsing fix above (leading-`"PT"`-only
parser silently zeroing every fully-qualified-form duration) prompted
checking whether the BFF has the same bug in its own duration parsing —
`VTN/bff/src/recorder.rs::parse_pt_duration_s` feeds
`report_submission_lag_s`, archived as the `report_lag_s` column in
`lab_recorder.reports_received` (the SG-3 timeliness metric). It did: same
`"PT"`-only prefix check, same silent `0` on the VTN's fully-qualified
`P[n]Y[n]M[n]DT[n]H[n]M[n]S` form, same root cause (`record_reports` fetches
reports back from the VTN via `GET /reports`, which normalizes durations to
the full form, rather than reading the VEN's raw POST body which uses the
compact `"PT5M"` form). Every `report_lag_s` value ever recorded to date was
silently wrong (`window_end` collapsed to `interval.start` instead of
`interval.start + duration`, an error of up to one report interval,
300–900s typical).

**Design:** Considered reusing `openleadr_wire::Duration`'s own parsing, but
the BFF has zero dependency on any openleadr-rs crate today (it talks to the
VTN as raw `serde_json::Value` over HTTP), and the submodule wasn't checked
out in the working worktree — adding that dependency sight-unseen for this
fix would be more scope and risk than the fix itself. Instead mirrored
`kpi.py`'s fix *shape* (recognize the full form, approximate Y/M as 0 since
report intervals are minute/hour-scale and never populate them) without
adding a `regex` crate dependency, since the BFF has none today and this
project's dependency-review rule applies to every new import. Extracted a
small `sum_digit_units` helper (walk one duration segment's chars,
accumulate digits, multiply by the unit each digit run's trailing letter
maps to) and called it once for an optional date segment (before a `'T'`
split, `D` → 86400s) and once for the time segment (`H`/`M`/`S`, same
mapping as the original code) — a natural two-phase extension of the
original hand-rolled loop, no new crate.

**Implementation:** `VTN/bff/src/recorder.rs` — replaced
`parse_pt_duration_s`'s body and updated its doc comment to describe the
full form; added `sum_digit_units` as a shared helper. Extended (not just
supplemented) the existing 4 unit tests per this project's test-first
discipline: `test_parse_pt_duration_s_variants` gained 3 full-form
assertions (`"P0Y0M0DT0H5M0S"` → 300, `"P0Y0M0DT1H30M0S"` → 5400,
`"P0Y0M1DT0H0M0S"` → 86400, exercising the new `D` handling) alongside the
existing compact-form ones; the 3 `report_submission_lag_s` tests had their
`"duration"` fixtures changed from compact to full form (the real shape the
VTN actually returns), with identical expected lag values — the fix changes
which input shapes parse correctly, not the intended lag semantics.

**Verification:** `wsl cargo test -p vtn-bff` (all 13 `recorder` tests
green, including the updated fixtures), `cargo fmt --check`, `cargo clippy
--all-targets --all-features -- -D warnings` (clean), `scripts
/audit_file_sizes.py` (pass).

**Bookkeeping:** Removed GB-32's row from `docs/BACKLOG.md`. **Known caveat,
not addressed by this fix:** already-archived `report_lag_s` rows in
`lab_recorder.reports_received` (recorded before this fix landed) remain
silently wrong — this change only fixes parsing going forward, it does not
backfill or correct historical rows. Anyone analyzing historical SG-3
timeliness data should treat pre-fix `report_lag_s` values as unreliable.

## GB-33: capacity-limit schedule silently dropped every experiment event (2026-08-16)

Root-caused and fixed the gap flagged as GB-33 in the previous entry:
`grid_samples.import_limit_kw`/`export_limit_kw` had never once been
populated on any VEN, despite scenarios like S-3/S-6/S-7 posting real
`IMPORT_CAPACITY_LIMIT` events and the planner correctly enforcing them
(`CAPACITY_VIOLATION` warnings fired as expected — the real constraint
pipeline was never affected, only this history column).

**Root cause**, traced via `poll_events/detect.rs` → `parse_capacity_schedule`
→ `rate_schedule.rs`'s shared `collect_interval_groups`: that function
required each `interval` to carry its own `intervalPeriod`
(`interval.intervalPeriod.as_ref()`, `None => continue`), with no fallback to
the event-level `intervalPeriod`. `experiments/run_experiment.py`'s
`build_event()` — and, per the OpenADR 3 spec, any single-window capacity/
alert/dispatch event — sets `intervalPeriod` only at the event level for a
single bare interval, exactly the shape every non-price scenario action in
this project sends. Price events worked fine because `price_series` always
gives each interval its own `intervalPeriod`, which masked the gap for
months: only the capacity-schedule/tariff-schedule path (`collect_interval_
groups`) was missing it, while `parse_alert_windows` (a few functions away
in the same file) already had the correct `interval.intervalPeriod.as_ref()
.or(event.intervalPeriod.as_ref())` fallback — the fix pattern already
existed in the codebase, just not applied everywhere it needed to be.

**Fix** (`VEN/src/controller/rate_schedule.rs`): fall back to the event-level
`intervalPeriod` only for the single-interval case — a multi-interval event
without per-interval periods has spec-ambiguous sequential timing nothing in
this project emits or needs, so the fallback deliberately doesn't guess at
it. Two regression tests added to `openadr_interface.rs` (test-first: written
and confirmed failing before the fix, per project convention) — one
asserting the real single-interval/event-level shape now round-trips
correctly, one asserting the multi-interval case still returns nothing
rather than guessing. Both pre-existing `parse_capacity_schedule` tests
(which always gave each interval its own `intervalPeriod`) kept passing
unchanged — the gap they never covered is exactly what the new tests close.

**Verification:** full `cargo test` (1028 passed, 0 failed), `cargo fmt
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
`scripts/audit_file_sizes.py` all clean. Not yet re-verified against a live
run (would need a fresh scenario execution to populate real
`grid_samples.import_limit_kw`/`export_limit_kw` rows) — the KPI reframe's
`grid_regulation` block should now populate correctly once one runs; that
check is deferred to whenever `s9_diurnal`/`s10_overexport` (or any of
S-3/S-6/S-7) next actually executes against the live fleet.

**Bookkeeping:** Removed GB-33's row from `docs/BACKLOG.md`.

## Fleet re-run S-9 (24h diurnal): GB-33 verification on live data (2026-08-17/19)

Following GB-33's fix (interval-schedule event-level `intervalPeriod`
fallback, merged 2026-08-17), the fleet-experiment stakeholder-KPI-reframe
tooling had never been exercised against a real live run — the whole point
of GB-25/GB-33's work was to make `grid_regulation` finally populate with
real import/export capacity-compliance data, and that needed an actual
scenario execution to confirm, not just unit tests.

**Redeploy**: both hosts rebuilt from current `main` (Node1 was 15 commits
behind; Node2's git checkout already matched `main` but its containers were
still running images built 2 days earlier — a reminder that a matching
`git rev-parse HEAD` is not proof of matching running code, only a rebuild
+ binary-timestamp check is). Both verified healthy, `GET /history/plans`
returning 200, before touching the scenario.

**Sequencing**: `s9_diurnal` (24h) and `s10_overexport` cannot run
concurrently with anything else on this fleet — `run_experiment.py`'s
events are program-wide and this project's VTN hands every polling VEN
every active event regardless of which program it belongs to (the same
mechanism behind fleet-run-2's "leftover test-rd-check program's reports
leaked into our time window" finding). Two overlapping scenarios on the
same 13 VENs would contaminate each other's price/capacity signals. Given
the timing (redeploy finished with only ~90 min left before that night's
local midnight — not enough for S-1..S-8 first), `s9_diurnal` was launched
alone that night; S-1..S-8/S-10 are deferred to a follow-up run.

**A real bug in this session's own launch, caught immediately**: the first
launch attempt crashed on its very first HTTP call — `--vtn-url` was never
passed, so it defaulted to `localhost:8200` (the orchestration workstation)
instead of Node1's actual VTN. Caught within minutes via the log
(`ConnectionRefusedError`), fixed, and relaunched ~6 minutes past the
intended midnight start — negligible drift for a scenario whose price steps
are 4 hours apart.

**Lock-holding strategy**: rather than hold a single ~26h lock spanning the
idle gap before the scenario's actual start, the process was launched
detached (`--start-at` sleeping internally) *without* holding the lock, and
the lock was acquired only ~6 minutes before the scenario's actions began
posting — avoiding blocking other sessions' use of Node1/Node2 during a
period when nothing was actually happening yet. Monitored hourly for the
full 24h (process liveness, VTN program/plan cross-checks rather than
trusting a possibly-buffered log, container-timestamp/git-HEAD integrity
checks to catch a concurrent redeploy before it could corrupt the run) —
mid-run, the user asked to release the locks, which was done, and a
subsequent `git rev-parse HEAD` check confirmed another session's `git
pull` landed on Node2 during that gap but did **not** trigger a rebuild/
restart (container `CreatedAt` timestamps unchanged) — the run's data was
never actually at risk, though the exposure was real and the locks were
re-acquired for the remaining ~3.5h once asked to.

**Result — GB-33 confirmed fixed on live data**: `grid_regulation.import`/
`.export` now populate with real, non-`null` compliance and latency data
for all 13 VENs, matching the scenario's exact action windows (240/241
samples ≈ the 240-min import-cap window, 300 ≈ the 300-min export-cap
window). `compliance_latency_s` also shows real, distinguishable values
(e.g. ven-1: `0.0s` for the import cap — already compliant when it started
— vs. a genuine `246.0s` detection latency for the export cap). All 13 VENs
stayed 100% compliant with 0 overshoot on both caps — expected for this
`tier: realistic` scenario (caps were sized to be achievable, unlike
S-7/S-10's deliberately-forced stress-tier limits), not a sign anything is
broken.

**A genuine, unflattering finding, reported as-is — then investigated and
explained (2026-08-19)**: `energy_business.tariff_response.pearson_r` came
out slightly *positive* for ven-1 (`0.126`, weak) with
`cheap_vs_expensive_pct: -28.2%`, at first glance the opposite of good
demand-response behavior. Follow-up investigation across all 13 VENs'
per-VEN `tariff_response` (not just ven-1, the one quoted here) found the
sign split roughly evenly (5 anti-correlated, 5 correctly correlated, 3
flat) rather than being a fleet-wide failure, and traced the two real
drivers:

- ven-1 specifically runs on real MQTT weather/load ground truth
  (`measurements.pv_enabled: true` in its profile), not the modeled PV
  curve every other VEN uses — its result reflects actual Zunzgen weather
  on 2026-08-16/17, not the idealized diurnal shape the scenario assumes,
  and shouldn't be read as representative of the fleet.
- Every EV-touching VEN's correlation was noise, for a root cause found by
  reading `VEN/src/assets/ev_milp.rs::EvMilpContext::from_state`: without
  an active `EvSession` (created only via `POST /user-requests`, which
  `run_experiment.py` only does when launched with `--personas`), the EV's
  MILP mode is `MustNotRun` — the solver is hard-constrained to zero
  charging power for every slot, the whole run. This S-9 launch did not
  pass `--personas`, so **no EV in the fleet was ever deliberately charged
  by the optimizer**; each just sat plugged in at its `initial_soc` for 24h.
  Every EV-touching correlation number (ven-1, 3, 5, 7, 11, 12) is an
  artifact of that (most likely the `initial_state()` bootstrap setpoint
  before the first plan lands), not a real optimizer decision — confirming
  it really did look random, because no optimization was happening.

Batteries and heaters are *not* session-gated, so their correlations (all
negative/correct: ven-4, -6, -10, -12, -13; ven-1 aside) are real and
trustworthy. Re-testing EV price-response isn't as simple as adding
`--personas`, either: `scripts/personas.py`'s default persona mix is mostly
`OPPORTUNISTIC`/`ASAP_FREE`, which route the EV's reward off real-time PV
surplus (`v_ev_free_charge_eur_kwh`), never the actual grid tariff — for a
non-PV EV VEN those modes charge exactly nothing in a scenario with no
zero/negative price slot, same dead-EV outcome as no session at all. Only
`BY_DEADLINE`/`ASAP` sessions route the real per-slot tariff into the EV's
MILP reward. No controller code change is warranted here — this is a test-
harness gap (see GB-37, `docs/BACKLOG.md`), not a bug in `ev_milp.rs`.

**GB-36, a new bug found while sanity-checking `report_timeliness`**:
`report_timeliness` came back with all-negative lag values (median
`-43119.8s`), distinct in shape from the already-known GB-32 (which skews
lag by at most one report interval, not tens of thousands of seconds).
Traced to `VTN/bff/src/recorder.rs`'s `report_submission_lag_s`: it
computes lag as `created − max(interval_end)` across every interval
currently present in a report resource, correct for a report created fresh
per submission, but openleadr-rs appears to grow a single long-lived report
resource by appending intervals over its lifetime without updating
`createdDateTime` — so lag drifts increasingly negative the longer the
report resource lives, a signature only visible once a scenario runs long
enough to accumulate many intervals in one report (confirmed: values
progressed in ~300s steps matching the report's own frequency).
`kpi.py`'s own `event_ids` filtering was independently confirmed correct
during this investigation (count 7410 = 285 samples × 13 VENs × 2 report
types, exactly matching this run's own event — other programs' reports,
some off by *millions* of seconds, were correctly excluded). Filed as
GB-36, not fixed — needs a design decision on what the metric should
actually measure for a growing report resource before touching
`recorder.rs`, not a one-line parse fix like GB-32 was.

**Cleanup**: no orphaned VTN program/events after the run's own `finally`
block ran; a leftover empty result directory from the crashed first launch
attempt (`20260817-2016-s9_diurnal/`) removed; both locks released.

**Not done / left open**: `s1_flat` through `s8_budget` and `s10_overexport`
— deferred to a follow-up run, this session only covered `s9_diurnal`.
GB-36 (new). GB-31/GB-24 remain open as before. Re-testing genuine EV
price-response needs a follow-up `s9_diurnal` run launched with
`--personas` — see GB-37 in `docs/BACKLOG.md`.

---

## S-9 re-run #2 with live EV sessions (2026-08-20/21): GB-37 partly proven, and a solver-timeout finding

**Why**: the first GB-37 fix (roster + `--ev-session-mode`, merged 2026-08-19/20)
was self-check-verified but never proven against the live fleet. This run was
that verification: `s9_diurnal` again, 24h from local midnight
(`--start-at 2026-08-20T22:00:00Z --no-paired-baseline --ev-session-mode
BY_DEADLINE --ev-roster experiments/fleet_ev_roster.json --fleet-map
experiments/fleet_map.json`). No redeploy was needed — GB-37 only touched the
off-host orchestration script, not anything running in the VEN containers.

**Pre-flight, and a mistake worth recording**: a 30-min `s2_price_spike` smoke
test first (the new session path had never hit a live `/user-requests`). It
found 4 of 6 roster EVs at/above the 0.8 target SoC, rejected with a correct
422 (`computed target_energy_kwh is zero or negative`). Those four
(ven-1/3/5/7) were reset to 0.3 via `POST /sim/reset/ev` and re-verified 201.
**The mistake**: ven-11 and ven-12 had *succeeded* in the smoke test, so they
were left alone — but succeeding meant they had charged, and by S-9 launch
ven-11 sat at 79.9998% against a 0.8 target. Its session was created (it
squeaked past the `>1e-6 kWh` check) but had essentially zero energy to
deliver, so its EV never moved all run. Resetting only the VENs that *failed*
was the error; the fix is to reset all of them unconditionally, now built into
the script (`--ev-reset`, default on).

**GB-37's core claim is proven — partly.** Two VENs charged for real, and in
the right place: ven-1 at the full 7.4 kW (SoC 30 → 45.9%) and ven-7 at 1.93
kW (30 → 36.6%), both between ~13:00 and 17:36 UTC — inside the day's cheapest
price block (12:00-16:00, €0.08) and the west-facing PV peak. Nothing like
this happened in the first S-9 run, where every EV was inert by construction.

**But three VENs never charged at all, for a different reason.** ven-3, ven-5
and ven-12 held valid active sessions at 30%/30.07%/25% SoC and charged
nothing. Their plan diagnostics explain why:

| VEN | avg solve | in-run solve_status | EV charged |
|---|---|---|---|
| ven-1 | 5.5 s | OPTIMAL 839, GAP_LIMIT 580 | yes, 7.4 kW |
| ven-7 | 0.13 s | fine | yes, 1.93 kW |
| ven-3 | 112 s | TIME_LIMIT 1363, INFEASIBLE 56 | no |
| ven-5 | 115 s | TIME_LIMIT 1237, INFEASIBLE 14 | no |
| ven-12 | 63 s | TIME_LIMIT 1419 (every solve) | no |

`solver_timeout_s` defaults to 60 s and the solve is two-phase, matching the
observed ~120 s ceiling. These VENs are not chronically too slow: ven-12 has
1163 OPTIMAL solves in its lifetime `plan_history` and not one during this run
window, so something specific to the run broke them.

**Root cause (strongly supported, not fully confirmed)**: sessions were created
with `--ev-deadline-hour-utc`'s then-default of 7, i.e. `2026-08-21T07:00:00Z`
— only ~9h into a 24h run, and *before* the cheapest block. Once a deadline
passes, `ev_milp.rs::from_state` collapses `t_dead` to slot 0, and because the
harness never set `soft_deadline` it defaulted to false → `EvMilpMode::MustRun`
→ the hard equality `ev_energy == e_core_kwh + e_ev_extra` demands the whole
25-30 kWh inside one expired slot. Infeasible by construction, which fits the
TIME_LIMIT storm and the INFEASIBLE counts. **The unexplained part**: ven-1 and
ven-7 charged at 12:57-17:36, *after* that same expired deadline. That does not
fit the mechanism cleanly, so this is the leading explanation rather than a
closed case — a clean re-run showing all six EVs charging is what would confirm
it.

**Fixed forward** (commit on `fix/gb-37-ev-deadline`, not yet re-run):
- `resolve_ev_deadline()` — the deadline now defaults to the scenario's own end
  (`start + duration_minutes`) instead of an absolute wall-clock hour, so it
  cannot expire partway through the window. `--ev-deadline-hour-utc` still
  overrides for scenarios that genuinely want a wall-clock deadline.
- `soft_deadline` defaults true (`MayRun`), so an unmeetable deadline degrades
  to "charge what you can" plus a plan warning instead of an infeasible hard
  equality. `--ev-hard-deadline` opts back in.
- `reset_ev_soc()` — every roster EV is reset to `--ev-initial-soc` (0.3)
  before the run and again before the scenario window when a paired baseline
  ran, so both windows start from the same state and no run inherits the
  previous one's SoC. This is the systematic version of the manual reset above.

**KPI note**: `tariff_response` improved for some VENs (ven-1 +0.126 → -0.094,
ven-3 -0.058) but ven-11 was unchanged (+0.371 → +0.374), consistent with its
EV having been stuck at target. Given three VENs' planners were timing out and
one EV was inert, **this run's fleet-wide `tariff_response` should not be read
as a valid measurement** — it is a diagnostic of the above, not a result.

**Not done / left open**: `s1_flat`..`s8_budget` and `s10_overexport` still
deferred — deliberately held rather than launched into the same solver-timeout
condition. GB-36 and GB-31/GB-24 remain open as before.

## S-1..S-8 + S-10 against the 20-VEN fleet (2026-08-23/24)

Seven new VENs (ven-14..20) were deployed to Node2, bringing the hand-authored
fleet from 13 to 20. `experiments/fleet_map.json`, the Node2 docker-compose,
`experiments/fleet_ev_roster.json` (now 9 EV-bearing VENs: the original 6 plus
ven-16/18/19), and `docs/guidelines/FLEET_EXPERIMENT_DESIGN.md` were already
updated as part of that deploy — the only gap found was a stale "(all 13)"
string in `run_experiment.py`'s `--fleet-map` help text (fixed,
`b0b805c`). A live smoke test against all 20 VENs via `--fleet-map` confirmed
the harness needed no other changes, so the plan was simply: run the deferred
S-1..S-8/S-10 batch (9 scenarios) against the bigger fleet, then S-9.

**Results — fleet totals across 20 VENs:**

| Scenario | Import (kWh) | Export (kWh) | Cost (EUR) | Peak (kW) | Shift vs baseline (kWh) | Warnings | Max violation (EUR) |
|---|---|---|---|---|---|---|---|
| S-1 flat (baseline) | 6.556 | 0.094 | 1.297 | 1.064 | -- | 582 | 161505.68 |
| S-2 price_spike | 6.676 | 0.028 | 0.932 | 3.702 | -0.455 | 582 | 161862.04 |
| S-3 capacity_limit | 6.375 | 0.000 | 0.638 | 3.600 | -0.126 | 586 | 162199.58 |
| S-4 alert | 6.022 | 0.000 | 0.482 | 3.600 | +1.382 | 786 | 1000.00 |
| S-5 dispatch | 11.753 | 0.000 | 1.578 | 7.881 | -1.792 | 864 | 0.38 |
| S-6 combined | 10.212 | 0.015 | 0.741 | 7.881 | +9.082 | 880 | 10733.86 |
| S-7 stress | 8.484 | 0.011 | 0.753 | 8.381 | +10.965 | 908 | 132419.81 |
| S-8 budget | 8.032 | 0.000 | 1.288 | 7.881 | +5.716 | 580 | 0.00 |
| S-10 overexport | 4.613 | 0.733 | 0.488 | 2.150 | +7.690 | 864 | 0.34 |

Same qualitative shape as the 13-VEN run: S-2's price signal alone barely
moves the fleet total; capacity/alert/stress scenarios (S-3/S-4/S-6/S-7) show
real import reduction and real `CAPACITY_VIOLATION`s; S-8's `BUDGET_SHORTFALL:
4` confirms the direct `/user-requests` action fired against ven-11 as
designed. `PEAK_PENALTY_EXCEEDED` dominates every scenario's warning count
(576-864 of each total) — this is ven-9's deliberately-thin `penalty_rules`
threshold (0.3 kW, below its own no-asset baseline) firing every cycle by
design, not a regression; the `max_violation_eur` figures inherit the same
uncapped-penalty-accumulation character noted in the 13-VEN run and should be
read as "the diagnostics tooling is working," not as literal currency.

**New finding — `TIME_LIMIT` solve status is now pervasive, fleet-wide, and
unrelated to EV/GB-38.** Every one of these 9 scenarios shows a substantial
`TIME_LIMIT` share of total solve cycles across the fleet (roughly 30-50%,
e.g. S-10: 62 of ~154 cycles, S-7: 48 of ~129, S-1: 32 of ~108) — none of
which use `--ev-session-mode` at all, so this cannot be GB-38's expired-EV-
deadline mechanism (no EV sessions exist for S-1..S-8/S-10; the routine
`ev-reset` calls only set SoC, they don't create sessions). This looks like a
fleet-scale solver-contention effect specific to running 20 VENs' planners
concurrently rather than 13.

**Cause identified — Node2 CPU saturation, not memory.** The under-load
capacity capture was taken during the S-9 run (2026-08-25T18:05Z) and refutes
the memory-contention theory this entry originally proposed:

| | Node1 (VTN + ven-1..3) | Node2 (ven-4..20) |
|---|---|---|
| cores | 4 | 4 |
| load avg (1m) | 1.41 | 3.64 |
| CPU idle | 67% | 5-7% |
| available RAM | 1.9 GiB | 2.0 GiB |
| swap in/out (`vmstat` si/so) | 0 / 0 | **0 / 0** |

Memory is not the constraint: `si`/`so` are flat zero on both hosts and ~2 GiB
stays available on each, so the 204-229 MB of resident swap is stale carry-over,
not active thrashing. **CPU is** — Node2 runs 17 VEN containers on 4 cores at
85-89% busy with a run queue of 4-5, and at the moment of capture three VENs
were each burning most of a core simultaneously (ven-14 94.6%, ven-20 93.2%,
ven-17 86.6%; every other VEN under 12%). Since `solver_timeout_s` is
wall-clock, a MILP solve that only gets a fraction of a core hits the timeout
before proving optimality — which is exactly `TIME_LIMIT`, and explains why the
symptom is fleet-wide and scenario-independent rather than tied to any signal.

**And the CPU is going to heaters (GB-40).** The "three hot VENs all have
heaters" hunch from the capture above was worth measuring properly, so per-VEN
mean solve time was taken across the whole fleet from one S-7 window
(`20260824-0312-s7_stress/*-plan-history.json`, 129 solves):

| | n | mean solve |
|---|---|---|
| VENs with a heater | 10 | **84.2 s** |
| VENs without | 10 | 18.0 s |

A 4.7× difference, and the **eight slowest VENs in the fleet all carry a
heater** — ven-5 121.4 s, ven-17 121.3 s, ven-3 120.2 s, ven-15 106.3 s,
ven-14 105.4 s, ven-18 83.3 s, ven-10 70.2 s, ven-12 67.1 s. The top six sit
at the `solver_timeout_s: 60` two-phase ceiling (~120 s), i.e. they time out
nearly every cycle. That is self-reinforcing: a `TIME_LIMIT` solve consumes its
*entire* budget before giving up, so the slowest VENs are also the most
expensive, starving the others into timing out too — which is why the symptom
appeared abruptly at 20 VENs rather than degrading gently.

Heaters are not sufficient on their own, though: ven-2 (heater+PV) at 18.2 s
and ven-20 (heater-only) at 29.0 s are both fast. The expensive cases pair a
heater *with* other flexible assets, pointing at the heater's integer
relay/staging variables interacting with the continuous ones rather than the
heater formulation in isolation.

**Isolated measurement: 561×, and it is a timeout, not a slowdown.** A
benchmark (`VEN/src/controller/milp_planner/tests/solve_cost.rs`, `#[ignore]`d)
solves the same ven-3-shaped site on the same 288-slot grid with and without
the heater, nothing else changed:

| | solve |
|---|---|
| without heater | **0.19 s** |
| with heater | **108.55 s** |

The with-heater figure sits at the two-phase `solver_timeout_s` ceiling — an
*active* heater doesn't slow the solve, it **times it out**. The fleet's
gentler 4.7× is an average diluting active heaters with idle ones (a
`MustNotRun` heater has every `z` fixed to 0, so there is nothing to branch
on), which also explains fast ven-2/ven-20. Debug build, but immaterial: the
no-heater case at 0.19 s shows Rust-side constraint building is negligible, so
the 108.55 s is essentially all HiGHS branch-and-bound.

**This retires GB-38's root cause.** The S-9 re-run's unexplained wrinkle —
three VENs timing out and never charging their EVs, while ven-1/ven-7 charged
normally *after the same expired deadline* — was never explained by the
expired-deadline theory. Heater presence explains all six EV-roster VENs
exactly:

| VEN | assets | heater | outcome |
|---|---|---|---|
| ven-3 | ev, heater, pv | yes | TIME_LIMIT, never charged |
| ven-5 | ev, heater, pv, battery | yes | TIME_LIMIT, never charged |
| ven-12 | ev, heater | yes | TIME_LIMIT, never charged |
| ven-1 | ev, pv, battery | no | charged 7.4 kW |
| ven-7 | ev, pv | no | charged 1.93 kW |
| ven-11 | ev | no | blocked by the separate SoC-reset mistake |

Six for six. The EVs never charged because their *planner* never produced a
plan — not because a deadline had expired. The GB-37 deadline fix was still
worth making, but it was not the fix for this. Confirmation comes free from the
S-9 re-run already in flight: it carries the deadline fix, so if the same
heater VENs still time out, the deadline was never the cause.

> **Retracted the next day — the paragraph above is wrong.** The S-9 re-run
> (below) refutes it: the same heater VENs *did* still time out, and charged
> their EVs to target regardless. A `TIME_LIMIT` solve still yields a feasible
> incumbent, so it never blocked charging. The "six for six" was coincidence on
> n=6, and the deadline fix was in fact the thing that made those EVs charge.
> Kept rather than deleted because the reasoning error is the lesson: a perfect
> correlation across six samples, with a mechanism that sounded right, still
> wasn't causation — and the disconfirming test was already running when the
> claim was committed.

## S-9 re-run #3 (2026-08-24/25): GB-38 closed, GB-40 confirmed, GB-41 opened

Third 24h diurnal run, first one carrying GB-37's `resolve_ev_deadline` +
`soft_deadline` fixes, against the 20-VEN fleet. All nine EV-roster sessions
were created with the deadline correctly resolved to the scenario's own end
(`2026-08-25T22:05:35Z`, `soft=True`), and `reset_ev_soc` verifiably worked —
every roster EV starts the window at exactly 30.0%.

**GB-38 is closed, and the deadline really was the cause.** ven-3 and ven-5
charged 30 → 76.6% and 30 → 79.5%, having charged *nothing* in the previous
run. That is the deadline fix working.

**GB-40 is confirmed, and its consequence is now bounded.** Across 5111 solves:

| | n | solves | TIME_LIMIT |
|---|---|---|---|
| heater VENs | 10 | 2332 | **1643 (70%)** |
| no-heater VENs | 10 | 2779 | 226 (8%) |

A ~9× split — the clearest signal in the fleet dataset, and consistent with the
561× isolated benchmark. But `TIME_LIMIT` **degrades cost-optimality, not
function**: ven-3 (209/210 TIME_LIMIT, avg 118.6 s) and ven-5 (203/210, avg
118.3 s) both charged to target anyway, because a timed-out MILP still returns
a feasible incumbent. That is the specific point the retracted paragraph got
wrong.

**GB-41 (new, unexplained).** Four of nine EV VENs charged *nothing* — flat
30.0% for all 1440 in-window samples:

| VEN | heater | solve_status | avg solve | EV SoC |
|---|---|---|---|---|
| ven-1 | no | GAP 138 / OPT 21 / TL 101 | 36.8 s | 30 → **80.0** |
| ven-3 | yes | TL 209 / INF 1 | 118.6 s | 30 → **76.6** |
| ven-5 | yes | TL 203 / INF 7 | 118.3 s | 30 → **79.5** |
| ven-7 | no | GAP 213 / OPT 74 | 4.3 s | 30 → **78.7** |
| ven-19 | no | GAP 105 / OPT 33 / TL 112 | 50.3 s | 30 → **78.8** |
| ven-11 | no | **OPT 291 (all)** | **0.21 s** | 30 → 30 |
| ven-12 | yes | TL 240 (all) | 64.3 s | 30 → 30 |
| ven-16 | no | GAP 148 / OPT 117 / TL 9 | 19.8 s | 30 → 30 |
| ven-18 | yes | GAP 57 / TL 178 / INF 1 | 70.4 s | 30 → 30 |

Neither known problem explains it. ven-11 solved **OPTIMAL on every one of 291
cycles in 210 ms** and still never charged; ven-3/ven-5 timed out constantly and
charged fine. So it is not solver cost, and not the deadline. Two of the four
have heaters and two do not. A PV correlation is tempting (four of five
chargers have PV, three of four failures lack it) but ven-18 has PV and failed,
so it is already broken at n=9 — recorded as an observation, not a theory.
GB-41 tracks it; the first step is separating planner from dispatcher (did those
plans ever contain non-zero `p_ev_kw`?).

**Methodology note — in-window filtering is not optional.** The first pass at
these SoC deltas read each VEN's whole `tick_samples` table and produced
nonsense (ven-16 "40 → 30", i.e. apparently discharging; ven-1 "+40" instead of
+50), because the snapshot databases carry weeks of history from before the run
— ven-1's goes back to 07-11. Only after filtering to `[started_at,
started_at+duration)` did every VEN correctly show the 30.0% reset value at
t=0, which is itself the check that the filter is right. Any future per-VEN
history analysis must filter by the run window first and verify that the
starting values match the known reset state.

**Rebalancing onto Node1 was considered and rejected.** Node1's 67% idle is not
spare capacity: it runs pihole (network DNS *and* NTP), mosquitto, openvpn,
influxdb, telegraf, the hargassner-* biomass boiler control, house_coordinator
and the data-acquisition stack — ~25 containers. Idle headroom on a
latency-sensitive host is burst capacity, not a harvestable resource, and MILP
solves are exactly the CPU-saturating batch work that would sit on top of DNS
and boiler-control latency. The blast radii are not comparable either: losing
Node2 costs a test run, losing Node1 costs house DNS and heating. It is also
self-defeating — Node1 serves the NTP these absolute-timestamp measurements
depend on. Node2 stays the only VEN host.

Remedies, in order of appeal: fix the heater formulation (GB-40 — addresses the
cause); raise `replan_interval_s` to 600 s (halves fleet solver CPU, 2.9 → 1.5
expected concurrent solves, costs no VENs and no host changes, but treats load
rather than formulation); shrink the fleet (works, but the seven new VENs were
added deliberately to fill asset-mix gaps and per-category KPIs are already
thin at 20); raise `solver_timeout_s` (weakest — lengthens every cycle and
makes the contention worse).

**Methodology, four incidents during this run:**

1. **Tool-tracking false-kill.** The Bash tool's own background-task tracking
   reported the first S-1 launch attempt as "killed" while the underlying
   process actually kept running — but with its stdio apparently torn down,
   because every subsequent VEN snapshot/recorder-dump call failed silently
   (`res.stderr` empty, `res.returncode != 0`) even though the same `scp`
   command worked fine when run manually moments later. Fixed by discarding
   that run's data and relaunching every subsequent invocation as a fully
   detached `nohup ... & disown` process, polled via `wmic`/log-tail/`netstat`
   directly rather than the tool's own background-task tracking.
2. **A timing false alarm (S-4).** Misread a scenario's snapshot-dir name
   (which reflects when a window *starts*) as its completion time, and
   concluded S-4 was stuck ~2h into what should have been a ~60 min
   paired-baseline run. It wasn't — verified via `netstat` (live ESTABLISHED
   connections to the VTN and a VEN, i.e. active polling) and the process
   finished normally shortly after.
3. **A genuine mid-run network blip.** `WinError 10051` (network unreachable)
   crashed the process mid-S-5, during `ev-reset`/`get_token` — the laptop's
   own network dropped briefly and recovered within minutes (`ping`/`curl`
   confirmed both hosts healthy again immediately after). S-1..S-4's data was
   already safely snapshotted; S-5's corrupted partial dirs (baseline good,
   scenario-window data was two stub lines) were discarded and a resume
   script picked up from S-5 through S-10 cleanly.
4. **Snapshot/scp overhead is tens of minutes, not seconds.** The gap between
   one scenario's nominal end and the next one's dirname timestamp grew to
   over an hour at points later in the run (S-5→S-6) — not a hang, but the
   time `snapshot()` spends scp-ing every VEN's `history.sqlite` (some VENs,
   especially ven-1 with real-weather data, exceed 60-70 MB and grow over the
   run) plus three recorder-table dumps, sequentially, over the network, for
   20 VENs instead of 13. Confirmed each time by checking for a live,
   growing result directory (recent mtime, increasing `wc -l` on a
   `plan-diagnostics.jsonl`) rather than assuming a stall from dirname age.

**Not done / left open**: S-9 (24h diurnal, the GB-38 verification run) is
scheduled for the next Zurich local midnight — held back from tonight because
this batch didn't finish before then. The `TIME_LIMIT` fleet-scale finding
above needs a decision on backlog placement. GB-36 and GB-31/GB-24 remain
open as before.
