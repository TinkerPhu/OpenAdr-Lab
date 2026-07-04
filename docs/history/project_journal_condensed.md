# OpenADR Lab — Project Journal (Condensed)

> Condensed from project_journal.md — see git history for full detail.

---

## Foundation (Phases 1–14)

Deployed a Pi4-hosted OpenADR 3 lab: VTN (openleadr-rs + PostgreSQL), three VEN instances, VEN UI, VTN BFF, and VTN UI — all communicating over Docker bridge network `openadr-net`.

### VTN API Shape Discoveries

- Token endpoint is `POST /auth/token`, **not** `/oauth/token`. Token TTL is 30 days.
- VTN field names follow OpenADR 3 spec verbatim: `programName`, `programID`, `eventName`, `createdDateTime`, `venName`.
- Role-based access is strict per endpoint: `any-business` → `/programs`, `/events` (403 on `/vens`); `ven-manager` → `/vens` only. The BFF requires **two separate VtnClient instances** to cover all endpoints.
- User credential creation requires the API (`POST /users/{id}` to add `client_id`/`client_secret`) because secrets are argon2-hashed server-side — raw SQL INSERT won't work.
- Complete API sequence to register a new VEN: `POST /users` → `POST /users/{id}` (credentials) → `POST /vens` → `PUT /users/{id}` (assign VEN role with VEN UUID).
- `POST /reports` requires VEN role — business credentials receive 403 and cannot create reports on behalf of VENs.
- VTN `GET /reports` with `?clientName=X` filters server-side; no VTN changes needed for per-VEN isolation.
- VTN `PUT /users/{id}` requires a full body (`reference`, `description`, `roles`) — partial update returns 400.

### DTO No-Normalization Decision

- Decision: pass VTN field names through all layers unchanged (`programName`, `programID`, `eventName`, `createdDateTime`). One vocabulary across backend, BFF, and UI eliminates translation boilerplate and debugging friction.
- Net effect: removing the normalization layer saved ~76 lines and unified two previously divergent vocabularies.

### Infrastructure Constraints

- Docker Compose project name defaults to the directory name. A service named `vtn-db` in a directory called `VTN/` produces container `vtn-vtn-db-1`. Name services without the directory prefix.
- Docker Compose `${VAR:-default}` values are silently overridden by `.env` files. Check `.env` on both local and Pi4 when changing defaults.
- All OpenADR Lab services use the `82xx` port range to avoid conflicts with existing Pi4 containers.
- Pi4 hostname is `pi4server.local` (via mDNS/Avahi), not `raspberrypi.local`.
- Vite builds fail when run from Windows `subst` drive aliases — Vite resolves the real path internally causing mismatches. Build from the real path `C:\DriveD\...` or in Docker.
- `npm ci` requires `package-lock.json` to be in sync with `package.json`. Always run `npm install` locally and commit the lock file before Docker builds.

### Integration Test Suite Design

- Test stack uses an isolated Docker Compose project (`openadr-test`) with its own network — no published ports, no shared volumes with production.
- VEN poll intervals set to 5s (vs 30/300s in production) for fast test feedback.
- `--abort-on-container-exit` kills all containers when ANY exits — never use one-shot containers alongside it. Moved fixture loading into the test-runner entrypoint instead.
- Behave `Background` runs before **each** scenario, not once per feature — use unique test data names.
- `poll_until()` with short intervals is the correct pattern for eventual consistency checks.
- Hardcoded timestamps in test steps expire over time — always use `datetime.now(timezone.utc) + timedelta(...)` for event start times.
- Program accumulation across runs causes 409 conflicts and VTN pagination gaps. Add `before_feature` cleanup that paginates DELETE across all programs.

### OpenADR Protocol Discoveries

- OpenADR 3 event cancellation = DELETE (no cancel status field).
- Programs without `targets` are visible to all VENs ("open"). Programs with `targets: [{type: "VEN_NAME", values: [...]}]` are enrollment-filtered server-side.
- `VEN_NAME` targets wire format is an array of objects: `[{type: "VEN_NAME", values: [...]}]` — not an object map.
- VTN returns **201** on `POST /reports` — the VEN backend must forward this status code.
- Events are permanent records — deletion fails when reports exist (FK `ON DELETE RESTRICT`). The correct pattern is editing the event to add timing rather than deleting it.
- `reportDescriptor.frequency` is an integer (seconds), not an ISO 8601 duration string — the VTN silently drops unknown fields.

### openleadr-rs Bug Fixes (Contributed Upstream)

- **Duplicate report rows**: `GET /reports` returned duplicate rows when a program had multiple VEN enrollments, because `LEFT JOIN ven_program` multiplied rows without `DISTINCT`. Fixed with `SELECT DISTINCT r.*`.
- **VEN report isolation**: VENs in the same program could see each other's reports. Fixed by adding `ven_id` column to `report` table (FK to `ven`), filtering `r.ven_id = ANY(user_ven_ids)` for VEN users.
- **Program VEN_NAME targets not reconstructed**: `retrieve` / `retrieve_all` never reconstructed `ven_program` rows into the `targets` field — operators couldn't read enrollment back via the API. Fixed with `enrich_ven_targets()` helper.
- **Event-level VEN_NAME filtering + stripping**: Events with `VEN_NAME` targets were visible to all enrolled VENs (not just targeted ones). Fixed with SQL `AND (NOT $is_ven OR ...)` clause. Added strip: targeted VENs receive responses with `VEN_NAME` entries removed (privacy layer 2).
- **SQLx offline cache**: Hash is SHA-256 of the exact raw query string between `r#"` and `"#`. Whitespace (including trailing spaces) matters. Compute hashes on Linux (LF line endings) — CI runs Linux even if dev is Windows.

---

## Simulator and Reactor (Phase 15–16)

Built a physics-based simulator (EV charger, heater, PV inverter, battery) with per-VEN YAML profiles defining device mix, capacity, and reaction strategy. Added a Reactor module with an FSM (Idle→Delaying→Ramping→Holding→RampingBack) for event-driven setpoint control.

### Reactor Deletion Decision (Phase 24b)

- The reactor was deleted in favour of making the MILP planner the sole authority for setpoints. The reactor FSM treated multi-interval events as one continuous activation, ignoring per-interval payload changes.
- `target_key()` was a partial fix but insufficient — the FSM's boolean `event_active` is semantically wrong for multi-interval events with varying payloads.
- The clean solution (Phase 24b): `build_setpoints()` derives setpoints directly from the active MILP plan each tick. No FSM, no ramp interpolation, no reactor state.

---

## HEMS Controller Architecture

### Controller Architecture Decisions

- **VEN/src/ follows Hexagonal + Clean Architecture** with strict dependency direction (outer rings never imported by inner rings):
  - Adapters: `routes/`, `tasks/`
  - Application: `services/`
  - Domain: `entities/`, `controller/`
  - Infra: `assets/`, `simulator/`, `vtn.rs`, `controller/milp_planner/`

- **SimulatorPort trait** (`controller/simulator_port.rs`): all controller modules accept `&SimSnapshot` (not `&SimState`). `SimState` is infra; `SimSnapshot` is the domain-side view. The snapshot-and-release pattern ensures the simulator lock is dropped before any expensive work.

- **VtnPort trait** (`controller/vtn_port.rs`): all tasks receive `Arc<dyn VtnPort>` instead of the concrete `VtnClient`. Enables testing with `MockVtn` without HTTP infrastructure. `async_trait` is required for dyn dispatch.

- **AssetMilpContext trait** (`milp_planner/asset_port.rs`): the planner accepts `Vec<Box<dyn AssetMilpContext>>` — never imports `Battery`/`Ev`/`Heater` directly.

- **Profile rule**: `profile.rs` (YAML config) is infra. Domain rings import from `entities::asset_params` (`BatteryParams`, `EvParams`, etc.) which are pure data structs with no serde. `main.rs` is the sole assembly point that converts `Profile` → domain params via `build_domain_params()`.

- **Three independent locks** (R-07): `AppState` uses `polling: Arc<RwLock<>>`, `ctrl_sim: Arc<RwLock<>>`, `hems: Arc<RwLock<>>`. INVARIANT: no function acquires more than one lock simultaneously.

- **`PersistedVenState` struct** maintains identical `state.json` format across the lock split — existing Pi4 state files load without migration.

### Architecture Invariant Greps (run before any VEN PR)

```
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes  → must be empty
grep -r "use crate::assets::" VEN/src/controller/milp_planner --include="*.rs" | grep -v "cfg(test)\|tests/"  → must be empty
grep "serde_json::Value" VEN/src/vtn.rs  → must be empty or internal only
grep -r "use crate::assets::" VEN/src/entities  → must be empty
grep -r "use crate::vtn::VtnClient" VEN/src/tasks  → must be empty
```

The `TimelineSnapshot` / `TimelineAssetData` pattern: `SimState::to_timeline_snapshot()` clones all history buffers and releases the sim lock before the route handler serialises JSON. This avoids holding the Mutex during expensive serialisation.

### MILP Planner Design

- HiGHS is the MILP solver. It requires cmake/HiGHS to be available at compile time — not available natively on Windows. Use `wsl cargo build` for local builds; Pi4 Docker for full runs.
- Two-phase solve: Phase 1 minimises cost; Phase 2 minimises relay switches while staying within `phase2_epsilon_eur` of the Phase 1 cost.
- `dt_h: Vec<f64>` (not scalar) enables variable-slot-width zones. Uniform plans use `vec![step_h; n]` — the interface is zone-ready without behavioural change.
- **3-tier zones** (current state on `refactor/3-tier-milp`): Zone A = 300 s × 96 slots (8 h), Zone B = 600 s × 96 (16 h), Zone C = 900 s × 96 (24 h) = 288 slots total. Central abstraction: `cum_s: Vec<i64>` where `cum_s[t]` = seconds from now to slot `t` start.
- `resample_uniform` aligns to epoch-based grid boundaries. Planner slots start at `now` (arbitrary seconds). **The HashMap lookup pattern always returns `None`** for tariff lookups. Use `interpolate_at(slot_start)` per slot instead.
- LOCF extends a single Step tariff sample forward indefinitely. Use two-interval event design (target price + reset-to-default interval) to prevent tariff contamination of all subsequent slots.
- MILP solve time: 5–10 s on an unloaded Pi4; 80–120 s under 3-VEN concurrent load (CPU contention). BDD timeouts must accommodate the worst-case loaded scenario.
- `f64::MAX` is used as a sentinel for "no capacity limit" — `isFinite(Number.MAX_VALUE)` returns `true` in JS. Use a threshold check (`< 1e15`) in TypeScript.

### Sim Inject API

- `POST /sim/inject` uses partial-merge semantics: absent = no change, `null` = release, value = activate. Three injection behaviours:
  - **A (Jump + free evolution)**: apply once, physics drives from there (`battery_soc`, `ev_soc`, `heater_temp_c`)
  - **B (Frozen + EMA blend-back)**: hold while active, exponential return on release (`pv_irradiance`)
  - **C (Frozen + snap)**: hold while active, snap to profile default on release (`ev_plugged`, etc.)
- `POST /sim/override` was removed. All BDD steps must use `/sim/inject`. Silent 404s in `after_scenario` hooks that call removed endpoints corrupt shared state for all subsequent features — never use `except Exception: pass` swallowing in cleanup hooks.
- Behaviour C snap-back must be **active** (`s.plugged = ev_plugged_override.unwrap_or(true)`) — "do nothing when override is None" leaks state into the next scenario.
- `pv_plan_kw` inject pins every MILP horizon slot to a fixed kW, making plans deterministic regardless of time of day. This is orthogonal to `pv_irradiance` (physics tick only). Both are needed for deterministic BDD tests.

### Deviation Absorber

- Two-tier grid deviation control: Tier 1 (real-time absorber) applies transient setpoint corrections across battery/EV/heater; Tier 2 (sustained) fires a full MILP replan only when the absorber is exhausted for `deviation_trigger_ticks` consecutive ticks.
- Tier 2 accumulates `residual_kw` (what absorber couldn't cover), not raw `deviation_kw`. Prevents phantom replanning for deviations the absorber handles.
- EV departure guard: absorber skips EV curtailment when departure is imminent and SoC < target. Does NOT block increasing EV charge (absorbing surplus PV).
- `deviation_trigger_ticks` belongs in `AbsorberParams` (not `PlannerConfig`) because it controls absorber escalation, not planning frequency.

### TimeSeries Resampling (`common/mod.rs`)

- `interpolate_at(ts)`: Step uses LOCF; Linear uses proportional interpolation with no extrapolation past last sample.
- `time_weighted_mean(start, end)`: piecewise integration — constant segments for Step, trapezoids for Linear. Returns `None` for Linear past data end.
- `resample_uniform(width)`: epoch-aligned grid using `rem_euclid` — ensures cross-asset timestamps align when using the same `width`.
- Single-sample Step series only produces ONE resampled bucket (no LOCF propagation in `resample_uniform`). Use `interpolate_at` per slot for tariff lookup in the planner.

### Timeline API

- Server-side `max_points` downsampling is essential for constrained hardware. 3600 rows/asset × 5 assets = browser freeze on Pi4 ARM. Default 120 points.
- All assets share identical `ts` values at each index (uniform grid) — enables positional indexing instead of tolerance-based nearest-neighbour matching.
- Grid snapped to round boundaries of `resolution_s` — deterministic regardless of call time.
- Now-point is NOT grid-aligned; it sits between history and future at exact server `now`. Required because the UI cannot interpolate without knowing the interpolation method.
- `values: null` for empty future grid buckets (not omitted) — preserves array alignment.
- recharts `ReferenceLine` is silently dropped when its `x` value falls outside the XAxis domain. Always compute an explicit domain that includes the reference line value.
- `ResponsiveContainer`'s async `ResizeObserver` creates timing that MUI `Collapse` animation tests may depend on. Never replace with a fixed-width chart without checking test timing assumptions.

---

## Playwright / BDD Test Infrastructure

- MUI Slider `slotProps.input` does not reliably forward `data-testid` to the native `<input>` in Chromium (works in JSDOM). Wrap Slider in `<Box data-testid=...>` and scope selectors to `[data-testid="..."] input[type="range"]`.
- MUI Switch click must target `input[type="checkbox"]` inside the root — clicking the root `<span>` does not reliably trigger `onChange`.
- MUI `Collapse` renders children even when `in={false}`. Add `unmountOnExit` when tests check `queryByTestId(...).toBeNull()`.
- `vi.useFakeTimers()` breaks `userEvent` click tests — MUI animation callbacks stall. Use `vi.spyOn(Date, 'now')` per-test instead.
- `requests.Response` is falsy for 4xx/5xx (`response.ok == False`). Never use Python `or` to chain response fallbacks — use explicit `is None` checks.
- Playwright `wait_for_selector` timeout with "locator resolved to visible" in call log = JS thread blocked (CPU overload), not a missing element.
- Behave `{param}` captures are greedy — `{hours_back}` matches `0&hours_forward=1`. Avoid step patterns that partially overlap with generic steps.
- `docker compose run --build` only rebuilds the named service image, NOT `depends_on` images. Explicitly `docker compose build <service>` after source changes to dependent services.
- Docker named volumes survive Pi4 power cycles — cargo compiled artifacts are not lost.
- Bash `exit code 1` from nohup over SSH ≠ process failed — nohup writes to stderr. Always `docker ps` to verify before concluding a background launch failed.

---

## MILP Storage Planning Decisions (Steps 1–6)

Addressed heater under-utilisation (40°C instead of full 40–80°C band) and excessive relay switching for ven-2 (2000 L hot water tank + 12 kW PV).

- **Terminal energy reward** (`c_terminal_eur_kwh`): auto-computed as `mean(import_tariff) + ctrl_malus` for heater, `mean(import_tariff) × efficiency` for battery. The objective term `−c_terminal × e_tank[n−1]` makes the optimizer treat stored heat at horizon end as economically valuable, preventing premature stop at T_min.
- **48 h horizon** for ven-2 with 10-min slots (288 total): makes both solar windows visible, eliminating plan fragmentation when the horizon starts just after a solar peak.
- **Block commitment anchor** (Step 5): pins heater binary tier variables to the last adopted plan's values within an anchor window (`anchor_until`). Prevents relay chattering between consecutive planning cycles. Hard triggers (non-Periodic) clear the anchor so user-initiated replans are always fully free.
- **Gate switch-count guard** (Step 6): periodic replans that introduce more heater relay switches than the current plan must exceed the improvement threshold by a surcharge (`extra_switches × gate_switch_penalty_eur`) before adoption. `count_heater_switches` returns `f64` weighted by `slot_step_s / zone_a_step_s` so Zone-B and Zone-C switches cost proportionally more.
- **`phase2_epsilon_eur` must be ≥ 2× `switching_penalty_eur`** for Phase 2 to actually eliminate switches within the economic budget.

---

## Planner Audit Trail (Phase D: PlanReason)

- Replaced multi-phase greedy allocator with a unified per-step `rules_choose()` loop. Each asset at each timeslot gets a `(setpoint_kw, PlanReason)` pair, recorded in `PlanStep` and exposed via `GET /plan`.
- `PlanReason` kinds: `IDLE`, `FIRM_OBLIGATION`, `CHEAP_TARIFF`, `EXPENSIVE_TARIFF`, `CURTAILMENT`, `POLICY_CAP`.
- `GET /plan?summary` returns the plan with `steps: []` to avoid transmitting the full audit trail in summary views.
- The `resample_uniform + HashMap` tariff lookup was always silently broken — grid-aligned keys never matched `now`-anchored slot timestamps. Fixed by using `interpolate_at(slot_start)` per slot.
- Two-interval event design is required for LOCF-based tariff steps: one interval at the target price + one "reset" interval at the default price, so LOCF drops back after the event window rather than contaminating all future slots.
- Always add targeted polling steps (`wait for a "CHEAP_TARIFF" PlanStep`) rather than generic "has steps" polls — generic polls return immediately with the stale pre-event plan.

---

## Open / Partially Resolved

- **`milp_planner/` file sizes**: `absorber.rs` (1371 lines), `ev.rs` (945), `heater.rs` (1339), `battery.rs` (753), `pv.rs` (670) exceed the 500-line constitution limit. Pre-existing; no split planned in current branch.
- **SC-004 deferred items**: `controller/reporter.rs` and `controller/user_request.rs` still have infra imports in limited scopes (history ring buffer access, typed `AssetState` dispatch). Tracked as accepted deferred items.
- **`basic_create_read` flaky test** in openleadr-rs: client integration test races against other tests on the shared VTN server. Pre-existing; passes in isolation.
- **`@upstream_pending` scenarios**: 2 VEN isolation report tests tagged to skip in CI because they depend on upstream PRs not yet merged.
- **Reporter resampling (RF-05e)**: Multi-interval obligation reporting works; single-point fallback also works. Five complications remain for further improvement (obligation interval plumbing, import/export split, EV SoC point-in-time) — tracked in BACKLOG.
- **Polling tasks still use concrete `VtnClient`** in some routes (`AppCtx.vtn` used by HTTP handlers). The `VtnPort` trait is wired for all tasks (`spawn_*`), but the routes layer still holds a concrete client — a future cleanup to complete Invariant 4 across all of `VEN/src/`.
- **`deviation_absorber.feature:149`** (`DeviceDeviation does not fire for transient deviations`): marked `@wip`. Root causes: (1) T1+T2 trigger race where a second planning trigger queues while the first solve runs, and (2) time-of-day headroom — at solar-prep hours the MILP pre-discharges the battery, leaving insufficient headroom for the absorber assertion. Fixed for BDD by `pv_plan_kw` inject; the wip tag should be removed once confirmed green on Pi4.
