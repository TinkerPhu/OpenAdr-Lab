# Technical Debts Register

> **Next ID: R-101.** Use this number for the next new item filed, then increment this
> line to R-102. (R-98 came from `048-ev-soc-state-variables` and R-99 from
> `fix/fleet-chart-precondition`, both merged; R-100 is issued on this branch for the
> disabled VTN session path. The line counts every ID ever issued, so on a merge keep
> the higher number rather than the one either branch alone would suggest.) (Corrected
> 2026-09-27: the line still said R-86 while R-87..R-97 were
> already issued — exactly the drift the paragraph below warns about.) When resolving and removing an item — even the current highest ID —
> do NOT decrement this line: it tracks every ID ever issued, not the count of rows
> currently in the table, so a removed row never frees its number for reuse. This is
> the single source of truth for the next ID; do not derive it by scanning for the
> highest `R-NN` currently present, since a removed top item would make that scan
> under-count and hand out a number that was already used once (see R-66/R-68 below,
> caused by exactly that kind of drift before this line existed).
>
> Verified against code 2026-07-16. Detailed diagnostics for large refactors:
> `docs/plans/refactoring_backlog.md`.
>
> **Rule:** Before adding a feature in an affected area, check this file first.
> Refactor the relevant debt before adding new behaviour if effort is Small or Trivial.
> Enforced by `python scripts/audit_debt_gate.py` (tests: `scripts/test_audit_debt_gate.py`):
> a branch that touches a Small/Trivial row's `Affected files` must delete the row or carry a
> `Debt-deferred: R-NN: <reason>` commit line. Keep `Affected files` as backticked paths.
>
> IDs are stable and never reused; gaps in the numbering are resolved items
> (resolutions live in `docs/history/project_journal.md` and git history).
>
> Gain is the value of fixing the item — independent of Effort/Risk — rated
> High/Medium-High/Medium/Low-Medium/Low/None. Unlike `docs/BACKLOG.md`'s Gain
> (new capability delivered), most debt items don't change behavior at all, so
> Gain here usually reflects reduced risk, friction, or maintenance cost instead.

Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (deferred)

---

## Cross-register priority view (2026-09-27, R-99 row added 2026-10-03)

One ordered list across **both** registers — this file (`R-*`) and `docs/BACKLOG.md` (`GB-*`) —
because the two rate things differently: debts carry a priority emoji, while the backlog's last
column holds effort, not severity. Ratings for `R-*` items that already had one are reproduced
unchanged; `GB-*` severities are assigned here.

**This is an index, not a second source of truth.** Each row points at the item; the item owns
its detail. Re-rate in the item, then here.

| Severity | ID | Goal it blocks | One line |
|---|---|---|---|
| 🟠 Med-High | GB-50 | VTN stimulus | **Inbound half only** (outbound closed 2026-09-20, see `docs/reference/WIRE_PROFILE.md`): the VEN reads incoming payloads by type name and assumes the unit; declared `payloadDescriptors` are parsed and ignored. In progress: `openspec/changes/gb-50-inbound-payload-descriptors`. |
| 🟠 Med-High | R-97 | reliable system | Solves of 20.8 s and 63.6 s against a 60 s per-phase timeout — the mechanism behind GB-38 is one busy host away from recurring. |
| 🟡 Medium | R-86 | VTN stimulus | `randomizeStart` parsed but ignored — the whole fleet responds on the same instant, which is what the field exists to prevent. |
| 🟡 Medium | R-21 | reliable system | `cargo test` heap corruption around the HiGHS tests: undermines the instrument every other conclusion rests on. |
| 🟡 Medium | R-87 | VTN stimulus | Stricter than the 3.1 schema on `intervalPeriod.start` — the one failure a conformance lab must not have. |
| 🟡 Medium | R-100 | VTN stimulus | A VTN SoC command was modelled as the user's own charging intent; disabled 2026-10-03 pending a decision on what it should be. |
| 🟡 Medium | R-98 | reliable system | The EV variable set is declared twice — the real solve and phase-2's dual extraction — and R-93 widened what the second must mirror. |
| 🟡 Medium | R-99 | reliable system | **Test half resolved 2026-10-03**; what is left is a product question. The Fleet page opens on a 24 h / 900 s window and shows "No telemetry stored for the last 1440 minutes" on a freshly started VTN — which is false, there is just less than one bucket of it. It cannot tell "the store is still filling" from "nothing is reporting". |
| 🟡 Medium | R-94 | transparent UI | The MILP does not model the heater deadband, so the asset can refuse planned dispatch with nothing on screen saying why. |
| 🟡 Medium | GB-46 | VTN stimulus | Missing VEN-side instrumentation (tariff source event id, effective-limit column), so compliance cannot be proven from the data without harness reconstruction. |
| 🟡 Medium | GB-52 | reliable system | The E2E broker runs anonymous while production requires credentials — the test bed does not exercise the auth path. |
| 🔵 Low | R-74 | transparent UI | The Dashboard Simulation card hardcodes ev/heater/pv; battery, base load and shiftable loads are invisible there. Small, and the generic pattern already exists in `Controller.tsx`. |
| 🔵 Low | R-77, R-90, R-89, R-33, R-71, R-36, R-38, R-44, R-48, R-66, R-91, R-95 | — | Hygiene, naming, dead code, coverage gaps and future-proofing. Do opportunistically, per this file's own "refactor first if Trivial/Small" rule. |
| 🔵 Low | GB-43, GB-39, GB-12 | — | Aspirational or cosmetic (V2G modelling, dark mode, docs alignment). |

**Note on R-74:** rated Low because the same data is visible on Controller/Devices — but it is
the cheapest item in the table and the only one in the "transparent UI" goal that is purely
additive, so it is the natural filler task between larger pieces.


---

## Priority queue (🟠 / 🟡) — work these first, top down

| ID | Description | Affected files | Effort | Risk | Priority | Gain |
|----|-------------|----------------|--------|------|----------|------|
| R-90 | **Solar position is implemented twice, in two languages, and the lab's coordinates live in two places.** `ui-charts/src/solarPosition.ts` is a faithful port of the elevation path of `VEN/src/entities/solar.rs`, added so the VTN Fleet charts can shade day and night with the real sun rather than a fixed clock curve (at 47.45°N sunset swings from ~16:40 in December to ~21:30 in June, so a clock curve is hours wrong for half the year). The Rust `SolarPosition` is not serialized and no endpoint exposes elevation, so there is no channel that could carry the answer to a browser. The same reasoning forced `VTN/ui/src/utils/labLocation.ts`, a second copy of `weather_pv.latitude_deg`/`.longitude_deg` from the 14 VEN profiles. The two solar implementations can drift; the TS tests pin it to closed-form geometry (solstice elevation = 90 − lat ± 23.44°) rather than to the Rust, so a drift in *either* is caught. Resolution if precision ever matters: serve elevation and the site location from the BFF and delete the port — not make the port smarter. | `ui-charts/src/solarPosition.ts`, `VEN/src/entities/solar.rs`, `VTN/ui/src/utils/labLocation.ts`, `VEN/profiles/*.yaml` | Small | Low (a background tint; a degree of drift is invisible) | 🟢 | Low — cosmetic today, but it is a real second implementation of a physical concept and should not quietly grow more callers |
| R-89 | **A BDD step asserts a sign that the payload does not guarantee.** `ven_reporting_out.feature:32` requires the `BASELINE` payload to be non-negative, but `report_intervals.rs::build_baseline_report_intervals` sums `AssetHeuristics::sample_kw` across every asset -- PV included -- so a site whose learned generation exceeds its learned load produces a negative baseline, exactly as `USAGE` legitimately does. The same assertion on `USAGE` did fail this way (2026-09-23, -6.8e-06 kWh) and was corrected to a sign-agnostic one; `BASELINE` has not fired yet only because the heuristics in the test profile have not learned enough PV to cross zero. `IMPORT_RESERVATION_CAPACITY` on line 15 is fine -- a capacity really is non-negative. Not changed on speculation: a passing test should be corrected with evidence, not a hypothesis. | `tests/features/ven_reporting_out.feature:32`, `VEN/src/controller/report_intervals.rs` | Trivial | Low (one scenario, and only when PV heuristics dominate) | 🟢 | Low -- a flake that would look like a reporting bug, which is the expensive part |
| R-87 | **This stack is stricter than the 3.1 schema about `intervalPeriod.start`.** The schema gives `intervalPeriod` no `required:` list at all (`1_OpenADR_3.1.0_20250801.yaml`, ~line 2154), so a conformant peer may omit `start`; `openleadr-wire` makes it a non-`Option` `DateTime<Utc>` and carries its own `// FIXME field not required, though, it's unclear how to interpret it if it's missing` (`interval.rs:43`). Since 3.1b the VEN parses events with those types, so it refuses such an event -- which `wire-contracts` explicitly forbids ("never reject a peer for omitting what the spec lets it omit"). Not newly introduced by the VEN: our **VTN** already parses incoming events with the same types, so it rejects them at the API boundary first, and no such event can reach a VEN in this lab. Fixing it properly means making the field `Option` upstream (their FIXME) and teaching `EventRequest::ends_at()` -- our own P-1 patch -- to cope with an unanchored period; that is an upstream PR, not a local workaround. Until then the deviation is on the VTN's side of the wire and is documented rather than hidden. | `openleadr-rs/openleadr-wire/src/interval.rs`, `lab-core/src/event_timing.rs` | Medium | Low (no peer in this lab omits it; shadow parse clean across 20 VENs) | 🟡 | Medium — conformance-lab credibility: being stricter than the spec is the one failure a conformance lab must not have |
| R-86 | `intervalPeriod.randomizeStart` is parsed and carried (branch 045, 2026-09-20) but not honoured: a VTN asking a fleet to stagger its response still gets every VEN starting on the same instant, which is the one outcome that field exists to prevent. Honouring it means offsetting the VEN's own action within the declared window from a per-VEN deterministic seed (the `determinism` rule forbids an un-injectable clock or RNG here), and deciding whether the offset applies to dispatch only or to reporting too. Found during the 3.1 DTO gap audit; carried rather than half-built (`no-half-built-features`). | `VEN/src/controller/vtn_port.rs` (`OadrIntervalPeriod`), `lab-core/src/event_timing.rs`, `VEN/src/controller/dispatcher.rs` | Medium | Medium (changes when a VEN acts, fleet-visible) | 🟡 | Medium — the spec's own mechanism for avoiding a synchronised fleet response |
| ~~R-76~~ | **Resolved 2026-10-04.** Confirmed, then fixed. Three defects, not the one the entry suspected. (a) **The directions were crossed** — the `IMPORT_` payload read `up_kw` (the *Export*-commitment power) and the `EXPORT_` payload read `down_kw`. Reproduced first, by `reporter::tests::import_reservation_capacity_reads_the_import_direction`, which failed with 8 kW where 3 kW was due. (b) **The quantity was wrong**, as this entry suspected: the spec defines these as capacity *requested* beyond what the VEN is contracted for (Table 2, with a companion `*_RESERVATION_FEE` for what the VEN will pay), not live headroom. (c) **`.abs()` masked a legitimate sign flip** — `up_kw` goes positive when a site is net-importing even under a sustained Export commitment, and its magnitude was then reported as export capacity. The fix puts the quantity in `entities::reservation_request::ReservationRequest::from_headroom` (max-effort capability per direction, less the contracted allowance, floored at zero) and leaves `reporter.rs` a mapper reading one named field per payload type. The inbound half already existed — `IMPORT_CAPACITY_RESERVATION`/`*_SUBSCRIPTION` events bind the planner's slot caps (WP3.3 §8.10) — so the loop now closes, and the allowance rule itself moved onto `OadrCapacityState` rather than being copied (it was a closure inside `build_milp_inputs`). Normal value in this lab is **0**, honestly: no program here issues subscription or reservation events, so nothing binds and there is nothing to ask for. Surfaced on the grid-signal strip (`ui-transparency`) and pinned by `tests/features/ven_reporting_out.feature`'s `@r-76` scenario, which grants a 2 kW allowance to make the request non-zero. **Deliberate non-goal:** "what the site wants" in the strict sense needs a counterfactual solve with the allowance lifted; capability-less-allowance is an upper bound that never understates the binding constraint. | `VEN/src/entities/reservation_request.rs`, `VEN/src/controller/reporter.rs`, `VEN/src/entities/capacity.rs` | Small | — | ✅ | Protocol correctness: a live VTN report carried a crossed, wrong-quantity value for ~4 weeks with no test able to see it |
| R-77 | Project-wide "envelope" naming audit, deferred from the 2026-09-07 site-headroom fix (`naming-envelope-vs-headroom` in `.claude/CLAUDE.md`): a grep found ~48 `VEN/src` files referencing "envelope"/"Envelope". This session renamed only the two files it was already touching (`envelope.rs`→`site_headroom.rs`, `capacity_envelope.rs`→`capacity_headroom.rs`). Remaining candidates needing individual triage (not a blanket rename): `entities::plan::FlexibilityEnvelope` (per-device-session envelope, built by `milp_planner/envelopes.rs::build_plan_envelopes`) and `SiteFlexibilityEnvelope`/`SiteFlexibilitySample` (this fix's own structs, kept as-is since renaming a `Serialize`d struct mirrored by a TypeScript type in `VEN/ui/src/api/types.ts` is a larger, separately-reviewable change) are likely internal-HEMS concepts that should rename to "headroom"; `entities/capacity.rs`'s "Dynamic Operating Envelope" is a genuine OpenADR-boundary use that should keep "envelope" (`reporter.rs`'s reservation-capacity handling was listed here too; R-76 removed it — those arms no longer read an envelope at all, they read `entities::reservation_request`); `milp_planner/envelopes.rs`'s own mathematical scheduling-constraint sense of "envelope" is ambiguous and needs its own judgment call. | `VEN/src` (~48 files, see grep for `envelope\|Envelope`) | Medium (many small renames, no logic changes) | Low (naming-only, mechanical once triaged) | 🟡 | Low-Medium — code-comprehension/consistency gain, not a correctness fix |
| R-21 | `cargo test` intermittently crashes with heap corruption (SIGABRT, varying malloc messages) around the two heaviest HiGHS tests (`run_planner_n48_full_horizon`, `solve_ven3_heater_three_tier_zones_feasible`). Same tests pass clean in isolation every time; also crashes with `--test-threads=1`, so it is allocator/heap-state-dependent in the native HiGHS library, not a plain data race. Test-infra only — no production path. Workaround: run the affected module in isolation when the full suite crashes. | `VEN/src/controller/milp_planner/` (HiGHS FFI via `good_lp`), test harness only | Medium | Low (flake) | 🟡 | Medium — CI/test-suite trust, no production impact |
| R-33 | UI test gaps: `VTN/ui/src/pages/Metrics.tsx` is the only untested page in either UI; `JsonDialog.tsx` is byte-identical in both UIs (50 lines — accept the copy with a twin-note header, or fold into a shared package if one materializes). | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | 🟡 | Low — test-coverage gap |

## Low priority (🔵) — by topic

### Architecture & type placement

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|

### Code & repo hygiene

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-27 | Hard-coded tuning constants: task intervals (`state_persist.rs:8` 15 s, `progress_ticker.rs:15` 1 s). Name them and/or expose via config/PlannerParams. **The MILP-tolerance half is discharged (2026-08-28)**: `with_mip_gap` at all three solve sites now reads the per-profile `planner.mip_gap_target` (default 0.02) carried on `MilpInputs`, and the `MIP_GAP_TARGET` constant is gone. It was briefly configurable in August and reverted (5b8923c3) for line-count reasons rather than on its merits; restored here once `PlannerConfig` moved to its own module and the quality cost was actually measured (GB-40). | tasks/ | Trivial | Low | Low — config flexibility only |
| R-36 | Lint/doc hygiene bundle: (a) module-wide `#![allow(dead_code)]` without justification in `entities/capacity.rs:5`, `entities/design_vocabulary.rs:7`; (b) ~~12 eslint warnings (exhaustive-deps, mixed exports)~~ — fixed 2026-10-03: `VEN/ui` reports 0 errors and 0 warnings. The four `exhaustive-deps` ones were real (a bare `rates ?? []` re-ran three `useMemo`s every render); the `only-export-components` ones each marked genuinely misplaced code, including a `dayRangeIso` duplicated across two pages whose sibling helper had drifted into reading the browser's clock. VTN/ui still has 2 (both react-refresh/only-export-components, 0 errors) -- checked 2026-10-03, not fixed; (c) ~~eslint lints the generated `VTN/ui/coverage/` dir~~ — fixed 2026-08-05: turned out worse than lint noise, the whole generated dir (27 files, 423K) was actually committed to git because `VTN/ui/.gitignore` was missing the `coverage/` line `VEN/ui/.gitignore` already had; untracked and added; (d) `solve_ven3_heater_three_tier_zones_feasible` runs >60 s in debug `cargo test` — consider a smaller horizon variant; (e) "Stage 5 —" phase labels in `entities/user_request.rs` / `controller/user_request.rs` doc comments — drop the prefixes. | entities/, VEN/ui, VTN/ui, milp_planner/tests | Small | Low | Low — mostly cosmetic; (d) has a small developer-friction upside (faster test runs) |
| R-71 | **Re-scoped 2026-10-03 after a measured pass: 50 unjustified `#[allow(...)]` sites down to 42, and the whole `unused_imports` class is gone.** The original entry listed 9 sites in the assets/simulator area; a repo-wide count found 50, of which 13 were `unused_imports`. Those were not a hygiene problem: `milp_planner/mod.rs`'s ten were suppressing a correct dead-code warning (most of the names were used by nothing — see the 2026-10-03 journal entry), and the rest became `#[cfg(test)]` re-exports, which makes the claim checkable instead of asserted. Three `dead_code` allows covered genuinely dead items, now deleted (`asset_max_power`, `HvacService`) or wired up (`UserRequestService::create_shiftable`). **What remains: 42 sites, 26 of them `clippy::too_many_arguments`** — i.e. mostly a parameter-list problem, not a comment problem. `Simulator::tick` (25 params) and `spawn_sim_tick` (21) are discharged via `simulator::TickInputs` and `boot::World`; the biggest left are `MilpParticipant::build_milp_context` (18, trait-mandated and justified on its line in most impls) and `build_solve_request` (29, which is also R-40's `cycle.rs` entry). Fix the signature where there is a real parameter object to name, and add the same-line justification only where the breadth is genuinely trait-mandated. | `VEN/src` (run `grep -rn "#\[allow(" VEN/src --include="*.rs" | grep -v tests/ | grep -v "// "`) | Small | Low | Low — the dead-code half is done; the rest is signature work |
| R-38 | (a) `VEN/Cargo.toml` carries blueprint-era comments (commented-out `openleadr-client` etc.); (b) verify `VTN/data/db` (runtime artifact) is gitignored. | `VEN/Cargo.toml`, `VTN/data/` | Trivial | Low | None — pure hygiene |
| R-44 | `/health` handler (`routes/system.rs::health`) deep-clones the full `VtnConnectionStatus` and active `Plan` on every poll just to read a couple of fields. Cheap today but grows with `Plan` size; consider a narrower state accessor. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/system.rs` | Trivial | Low | Low — cheap today, future-proofing only |
| R-74 | Found while adding `shiftable_load` as a new asset type (`shiftable-load-as-asset`): `VEN/ui/src/pages/Dashboard.tsx`'s "Simulation" card dispatches per asset by hardcoded presence checks (`"ev" in sim.data.assets`, `"heater" in ...`, `"pv" in ...`) with no case — and no generic fallback — for `battery`, `base_load`, or the new `shiftable_load`. Pre-existing gap (Battery/BaseLoad were already invisible there before this change), not introduced by it; not fixed as part of that change since it's a UI-layer refactor unrelated to the backend asset-dispatch work. Contrast with `Controller.tsx`/`AssetSpecsTable.tsx`'s `deriveAssetSummaries`, which already has a generic `HARDCODED_IDS`-exclusion fallback loop covering any asset_id it doesn't special-case. Fix: give `Dashboard.tsx`'s Simulation card the same generic fallback (or iterate `sim.data.assets` by `asset_type` rather than a fixed id list). | `VEN/ui/src/pages/Dashboard.tsx` | Small | Low | Low — one dashboard card under-displays some asset kinds; the same data is already visible via Controller/Devices | 
| R-66 | `run_all_tests.sh`'s GB-24 pre-flight capacity check (`MIN_AVAILABLE_MEM_MB=800`) is a first-pass heuristic from one live `ssh Node2 "free -m"` observation (2026-08-14: 3794 MB total, 2482–2919 MB available with the resident fleet running), not empirically calibrated against an actual degraded run's memory profile. Same class as R-27 (hard-coded tuning constants). May need tuning if it proves too strict (blocks a run that would've been fine) or too loose (still lets a degraded run through). | `run_all_tests.sh` | Trivial | Low | Low — config-flexibility/accuracy concern only, not a functional defect |

### UI performance

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-48 | `useAssetCapabilities`/`useAssetForecasts` (WP-T6) fire one HTTP request per asset in parallel rather than a single batched endpoint; fine at lab scale (few assets) but won't scale. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/api/hooks.ts` | Small | Low | Low — fine at current (lab) scale, future-proofing only |
| R-49 | `Reports.tsx::latestSubmissionFor` recomputes its scan over all submissions on every render (not memoized) — fine at current volumes, revisit if submission history grows large. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/pages/Reports.tsx` | Trivial | Low | Low — fine at current volumes, future-proofing only |

### Weather forecast plugin (docs/architecture/weather_forecast.md)

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-53 | Horizon/shading obstructions, the Perez/HDKR diffuse-sky model (vs. the current isotropic-on-zenith simplification), and module degradation over time are known, deliberately deferred accuracy gaps in `entities::solar`'s clear-sky transposition — see `docs/architecture/weather_forecast.md`. | `VEN/src/entities/solar.rs` | Medium | Low | Low-Medium — PV forecast accuracy improvement, deliberately deferred until it's the dominant error source |
| R-54 | The Mosquitto broker in this project's existing deployment (Node1) allows anonymous connections on its plaintext 1883 listener — anyone on the local network can publish to the weather topics today. Acceptable for a lab on a trusted LAN; revisit (password file already exists at `/srv/docker/mosquitto/config/pwfile`, holding one unrelated user) before any exposure beyond the local network. **Halved 2026-09-19:** everything the lab *generates* moved to its own broker, `lab-mqtt` (Node1:1884, `allow_anonymous false`, per-client credentials) — 3.1's subscription notifiers and, later, fleet telemetry. What remains here is the house broker carrying the hardware-derived inbound feeds (`openadr-lab/measurement/*`, `openadr-lab/weather/*`), which stay anonymous because tightening them means changing config the home automation depends on. That is now a decision about the house, not a constraint the lab is stuck behind. | Node1 `mosquitto` deployment | Small | Low | Low today (LAN-only lab) — would become High if this deployment is ever network-exposed |
| R-55 | Snow-cover model's initial state (`PvSnowState` at the start of a forecast trajectory) only has the forecast-only fallback implemented — no cross-check against live PV telemetry deviation (`AssetState.power_deviation_kw`) to detect "actually covered right now" the way `docs/architecture/weather_forecast.md` describes as the preferred source. | `VEN/src/entities/pv_snow.rs` | Small | Low | Low — accuracy improvement for a specific, infrequent edge case |

### Cross-crate duplication

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-32 | `VTN/bff/src/vtn_client.rs` duplicates `VEN/src/vtn.rs`'s OAuth token + 401-retry + get/put-JSON plumbing (~300 lines each). Separate crates — extraction needs a shared workspace crate; record only, don't force. | `VTN/bff/src/vtn_client.rs`, `VEN/src/vtn.rs` | Medium | Low | Low-Medium — real duplication (bug fixes must be applied twice), but deliberately not forced until a shared crate is worth the indirection |

### Tooling & test infrastructure

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-35 | No script regenerates the module dependency graph — the SESSION_START.md quarterly check is manual. Add `scripts/gen_module_graph.py` emitting Mermaid from `use crate::` imports (test code excluded). | `scripts/` | Small | Low | Low — removes manual toil from a quarterly check |
| R-61 | `timeline_grid.feature :: Each asset array contains a now-point between history and future` is timing-dependent — observed to fail intermittently ("now-point at index 120 is not between history and future (array length 121)") on a Node1 E2E run where it had passed cleanly on an earlier run the same day, no code changes to the timeline/grid path in between. Likely an off-by-one at the exact boundary when "now" lands on the last grid slot. | `tests/features/timeline_grid.feature`, `tests/features/steps/timeline_grid_steps.py` | Small | Low (flake) | Medium — an E2E flake that could intermittently fail unrelated PRs' CI runs |
| R-62 | `pv_irradiance_one_shot.test.ts` (opt-in live-VEN integration test, skipped when unreachable) assumes "natural irradiance" is roughly static across its ~10 s window, but Node1's simulator is fed by live real-time weather data — cloud cover/sun-angle can shift the natural value mid-test faster than the injected offset's decay, so the after-decay assertion intermittently fails even after fixing two real bugs found alongside it (fixed 2026-07-31: hostname `Node1` isn't a real DNS/hosts entry so `getaddrinfo` flaked on Windows — switched to the LAN IP; injected offset was a fixed `+0.6` that violates the server's `[0,1]` clamp near solar noon — made it direction-aware based on headroom). Needs either mocking the simulator's weather input for this test or dropping the trend assertions in favor of only the one-shot-consumed check. | `VEN/ui/src/__tests__/pv_irradiance_one_shot.test.ts` | Medium | Low (flake, opt-in test only) | Low-Medium — an intermittently-flaky opt-in integration test, not a CI gate |
| R-65 | Narrowed by GB-31 (2026-08-19): `Plan.solve_status` now reads `good_lp`'s real `Solution::status()` (`Optimal`/`TimeLimit`/`GapLimit`, new `SolveStatus` variants) instead of being hardcoded, so an operator can at least see when a plan wasn't certified optimal. What's still missing: the achieved gap as a *number* — `good_lp`'s public `Solution` trait exposes only that coarse status, not the underlying `highs::SolvedModel::mip_gap()` float; reaching it means bypassing `good_lp`'s solve path (which drops the `SolvedModel` after extracting the solution) and reimplementing its private `Variable`→column-index mapping by hand — confirmed by reading `good_lp` 1.15.2's and `highs` 2.4.0's source, not assumed. `Plan.mip_gap_target` therefore still persists only the *configured* tolerance, not the achieved value — and since GB-40 made that tolerance a per-profile setting (`planner.mip_gap_target`, default `0.02`) rather than a const, the missing achieved-gap number is now the *only* way to tell what a given gap setting actually bought, which has to be measured offline instead (`milp_planner/tests/solve_cost.rs::bench_mip_gap_sweep`). | `VEN/src/controller/milp_planner/types.rs`, `VEN/src/controller/milp_planner/solver_phase1.rs` | Medium — real fix needs bypassing `good_lp`'s ergonomic layer for the solve step | Low | Low — diagnostic-quality gap, not a functional defect; `SolveStatus` already covers the more actionable half |

### Watch-list (not violations)

| ID | Description | Gain |
|----|-------------|------|
| R-40 | **File-size near-cap watch (production lines, re-measured 2026-10-03).** `scripts/audit_file_sizes.py` is the authority; run it rather than trusting this row. At the cap edge today: `assets/ev.rs` 480/500, `assets/shiftable_load.rs` 471/500, `services/planning/mod.rs` 471/500, `milp_planner/solver_phase2.rs` 471/500, `controller/arbiter.rs` 468/500, `milp_planner/results.rs` 466/500, `assets/pv.rs` 461/500, `state/mod.rs` 459/500, `entities/plan.rs` 453/500, and in `tasks/` (cap 200) `planning/cycle.rs` 195, `sim_tick/tick.rs` 190, `history_sampler/mod.rs` 182. **The lesson of the 2026-10-03 sweep belongs in this row:** a cap is met the right way by moving out what the ring does not own, and the wrong way by slicing orchestration thinner. Four `tasks/` files had reached 190-197 lines through splits their own module docs attributed to the cap (`helpers`, `context`, `post_lock`, `arbiter_glue` — names for *when* code runs, not what it does); relocating the four functions that were never scheduling took the worst of them from 196 to 105. `main.rs` went 488 -> 44 the same way. `planning/cycle.rs` is the exception: it is orchestration in the right ring, and its length is `build_solve_request`'s 29 parameters, so splitting it again would repeat the mistake. Historical note: `state/mod.rs` (2026-08-10), `services/planning.rs` (2026-08-23) and `profile/schema.rs` (499/500 at GB-40) each crossed or touched the cap and were split; `profile/schema.rs` still needs `PlannerConfig` moved to its own module before the next config field lands. | N/A — monitoring only, not an actionable fix until a cap is crossed |

---

## Notes

- `AssetProfile` (YAML, `profile.rs`) and `AssetConfig` (runtime physics, `assets/mod.rs`)
  share variant names but hold different inner types. Consider renaming `AssetProfile` →
  `AssetSpec` to avoid newcomer confusion.
- `SimInjectState` mixes three injection behaviours in one flat struct. A tagged `InjectBehaviour`
  enum per field would clarify intent. Track here if promoted to a formal debt item.
- 2026-07-15 recalibration (Part D, following WP5.2): simulated appliance spikes
  (`assets/base_load.rs`) switched from Gaussian pulses (`amplitude × sigma_h × √(2π)`
  energy, uncontrollable tails) to trapezoidal pulses (`amplitude × (duration_h − ramp_h)`,
  directly tunable to real appliance draw), roughly halving ven-1's daily spike energy
  (8.97 kWh/day → ~3.9 kWh weekday / ~4.9 kWh weekend). `AssetHeuristics.daytime_profile_kw`
  was restructured from one 24-hour curve + a `weekday_weights[7]` scalar multiplier to
  `[Vec<f64>; 2]` (weekday/weekend), and profiles now carry weekday-conditional spikes
  (brunch replacing coffee+lunch, dinner shifted earlier on Sat/Sun).
- 2026-08-30 revisit: the weekday/weekend limit above was revisited and lifted.
  `AssetHeuristics.daytime_profile_kw` is now `[Vec<f64>; 7]`
  (`chrono::Weekday::num_days_from_monday()`-indexed — Mon=0..Sun=6), so individual
  weekdays (e.g. Friday-evening routines vs. Tuesday) are distinguishable, not just
  weekday-vs-weekend. The sample-starvation concern the old note raised was addressed two
  ways rather than by capping bucket count: (1) `HeuristicsConfig`'s
  `rolling_window_days`/`ewma_halflife_days` moved from 42/14 to 56/28 — widened enough to
  give each day-of-week bucket a comparable effective sample size to the old weekend
  bucket's, but deliberately kept well under the ~91-day season boundary so the existing
  30-day-vs-window `seasonal_factor` mechanism still holds; (2) the learner's discrete
  zero/nonzero fallback (a bucket/hour cell with no ticks used the flat `overall_mean`, any
  data at all got 100% trust) was replaced with a continuous shrinkage blend
  (`shrinkage_k_days`, default 2.5) that leans a thin bucket toward `overall_mean` in
  proportion to how much data it actually has, rather than an all-or-nothing cliff. All
  three knobs, plus `min_samples_for_confidence`, are now profile-configurable
  (`profile/heuristics.rs`, `Profile.heuristics`). Known residual limitation:
  `min_samples_for_confidence` remains a *global* cold-start gate, not per-bucket — a
  specific day-of-week bucket can in principle be far sparser than the overall average
  without tripping it (thin buckets are protected by shrinkage instead, not by this gate).

---

## Implementation Task List — Gain: High or Medium Items

Scope: every item currently rated Gain exactly **High** or **Medium** (no compound levels
like Medium-High/Low-Medium). No item is currently rated plain High, so this is the 1 item
rated Medium: R-21 (R-22, R-52, R-56, R-24, R-08, R-64, R-63, R-43, R-31, and R-29 were also
on this list and are now resolved — see below; R-58 moved to `docs/FEATURE_VISIONS.md`
2026-08-23 — it turned out to require inventing new fault-input plumbing rather than wiring
an existing one, so it isn't a buildable debt-fix task today).

**Why R-21 is last:** its own entry has no concrete fix, only a workaround (root cause is
allocator/heap-state-dependent inside the native HiGHS library via FFI, not this codebase).
Its task below is an investigation, not a code fix.

Each item's tasks follow this repo's test-first convention (`test-first` rule, `CLAUDE.md`):
write the test, confirm it fails, implement until green. Full verification before considering
an item done: `wsl cargo test -j 2 -p ven-app` under `wsl_lock`, `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `scripts/audit_file_sizes.py`;
update `docs/history/project_journal.md` and remove the item from this register once resolved.

### 1. R-21 — Investigate the intermittent `cargo test` heap-corruption crash

- [ ] 1.1 Try to minimize a standalone repro isolating `run_planner_n48_full_horizon` and
      `solve_ven3_heater_three_tier_zones_feasible` from the rest of the suite.
- [ ] 1.2 Check for a `good_lp`/HiGHS version bump that might already fix an allocator bug;
      try upgrading in isolation and re-running the full suite several times.
- [ ] 1.3 If still reproducible, file an upstream issue against `good_lp` or HiGHS with the
      minimized repro; link it from this entry.
- [ ] 1.4 If no upstream fix lands, formalize the existing workaround (e.g. a
      `scripts/`-level note or CI retry step) rather than leaving it tribal knowledge.
- [ ] 1.5 This item stays in the register until the crash stops reproducing across several
      full-suite runs — remove only then, not merely once a workaround is documented.

## R-88 — the deviation arbiter's release edge is unbounded by its cause

**Where:** `VEN/src/controller/arbiter.rs::reconcile`, surfaced by
`tests/features/isolated/reactive_correction_notifications.feature`.

`reconcile` carries the dead-beat correctors' own last-applied setpoint forward as the
baseline rather than the plan's per-slot allocation — deliberately, and the code says why:
otherwise a tick where the lever does not fire silently reverts the correction and
re-creates the deviation it just resolved.

The consequence is not written down anywhere: **removing the disturbance does not end the
correction.** The still-corrected battery now deviates from plan in the opposite
direction, so the arbiter keeps correcting its own correction until it converges. Observed
on 2026-09-22 with a 3 kW base-load inject: `Reactive correction active` at 19:28:41,
`cleared` at 19:28:43 (before the inject was removed), `active` again at 19:28:54 (after it
was removed), then held for the full 300 s wait.

**Why it is debt rather than a bug:** the behaviour may well be correct dead-beat control
converging, and the feature is disabled by default. What is wrong is that nothing states
the release is unbounded, so a test — and presumably an operator — assumes "cause gone,
correction gone". That assumption is what made this scenario intermittently red for weeks
and read as a load problem.

**To resolve:** decide and document the release semantics (a convergence bound, a maximum
engagement, or an explicit "releases when its own correction has decayed"), then let the
scenario assert that stated behaviour instead of a presumed one. Until then the scenario
asserts only what BL-37 was about: that both edges reach `GET /notifications` during the
scenario.

## R-91 — plan-ahead's horizon check ignores `plan_zones`

**Severity: 🔵 Low** · Effort Trivial · Risk Low · Gain Low — `usage_sim` only; the
`usage_forecast` class derives its horizon from the MILP's own `cum_s` and cannot disagree.

**Where:** `VEN/src/tasks/sim_tick/usage_sim_plan_ahead.rs::sync_plan_ahead_session`,
introduced by `ev-usage-simulation`.

Plan-ahead decides whether the EV's next simulated leave falls "within the planner's
horizon" by reading `profile.planner.plan_horizon_h` directly. But `plan_horizon_h` is
*ignored by the MILP itself* whenever `plan_zones` is set (`profile/planner.rs`'s own doc
comment) — the actual solve horizon is then `plan_zones[0].step_s * total_slots`. A profile
that sets `plan_zones` without also setting `plan_horizon_h` to the matching total gets a
plan-ahead window that disagrees with what the planner really solves over.

**Why it is debt rather than a bug:** every profile committed so far either omits
`plan_zones` (default `plan_horizon_h` is exactly right) or sets both consistently
(`VEN/profiles/usage_sim_test.yaml` does, deliberately, to avoid tripping over this).
Nothing currently ships the mismatched combination. Scope note (`ev-usage-forecast`,
2026-09-26): this affects the `usage_sim` class only. The `usage_forecast` class derives its
horizon from the MILP's own `cum_s`, so it cannot disagree with the solve by construction —
which is also the shape the fix above should aim for.

**To resolve:** give `Profile`/`PlannerConfig` one method that returns the *effective*
horizon (`plan_zones`-derived when set, else `plan_horizon_h`) and have both
`tasks/planning/cycle.rs` and `usage_sim_plan_ahead.rs` call it, instead of the cycle task
reading `plan_zones` and plan-ahead reading `plan_horizon_h` as if they always agreed.

## R-100 — a VTN SoC command is modelled as if it were the user's own intent

**Severity: 🟡 Medium** · Effort Medium · Risk Medium · Gain Medium — an ownership question, not
a defect: nothing is broken today because the path is disabled, but the modelling must be settled
before a VTN can influence EV charging again.

**Where:** `VEN/src/tasks/poll_signals.rs::apply_vtn_charge_state_session` (preserved but **not
called** as of 2026-10-03), `VEN/src/entities/device_session.rs::EvSessionOrigin::Vtn`,
`VEN/src/controller/openadr_interface.rs::parse_charge_state_setpoint`.

A VTN `CHARGE_STATE_SETPOINT` used to create an `EvSession` — the same object a user's own
charging request creates, distinguished only by `origin`. Those are different things: one is an
external constraint from the grid, the other is intent about the user's own car and their own
travel. Modelling them identically says a grid preference *is* the driver's plan.

The spec's own wording supports the doubt: `CHARGE_STATE_SETPOINT` is "the state of charge of an
energy storage resource", which fits a grid-scale or aggregator-controlled battery far better than
a household EV whose schedule follows its driver.

**Why it stopped being theoretical.** The EV session queue forbids overlapping sessions, so a VTN
session and a user session compete for one calendar: a grid signal can be refused because the user
has booked their car, and a VTN session can block the user from booking it at all. The old single
slot hid this — the last writer simply won, which is also how a VTN signal could silently overwrite
a user's session whose target merely differed.

**Current state:** disabled on the user's instruction (2026-10-03), code preserved verbatim with
its ownership-by-id bookkeeping and withdrawal semantics intact, and pinned shut by
`a_charge_state_signal_creates_no_ev_session`. The two tests covering the preserved path drive it
directly rather than through `apply_signal_changes`.

**To resolve:** decide what a VTN SoC command *is* here. The likely answer is a planner
**constraint** weighed against the user's sessions (alongside capacity limits and prices), not a
session competing with them — an obligation the MILP receives without an `EvSession`, much as
`ev-usage-forecast` already hands the planner a predicted departure without writing one. Until
then it stays off; do not re-wire it as a session.

## R-98 — the EV variable set is declared in two places

**Severity: 🟡 Medium** · Effort Small-Medium · Risk Medium-High · Gain Medium — a silent
plan/dual divergence class: the two declarations can disagree and nothing fails loudly.

**Where:** `VEN/src/assets/ev_milp.rs::EvMilpContext::declare_vars` (the real one) and
`VEN/src/controller/milp_planner/solver_duals.rs::declare_fixed_ev_vars` (phase-2 dual
extraction, which re-declares the same variables with `z_ev_on` pinned).

`declare_fixed_ev_vars` has always re-declared `p_ev`, `z_ev_on`, `e_seg` and `e_ev_extra` by
hand, because the dual pass needs different bounds on `z_ev_on`. R-93 widened the set it has to
mirror: `soc_ev`, `drop_unmet`, `shortfall_soc` and `battery_kwh` were added to `EvMilpVars`,
and `constraints()` — shared by both paths — now references them, so the dual path had to grow
the same four. Their bounds (the SoC floor capped at the live reading, each slot's drop cap,
each obligation's shortfall cap) are now written out twice.

**Why a second thing waits on this (added 2026-10-04, R-76 follow-up).** `solver_duals` reads a
shadow price off each slot's *power-balance* row only (`power_balance_refs`). Pricing an
`IMPORT_RESERVATION_FEE` — the spec's companion to the `IMPORT_RESERVATION_CAPACITY` report R-76
fixed, "amount per unit of import capacity that the VEN is willing to pay" — wants the dual on the
*import-capacity* row instead, which nothing extracts today. That is the economically correct
number (marginal willingness to pay for one more kW of allowance) and it would come from the same
solve as the request, so one concept with one source. It is deliberately not attempted before
this entry is settled: adding a second dual to a declaration that is already written out twice
would widen exactly the divergence R-98 is about. Until then the VEN reports what capacity it
wants and not what it is worth, which `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`'s
UC-07 states plainly rather than leaving implied.

**Why it is debt rather than a bug:** the two copies agree today, and the full suite passes. But
the only thing keeping them in step is that someone remembers to edit both — exactly the shape
`one-concept-one-function` exists to prevent, and the bounds involved are no longer trivial.

**To resolve:** give `EvMilpContext` one `declare_vars_with(z_on: ZOnPolicy)` (free binary vs.
pinned continuous) and have both callers use it, so the variable set and its bounds exist once.

## R-99 — the fleet-chart UI scenario depends on elapsed time, by an unknown rule

**Severity: 🟡 Medium** · Effort Small-Medium · Risk Low · Gain Medium — a test that fails on a
fresh stack and passes minutes later reads as a product defect to whoever is bisecting, and costs
a full E2E run to dismiss. It cost one here.

**Where:** `tests/features/fleet_telemetry.feature:48` ("The fleet page draws a line for every VEN
that is reporting"), `VTN/ui/src/components/FleetPowerChart.tsx`, `VTN/ui/src/pages/Fleet.tsx`.

`FleetPowerChart` renders `fleet-chart-empty` *instead of* `fleet-power-chart` while
`rows.length === 0`, so the selector the scenario waits 60 s for cannot exist until the chart has
data. On a freshly started stack it does not, and the scenario fails.

**What was measured (2026-10-02), so the next attempt does not repeat it:**

- It is **not** the host-contention flake the resilience suite is. It failed at host load 7.31 and
  again at 2.33; the resilience scenario that failed alongside it passed on the quiet re-run.
- It is **not** caused by the EV SoC work: it reproduces on a branch based on `main` with none of
  `048-ev-soc-state-variables` in it.
- Waiting for the existing `I wait for the fleet history of the last 10 minutes to have samples`
  step is **not sufficient**. That step passed — `/api/fleet/power` *did* have rows — and the chart
  was still empty. So the store having samples is a weaker condition than the chart rendering.
- In both runs the same scenario **passed** in the later `@isolated` pass, minutes after failing in
  the main pass, with no code change between them. Elapsed time is the variable.

**What is still unknown:** what the Fleet page's own fetch receives when the chart is empty. The
page asks for 15 minutes at `stepSeconds: 5` (`Fleet.tsx`), the probe asked for 10 minutes at the
same step, and the two disagreed — so the next step is to read the actual response the browser gets
(or the 502 it may be getting: `tests/entrypoint.sh` documents nginx caching a stale BFF IP, which
turns every `/api/*` call into a 502 and would present exactly as an empty chart).

> **Both sentences above are wrong, corrected 2026-10-03 — see "The mechanism" below.** The page
> does *not* ask for 15 min at 5 s; it opens on 24 h at 900 s. And the 502 theory is dead. They are
> left in place rather than deleted because two attempts were aimed at them, and a reader who finds
> only the conclusion cannot tell which leads were already spent.

**To resolve:** find the real condition and wait for *that*, or fix the page if the answer is that
it never recovers without a reload. Raising the 60 s timeout is not a fix — it hides a rule nobody
has written down.

**Update 2026-10-03 — two candidates eliminated, one instrumented, still open.**

*The 502 theory above is already mitigated and should not cost another run.*
`tests/nginx-test.conf` carries `resolver 127.0.0.11 valid=5s ipv6=off` with a *variable*
upstream (`set $bff_upstream` then `proxy_pass $bff_upstream`), which is exactly the construct
that makes nginx re-resolve rather than cache an IP at startup. The warning in
`tests/entrypoint.sh` describes a hazard this config already answers.

*Why the existing precondition step could be satisfied while the chart stayed empty.* The probe
calls `http://test-bff:8090/api/fleet/power` **directly**, container-to-container
(`features/helpers/api_client.py`'s `BFF_BASE_URL`); the browser fetches `/api/*` **through the
UI's nginx proxy**. They are different transports, so a passing probe cannot speak for what the
page received. That asymmetry — not the window size — is why "wait for history, then assert on
the chart" did not hold.

*The scenario now diagnoses itself* (`fleet_steps.py::_fleet_chart_diagnosis`). On timeout it
reports whether `fleet-chart-empty` is rendered (present = no data, absent = the component never
rendered — two different bugs the bare selector timeout could not distinguish) and what
`/api/fleet/power` returns **when fetched inside the page**, i.e. over the app's own transport.
The next occurrence names its own cause instead of costing a run.

*Evidence so far, and why this stays open.* With the precondition in place the full suite passed
297/0 (+15 isolated) on 2026-10-03, where the run immediately before it — same host, no
precondition — failed this one scenario. That is one green against one red, on a scenario already
known to be non-deterministic. This register's own R-97 is a long account of what a one-sample
comparison is worth, so it is recorded as evidence and not as a fix. **Do not close this on a
green run**; close it when the diagnostic has fired and named the condition, or after enough full
runs that the old failure rate would have shown.

*An argument made here on 2026-10-03 and withdrawn the same day.* It read: data latency fits
badly, because the two single-feature runs on a freshly built stack both passed while the failure
happened in a full suite where the store was already full — so contention fits better. **The
comparison was confounded**: those single-feature runs had the precondition step and the failing
run did not, so two variables moved at once. Recorded because the conclusion ("force reproduction
under load") was acted on, and because an invalid comparison that *sounds* controlled is the
failure mode R-97 is also about.

**The mechanism, found by reading `Fleet.tsx` rather than by another run (2026-10-03).**

```tsx
const WINDOWS = [{minutes: 15, stepSeconds: 5}, …, {minutes: 1440, stepSeconds: 900}];
const [windowIndex, setWindowIndex] = useState(3);   // opens on 24 h / 900 s
```

The page opens on **24 h at 900 s steps**, not the 15 min at 5 s this entry recorded. The probe
waits on **10 min at 5 s**. Same endpoint, a question **144x apart in window and 180x apart in
step**. `fleet_store::group_and_resample` builds each VEN's series from the rows that exist and
then `resample_uniform(step, Mean)`s it, so a store holding a few minutes of telemetry yields
**zero 900 s buckets** while yielding plenty of 5 s ones. `FleetPowerChart` renders
`fleet-chart-empty` *instead of* the chart whenever `rows.length == 0`.

That one fact accounts for every observation this entry collected:

| Observation | Accounted for |
|---|---|
| "the step passed and the chart was still empty" | rows at 5 s, none at 900 s — both true at once |
| fails early in a run, passes in the `@isolated` pass minutes later | the store crosses one 900 s bucket |
| "elapsed time is the variable" | that is the variable, and now it has a rule |
| failed at load 7.31 *and* 2.33 | load-independent, as recorded |
| reproduces without any of `048-ev-soc-state-variables` | unrelated, as recorded |

**Resolved for the test (2026-10-03, option B):** the scenario now drives the page's own window
selector to "15 min" before asserting, so elapsed time stops being a variable instead of being
waited out. Assertions unchanged. The diagnostic added the day before had copied this entry's wrong
15 min / 5 s figure and would have reported counts for a query the page never makes — also fixed.

**What stays open, and it may be the more useful half.** An operator opening Fleet on a freshly
started VTN reads *"No telemetry stored for the last 1440 minutes"* — which is false: telemetry
exists, there is just less than one bucket of it. The page cannot distinguish "the store is still
filling" from "nothing is reporting", and it defaults to the window where that is most likely.
Deciding whether the default window is right, and whether that empty state should say something
truer, is a product question and is what remains of R-99.

## R-94 — the MILP does not model the heater's thermostat deadband

**Severity: 🟡 Medium** · Effort Small-Medium · Risk Medium · Gain Medium — a new plan/actual
divergence class (the asset can refuse planned dispatch), with no UI surface saying why.
Narrow window in practice.

**Where:** `VEN/src/assets/heater_milp.rs` (tank model) vs
`VEN/src/assets/heater_thermostat.rs` (the deadband), introduced with
`thermostat_delta_c` (2026-09-27).

The MILP bounds tank energy at `temp_max_c` (`heater_milp.rs`'s `e_max`) but knows nothing
about "once at the ceiling, stay off until `temp_max_c - thermostat_delta_c`". A plan that
wants to absorb PV surplus into a nearly-full tank can therefore be refused by the asset,
which surfaces as a plan/actual deviation rather than as an infeasibility.

**Why it is debt rather than a bug:** the divergence window is exactly the region where the
tank has almost no absorbing capacity left to plan for, and the deviation arbiter already
handles a setpoint the asset does not follow. Nothing observed on the fleet yet — recorded
because it is a new divergence class, not because it has bitten.

**To resolve:** give the heater's MILP context a "may not run below this stored-energy level
once latched" constraint derived from the same `thermostat_delta_c`, so the planner stops
proposing dispatch the asset will refuse. Pairs with the existing switching-penalty work
(`lambda_heat_sw_eur`), which is the economic half of the same anti-chatter concern.

## R-95 — no sustained-commitment series exists alongside the capability curves

**Severity: 🔵 Low** · Effort Small · Risk Medium (see the reverted attempt) · Gain Low —
the mislabel is fixed; what remains is an optional extra series.

**Where:** `VEN/ui/src/components/controller/charts/SiteHeadroomChart.tsx` and
`CapacityForecastChart.tsx`, fed by `CapacityCurve::steps`.

**Partially resolved (2026-09-27):** the mislabel is fixed. The series now read "Import/Export
capability", the export line's tooltip says "net import — nothing left to export" when
positive (`charts/capabilityFormat.ts`), and
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` has a "How to read the Site Headroom
panel" section. Found because ven-3's export curve pointed upward at night and looked like a
sign bug; it was the site having no battery and a thermostat forcing reheats.

**What remains:** there is still no series answering "what could the site *hold* for this
window" — energy ÷ duration — which is a real and different question from instantaneous
capability.

**To resolve, if it is ever wanted:** add such a series *alongside* the capability curves,
never replacing them. The attempt that replaced them (`4459a4ab`, reverted in `e47ae50d`)
destroyed the panel's near-term forecasting value — a cumulative average carries the whole
history since the anchor, so a real −11 kW EV-departure cliff smeared into a slow decay — and
sampled the average only at the instantaneous curve's sparse breakpoints, which `stepAfter`
then held flat, overstating capability by 11 kW for two hours. Any such series must be densely
sampled and drawn as an interpolated line, not a step.

## R-97 — MILP solve time sits close to its own timeout

**A claimed regression from `ev-comfort-piecewise-core`, retracted the same day (2026-09-29).**
Kept because the method is the lesson, not the conclusion.

Post-deploy sampling showed ven-11 (EV + base load, no heater) at a 6.4 s median against 114 ms
the day before, and ven-2 at 45-54 % TIME_LIMIT against 5.6 %. That was reported as a 56x
regression caused by the change. **It was not established, and the attribution was wrong.**

What refuted it:

- **The step precedes the deploy.** Plotting consecutive solves instead of hourly medians, ven-2
  jumps 15.4 s -> 66.0 s between 08:13:28Z and 08:18:44Z, and ven-11 402 ms -> 21984 ms between
  08:09:54Z and 08:14:54Z. Both hosts stepped within five minutes of each other at ~08:15Z. The
  feature reached Node1 at 09:17Z and Node2 at ~09:20Z. A cause cannot follow its effect.
- **It does not reproduce offline.** Seven band shapes on the production 288-slot grid solve in
  0.04-0.09 s (`bench_ev_band_solve_cost`), including the fleet's actual shape — no bands at all,
  one synthetic zero-reward guarantee band, which is the *fastest* of the seven. Through the full
  two-phase planner with `usage_forecast` and charge planning, 0.08-0.22 s
  (`bench_ev_session_solve_cost`). Nothing in the band model costs seconds.
- **Other VENs moved the other way in the same window.** ven-7 went 28.3 s -> 5.0 s across 08:15Z
  while ven-19 and ven-15 stayed flat — all pre-deploy. That is host and fleet conditions, not a
  code change.

**The methodological failures, which are why this entry is kept.**

1. *A one-sample baseline.* The change's own baseline table held one `solver_ms` per VEN. ven-2's
   entry was 11.9 s; its real distribution over the preceding 1910 solves was median 15.7 s, p90
   31.5 s, tail to the ceiling. A single draw cannot distinguish a 4x regression from an ordinary
   sample — in either direction.
2. *Time-of-day matching is not load control.* Comparing the same clock window on consecutive days
   looks controlled and is not, when the second day is full of E2E runs, image builds and fleet
   redeploys on those same hosts — activity caused by the very work being measured.
3. *Hourly medians hid the boundary.* They placed the jump "somewhere in the 08-09Z hour", which
   was compatible with the deploy. Consecutive-solve sequences placed it at 08:15Z, which is not.
4. *A confident fix for an unverified cause.* Band merging was shipped as the remedy and changed
   nothing (6406 -> 6752 ms). That should have been read immediately as the diagnosis being wrong
   rather than the fix being insufficient. The merge is kept on its own merits — fewer variables,
   provably identical valuation, pinned by `bands_at_the_same_bid_are_one_band`.

**What did cause the step: an EV usage-forecast transition.** Both VENs stepped as their EV
changed availability state, not as code was deployed.

- **ven-11** (assets: `base_load` + `ev` only — nothing else can explain it): `GET /ev-usage-sim`
  gives `leave_at 2026-09-29T08:09:39Z` with a **65.1 % expected SoC drop**. The step is the very
  next replan, 08:14:54Z.
- **ven-2** (evening departure / morning return pattern, `return_at ~07:59`): its EV **returned**
  at ~07:59Z with a 17.4 % drop; the step follows three cycles later at 08:18:44Z.

The mechanism is the same in both directions: once a car is away and returning with a real SoC
drop, the planner must place a substantial charge *after a predicted return* instead of charging a
car that is present — a materially harder problem, and one whose difficulty varies day to day with
the randomised drop (`soc_drop_pct_mean`/`stddev`). That is why the effect appears on one morning
and not the previous one.

This capability is `ev-usage-forecast`'s, made reachable for an away-now car by `96f259e9`
(deployed 2026-09-27) — which is also when ven-11's daily median first stepped, 136 ms on the 26th
to 563 ms on the 27th, two days before `ev-comfort-piecewise-core` existed on any host.

**Confidence:** strong for ven-11 (one cycle, single-asset site, exact time match), good for ven-2
(same mechanism class, three cycles). Neither is proof of a *quantitative* cost model for
away-window planning, which is what the next measurement should establish.

**The EV half, measured properly (2026-09-29).** `bench_ev_session_solve_cost` now repeats each
variant five times and reports min/median/max, because single samples on a laptop swing 3-6x —
enough to invent or erase the whole effect. With that, the away-window step is solid and repeatable
on an EV-only site at 288 slots through the real two-phase planner:

| variant | min | median |
|---|---|---|
| no EV session | 0.16 s | 0.17 s |
| firm EV session | 0.26 s | 0.31 s |
| forecast, car **home** | 0.19 s | 0.21 s |
| forecast, car **away** (20/65/90 % drop) | 0.90-0.94 s | 0.95-1.09 s |
| forecast, just returned | 1.02 s | 1.36 s |

So ~5x for a car that is out, and **the SoC drop size is irrelevant** (20 %, 65 % and 90 % all land
together) — the cost is the away window itself, not the energy the trip needs. That kills the
"large drop means a large charge to place" reading.

**Re-measured after R-93 (2026-10-02), same bench, same machine.** Making the EV's SoC a solved
variable did not cost solve time — it reduced it, and the away-window penalty this entry was opened
to explain is largely gone:

| variant | before (min) | after R-93 (min) | status after |
|---|---|---|---|
| no EV session | 0.16 s | **0.05 s** | Optimal |
| firm EV session | 0.26 s | **0.11 s** | GapLimit |
| forecast, car **home** | 0.19 s | **0.18 s** | GapLimit |
| forecast, car **away** 20 % | 0.90 s | **0.74 s** | Optimal |
| forecast, car **away** 65 % | ~0.90 s | **0.15 s** | GapLimit |
| forecast, car **away** 90 % | ~0.94 s | **0.14 s** | GapLimit |
| forecast, just returned | 1.02 s | **0.20 s** | GapLimit |

These are 5-repeat minima, the estimator this entry already argued for, and "noise only ever adds
time" makes a *lower* minimum the trustworthy direction — so the drop is real rather than a quiet
laptop.

**One caveat, stated rather than buried:** the "after" column reports `GapLimit` on five of seven
variants, and the earlier table did not record status at all, so part of the speed-up may be the
solver reaching its 2 % `mip_gap_target` sooner on the new formulation rather than doing less work
to the same quality. A fair like-for-like needs the gap sweep
(`bench_mip_gap_quality_sweep`) re-run against the SoC-variable model; until then read the table as
"no regression, probably a real improvement", not as a certified 5x.

Why it would plausibly be faster: the obligation is now a bound on one `soc_ev` variable instead of
a cumulative-energy sum over a deadline-masked slot range plus a floor that had to be pre-capped at
a reachability estimate, and the penalised shortfall slack removes a tight equality that sat next to
infeasibility. Fewer vacuous rows per away slot, and a relaxation the solver can bound earlier.

**Attempt 1 — tighten the away slots — is not demonstrated.** Away slots declare `p_ev` over the
full `[0, p_max]` range and force it to zero only through `p_ev[t] <= 0 * z_ev_on[t]`, and their
`z_ev_on` is a *binary* fixed at 0. Bounding that power to zero directly, declaring those `z`
continuous, and skipping the two now-vacuous rows per away slot gives: away-20 % 1.25 -> 0.94 s,
away-65 % 1.10 -> 0.91 s, but away-90 % 0.81 -> 0.90 s and just-returned 0.54 -> 1.02 s. Two better,
two worse, i.e. inside cross-process variance. **Not shipped** — model size looks like the wrong
lever here, exactly as it was for the heater (see GB-40's refuted single-integer encoding).

**Next measurement, before any further attempt:** time the two phases separately, as
`bench_heater_variants` already does for the heater. GB-40 established that phase 2 never binds and
burns its full budget; if the EV's 5x also lands in phase 2 then the away window is a phase-2
problem and every model-size idea is aimed at the wrong half.

**Phase split settles it: the EV away window is not worth optimising (2026-09-29).**
`bench_ev_phase_split` times the two phases separately, five repeats, minimum reported.

EV-only site, 288 slots:

| variant | phase 1 | phase 2 | statuses |
|---|---|---|---|
| no forecast | 0.044 s | 0.036 s | Optimal / Optimal |
| forecast, car home | 0.035 s | 0.079 s | Optimal / Optimal |
| forecast, car **away** | 0.039 s | **0.833 s** | Optimal / GapLimit |
| forecast, away, 90 % drop | 0.033 s | 0.681 s | Optimal / GapLimit |
| forecast, just returned | 0.037 s | 0.490 s | GapLimit / Optimal |

**Phase 1 is flat.** The entire away-window cost is phase 2 — ~20x — which is why the phase-1
tightening above was inconclusive: it aimed at the half that spends 40 ms.

Same site **with a heater**, the shape that actually times out:

| variant | phase 1 | phase 2 | statuses |
|---|---|---|---|
| heater + EV, no forecast | 57.5 s | 57.4 s | TimeLimit / TimeLimit |
| heater + EV, car home | 57.3 s | 57.2 s | TimeLimit / TimeLimit |
| heater + EV, car away | 56.9 s | 56.3 s | TimeLimit / TimeLimit |

**Both phases are already pinned at the timeout with or without the EV, and with no usage forecast
at all.** There is no headroom for the EV to consume, so removing the away-window cost cannot move
any VEN that times out; and the VENs where the 20x is visible (EV-only, e.g. ven-11) run at
0.8 s against a 60 s-per-phase budget, solving OPTIMAL. **Conclusion: do not optimise the EV away
window.** It has no operational payoff at either end of the fleet. 100 % of the real timeout is the
heater (GB-40).

**The cheap option this exposes, which is not EV-specific.** Phase 2 minimises friction subject to
`phase1_cap_expr <= c_star + epsilon`, so *by construction* everything phase 2 can change is
bounded by `phase2_epsilon_eur`. On a heater VEN it spends its entire 57 s budget and still returns
TimeLimit — i.e. it cannot prove the refinement it is buying, while the most that refinement can be
worth is one epsilon. A separately configurable, much shorter phase-2 timeout would therefore cut
heater-VEN solve time by roughly half at a cost bounded by epsilon, without touching the
formulation. That is a measurement worth doing (and R-27 already asks for these solver constants to
be configurable).

**CORRECTED: phase 2 is *effective*; the earlier "inert" finding was a test-fixture artefact
(2026-09-30).** The conclusion below about the 5 s budget stands, but the reasoning that produced it
was wrong and is replaced here.

*What was claimed:* that phase 2 returns its warm start unchanged on every instance — 0 of 288 slots
moved, friction identical from a 1 s to a 60 s budget — and therefore burns half the planner's
budget for nothing.

*Why it was wrong:* every one of those runs used `make_tariffs(...)`, a **flat** tariff. Flat prices
give phase 1 no reason to fragment the heater schedule, so there was no chatter for phase 2 to
remove. The inertness was a property of the fixture, not the planner.

*What phase 2 actually does,* measured with a diurnal tariff at ven-2's own production settings
(`mip_gap 0.06`, `phase2_epsilon_eur 1.00`, `bench_phase2_epsilon_sweep`): it moves 68 of 288 heater
slots and 31 import slots, cutting friction from 4.62 to 2.08 EUR. Against phase 1 alone it roughly
**halves heater switching** — 58 switches over 48 h down to 32, and 28 down to 11 over the first 8 h.
Phase 2 earns its place.

*The starvation threshold is real, just below production.* At `mip_gap 0.06`: epsilon 0.17 moves
nothing, epsilon 0.50 moves 59 heater slots, 1.00 moves 68, 5.00 moves 87. ven-2's 1.00 sits
comfortably above the threshold. A profile left at a tight epsilon would get no smoothing at all.

*Budget vs value* (`bench_phase2_budget_with_real_prices`, same settings): friction 4.6201 at 1-2 s
(no change), **3.4555 at 5, 10 and 20 s** (an exact plateau — 76 heater slots), 2.0805 at 60 s. So
there is no useful middle setting: 10 s and 20 s buy nothing over 5 s, and the remaining gain
appears only somewhere past 20 s.

*Why 5 s is nonetheless right* (`bench_phase2_budget_executed_window`): with `replan_interval_s` at
300 s only a plan's first slots ever reach the relay, and there 5 s and 60 s are **identical** — 2
switches at 25 min and 2 at 1 h under both. Over 8 h the 5 s solution has *fewer* switches (11 vs
15). The horizon-wide friction difference lives in slots that are replaced before they run.

**Phase 2 returns provably suboptimal incumbents, and its response to configuration is not
monotone (2026-09-30).** This is the sharpest thing the sweeps show, and it comes out of the data
without needing a mechanism.

Phase 2 minimises friction subject to `cost <= c_star + epsilon`. Raising epsilon strictly enlarges
the feasible set, so the true optimum at a larger epsilon **cannot be worse**. The sweep violates
that in both gap rows:

| mip_gap | epsilon | heat slots moved | friction |
|---|---|---|---|
| 0.02 | 0.17 | 51 | 2.2916 |
| 0.02 | **0.50** | **0** | **4.9113** |
| 0.02 | 1.00 | 47 | 1.9949 |
| 0.02 | 5.00 | 74 | 1.2471 |
| 0.06 | 0.17 | 0 | 4.6201 |
| 0.06 | **0.50** | 59 | **1.8315** |
| 0.06 | **1.00** | 68 | **2.0805** |
| 0.06 | 5.00 | 87 | 1.4559 |

Friction *rises* from 2.29 to 4.91 as the cap loosens from 0.17 to 0.50 (gap 0.02), and from 1.83 to
2.08 loosening 0.50 to 1.00 (gap 0.06). Both are impossible for optimal solutions, so phase 2 is
landing on substantially suboptimal incumbents — consistent with TimeLimit at 58-59 s in all eight
cells. The cells showing zero movement are therefore **search failures, not structural inertness**.

Three consequences:

1. **`phase2_epsilon_eur` cannot be tuned by measurement on one instance.** The response is not
   monotone, so a sweep can rank a tighter cap above a looser one purely by incumbent luck.
2. **Phase 2's freedom is inversely coupled to phase 1's quality.** The cap is anchored on
   `c_star` = phase 1's objective, so a sloppier phase 1 gives phase 2 more absolute room. Visible
   at epsilon 0.17: a timed-out phase 1 (gap 0.02) lets phase 2 move 51 heater slots, a converged
   one (gap 0.06) lets it move none. Tuning `mip_gap_target` therefore silently retunes how much
   smoothing the plan gets — an odd property for a lexicographic two-phase design.
3. **Phase 2 is a heater-only pass in practice.** The EV column is 0 in all eight cells.

**The budget is a staircase, not a curve** (`bench_phase2_budget_with_real_prices`): nothing below
~5 s, an exact plateau at 5/10/20 s (friction 3.4555, 76 heater slots), then a better basin past
20 s (2.0805, and only 68 slots moved — fewer edits, better result). Classic incumbent-update
behaviour.

**Default raised 5 s -> 15 s as a result.** The plateau means 15 s delivers the same plan quality as
5 s, while the threshold behaviour means 5 s is fragile: it is a **wall-clock** budget, not a work
budget, and production hosts run 85-89 % busy, so 5 s there buys less solver work than 5 s on an
idle laptop and would intermittently land in the found-nothing regime. 15 s keeps the plateau with
2-3x margin, still far below the 60 s it replaced (ven-2 total was 64 s before any of this).

**Heater MILP difficulty is set by the tank's thermal slack — a property of the installation, not
the formulation (2026-09-30).** This is the first mechanism proposed today that survived its control,
and it bears directly on GB-40.

Live ven-2 and ven-3 run the same assets, the same 288-slot 48 h grid and the same
`mip_gap_target` (0.06 — confirmed in both profiles, so the tolerance is controlled), yet phase 1
takes **227-309 ms** on ven-2 and **11-46 s** on ven-3. The profiles differ in tank physics:
ven-2 has 2000 L across a 40 K band, ven-3 has 200 L across 15 K — about 27x the usable slack.

`bench_phase1_vs_tank_slack` varies volume and band with grid, tariffs, gap and assets held fixed,
three repeats:

| volume L | band K | slack kWh | phase 1 (3 runs) | switches |
|---|---|---|---|---|
| 200 | 15 (**ven-3**) | 3.5 | 1.29 / 1.22 / 2.00 s | 53 |
| 200 | 40 | 9.3 | 0.66 / 0.70 / 1.21 s | 14 |
| 500 | 15 | 8.7 | 0.61 / 0.61 / 0.67 s | 12 |
| 1000 | 15 | 17.4 | 0.44 / 0.42 / 0.50 s | 16 |
| 2000 | 15 | 34.9 | 0.22 / 0.18 / 0.19 s | 4 |
| 2000 | 40 (**ven-2**) | 93.0 | 0.19 / 0.21 / 0.24 s | 22 |

Monotone in slack in every run, ~6-8x end to end, and the endpoints are exactly the two live VENs.
These solves finish (GapLimit) so they are deterministic — switch counts are identical across runs,
unlike the time-limited phase-2 measurements elsewhere in this entry.

**Switch count does *not* predict difficulty** (2000 L/40 K has 22 switches and is fastest; 500 L/15 K
has 12 and is slower), so "number of forced thermostat cycles" is not the mechanism. Slack itself is
the predictor. The likely reason is that low slack narrows the feasible tank-trajectory corridor
until integrality binds hard — standard MILP behaviour — but that is **not verified**.

**Why this matters for GB-40.** GB-40 has pursued the formulation for weeks on the theory that the
stage binaries carry the power level and weaken the relaxation, and three reformulations failed:
the single-integer encoding bought nothing, tier-bounded continuous power was unsound (~25 % cheaper
unphysical answers), dwell-time constraints were worse. GB-40 also records variance it cannot
explain — "not every heater VEN is slow (ven-2 18.2 s, ven-20 29.0 s)". Thermal slack explains that
variance, and explains why the reformulations failed: when slack is tight the discreteness they tried
to relax away is load-bearing.

**Levers this opens, none of them a reformulation:**
1. **Widen a needlessly narrow thermostat band** where the installation permits. Measured: 200 L
   going 15 K -> 40 K roughly halves phase 1 (1.29 -> 0.66 s). A physical/config change, and it
   also reduces switching (53 -> 14).
2. **Treat slack-poor VENs differently** — a looser `mip_gap_target` or coarser far zones for them
   specifically, rather than fleet-wide settings that are wasted on slack-rich sites.
3. Far-zone coarsening (measured 2.6x earlier in this entry) now has a mechanism behind it.

**Caveat on magnitude:** the bench's hardest row is 1.3-2.0 s while production ven-3 is 11-46 s, so
the bench under-represents ven-3 by roughly an order of magnitude — its base load, PV, EV state and
real tariffs compound on top. The *direction* is confirmed; the absolute scale is not this bench's
to give.

**ROOT CAUSE of ineffective smoothing: 9 of 11 heater VENs run an epsilon below the threshold at
which phase 2 can do anything (2026-09-30).** Found by the new `planner: phase timings` log, within
minutes of deploying it.

Live ven-2 vs ven-3, same host, same code:

| | ven-2 | ven-3 |
|---|---|---|
| phase 1 | **227-309 ms** | **11 131-45 566 ms** |
| phase 2 | 13-15 s, sometimes GapLimit | ~15 s, always TimeLimit |
| `phase2_epsilon_eur` | **1.00** | **0.17** |
| friction | ~1.33 | 4.30-5.60 |

Auditing every profile: `default_phase2_epsilon()` is **0.02**, and the sweep above shows epsilon
0.17 already yields **zero** heater slot changes at gap 0.06. Only **ven-2** sets a working value
(1.00); ven-3 sets 0.17; every other heater VEN — ven-5, ven-10, ven-12, ven-14, ven-15, ven-17,
ven-18, ven-20 — runs the 0.02 default. **They get no smoothing at all, and now pay a 15 s phase 2
for it.**

There is a mechanical floor behind this, not just a tuning preference: removing a heater switch
requires moving energy in time, which costs something. If epsilon is below that cost, no improving
move is feasible and phase 2 provably finds nothing however long it runs. ven-2's own profile
comment states the scale — effective switching cost `3.0 x 10/60 = 0.50 EUR/switch`, epsilon set to
2x that. The 0.02 default is **25x below a single switch**.

**Validation gap:** `validate.rs` already rejects an epsilon that is too *large* relative to
switching cost, but has no lower bound — nothing warns that phase 2 has been configured into
uselessness. A check that epsilon is at least the effective cost of one switch (when a heater is
present) would have caught this on every one of those nine profiles. Making it a hard error would
stop nine VENs from booting, so it should warn, or ship together with the profile fix.

**Remedies, for a decision rather than an unattended change:**
1. Raise `phase2_epsilon_eur` to ~2x effective switching cost on heater VENs (ven-2's 1.00 is the
   worked example) — phase 2 then halves switching, 58 -> 32 over the horizon.
2. Or set `phase2_epsilon_eur: 0.0` on them, which disables phase 2 outright and reclaims the 15 s.
   Honest, and strictly better than paying for a pass that cannot act.
Doing neither is the only option with no argument for it.

**Also corrects a claim made earlier today:** "phase 1 is negligible in production (~0.25 s)" was
drawn from ven-2 alone and is **wrong for ven-3**, whose phase 1 runs 11-46 s. Phase-1 cost is
strongly VEN-dependent, so parking phase-1 optimisation fleet-wide was premature — it is negligible
on ven-2 and dominant on ven-3.

**Phase 2's result at a fixed budget is NOT reproducible — every budget comparison here is
suspect (2026-09-30).** Two runs of `bench_what_is_in_phase2_friction` / `bench_phase2_budget_with_real_prices`
on the same instance, same code, same 60 s budget returned friction **2.0805** and **3.4555**. A
time-limited search explores however many nodes the host grants in that window, so its incumbent
depends on machine load. GB-40's finding that "HiGHS is deterministic here" applies to solves that
*finish*; it does not extend to time-limited ones.

Consequences for everything measured on this axis:

- The 5/10/20 s "plateau" and the better basin "past 20 s" may both be single-run artefacts. In the
  second run **5 s and 60 s were identical** (friction 3.4555, 32 switches), which argues 5 s is
  sufficient.
- `phase2_solver_timeout_s: 15` remains defensible as **load margin** — phase 2 achieves nothing
  below ~5 s, and a wall-clock budget pinned at that threshold on an 85-89 %-busy host would
  intermittently deliver no smoothing. The separate claim that a longer budget *captures more value*
  does not survive.
- Any future budget or epsilon decision needs repeated runs, not one sweep. This is the same
  one-sample trap that produced the retracted regression earlier in this file, in a new place.

**What is robust across both runs:** phase 2 halves heater switching, 58 -> 32 over the horizon.
Its value is real; only its budget-sensitivity was noise.

**A refuted hypothesis, recorded so it is not re-proposed.** `PV_USE_TIEBREAK_EUR_PER_KWH` (0.005
EUR/kWh) is documented as a bias "small enough that any real constraint still dominates" — which is
calibrated against phase 1's objective (tens of EUR of energy cost), while phase 2's objective is
friction-only (a few EUR). The concern was that the same constant becomes first-order in phase 2 and
that `friction_eur` is really a switching/PV blend. **It is not:** PV utilisation is identical
(91.87 kWh) across phase 1, the 5 s and the 60 s solutions — it is saturated, nothing is traded —
and the term is worth only -0.4594 EUR, not the ~2 EUR estimated from assuming continuous 6 kW
output. The estimate, not the constant, was wrong.

**Rule for any future phase-2 measurement: use a varying tariff.** A flat fixture makes phase 2 look
inert and will reproduce this wrong conclusion.

**Fix shipped:** `planner.phase2_solver_timeout_s`, separate from `solver_timeout_s`, defaulting to
**5 s** (validated non-zero; `phase2_epsilon_eur = 0.0` remains the documented way to disable phase
2 entirely). This is safe by construction as well as by measurement: phase 2's cap
`phase1_cost <= c_star + phase2_epsilon_eur` is a hard constraint in its own model, so any
incumbent it returns — however early — already respects the cost bound. Truncating it can only cost
smoothing, and measurably costs none. 5 s leaves ample headroom for the sites where phase 2 does
finish (EV-only: 0.08-0.83 s, Optimal).

**Result:** `bench_heater_solve_cost` total for the heater site falls from the **108.55 s** recorded
under GB-40 to **61.41 s** — phase 1's budget plus a bounded phase 2. The whole Rust suite also
halved, 117 s to 63 s.

**What this does *not* fix:** phase 1 still hits TimeLimit at ~57 s on every heater instance. That
is GB-40's standing problem and the remaining half. The two known levers there are already measured
in GB-40: a looser `mip_gap_target` (10 % put phase 1 on GapLimit for nine of ten instances at
+2.05 % mean cost) and the refuted reformulations. Phase 2 is no longer part of that problem.

**Open question worth answering:** *why* is phase 2 inert? Either phase 1's schedule is already
friction-optimal within the 0.17 EUR epsilon — plausible, since rescheduling a heater stage costs
more than epsilon allows — or the search cannot find the improvement. Raising `phase2_epsilon_eur`
and re-running `bench_phase2_changes_across_instances` distinguishes the two. If epsilon is the
binding constraint then phase 2 is not broken, it is starved, and the friction it is meant to remove
is simply unaffordable under the current cap.

**Production verification of the phase-2 budget, and two side effects (2026-09-30).** Measured from
`plan_history` before (29th 13:00-16:00Z) and after (30th 05:45Z+) the deploy:

| VEN | before median | after median | TIME_LIMIT before -> after |
|---|---|---|---|
| ven-2 (heater) | 64.0 s | **10.3 s** | 57 % -> 100 % |
| ven-3 (heater) | 72.2 s | **8.5 s** | 90 % -> 100 % |
| ven-1 (no heater) | 4.2 s | 4.6 s | 0 % -> **27 %** |

A 6-8x win on exactly the VENs that were hurting, with tight distributions (ven-2 min 9.8 s,
p90 10.4 s, n=11). It also corrects the model this work was reasoning from: live ven-2's phase 1 is
only ~5 s, so **phase 2 was consuming nearly all of its 64 s**, and the benchmark instance
(phase 1 at 57 s) is considerably harder than any real fleet VEN.

Two side effects, neither predicted:

1. **`TIME_LIMIT` is now the normal state for a heater VEN, which breaks it as a health signal.**
   Phase 2 always hits its 5 s cap, so `solve_status` reports TIME_LIMIT on ~100 % of cycles by
   design. GB-38 and GB-40 both use the TIME_LIMIT *rate* as the fleet's headline symptom, and that
   metric is now uninformative: it can no longer distinguish "phase 1 could not solve this site"
   (a real problem) from "phase 2 stopped at its intended budget" (normal). The status should
   carry the two phases separately — e.g. record phase-1 and phase-2 status independently on
   `Plan`, rather than one field that collapses them. Until then, treat TIME_LIMIT on a heater VEN
   as uninformative rather than as GB-40 evidence.
2. **Non-heater VENs lose some smoothing in their tail.** ven-1 went from 0 % to 27 % TIME_LIMIT:
   phase 2 used to converge there within 60 s and now the slowest quarter of its cycles are cut at
   5 s. The cost impact is bounded by `phase2_epsilon_eur` by construction, so this is lost
   friction smoothing rather than lost cost-optimality — but it is a real behaviour change the
   bench did not predict (an EV-only bench site finishes phase 2 in 0.08-0.83 s; production ven-1
   evidently carries more). If that smoothing turns out to matter, 10-15 s would still cut the
   heater VENs by ~4x while leaving non-heater sites room to converge.

**Phase-1 optimisation is parked; the 48 h horizon stays (2026-09-30).** Decided with the user.
Two reasons:

- **The 48 h span is a requirement, not an accident.** A receding-horizon controller needs lookahead
  well past the window it optimises, or end-of-horizon effects distort the near term — battery
  drained at the boundary, tank left cold, no preparation for the next morning's departure. The
  profiles say so explicitly (`plan_horizon_h: 48 # 2 solar windows for the 15.5 h-fill tank`). The
  executed-window comparison below is consistent with keeping it: 24 h and 48 h produce identical
  near-term behaviour, so the long horizon is not costing anything in decisions.
- **Phase 1 is only ~5 s on a live VEN.** ven-2's pre-deploy total was 64 s, of which phase 2 was
  ~57 s. The 57 s phase-1 figure that drove this investigation came from the *benchmark* instance
  (mip_gap 0.02, emergency-full heater), which is harder than any real fleet VEN — ven-2 runs
  mip_gap 0.06. Phase-1 work would therefore shave seconds off a solve that is no longer the
  problem.

If a real VEN ever does sit at the benchmark's difficulty, the measured options are recorded above:
coarsening the far zones keeps the 48 h span and bought 2.6x (192 vs 288 slots at the same span);
`mip_gap_target` 0.06 -> 0.10 has GB-40's 10-instance cost measurement behind it (+2.05 % mean); and
relaxing heater integrality in *far zones only* is the strongest untested idea but must be weighed
against GB-40's Arm 1, where decoupling heater power from the stage integer produced ~25 % cheaper
unphysical answers — confined to slots that never execute it may be acceptable, but it biases
lookahead optimistically, which can distort near-term deferral.

**The open item is phase 2's effectiveness, not its cost.** Its cost is now bounded at 5 s. What is
unexplained is why it changes nothing, and therefore whether even 5 s is worth spending or
`phase2_epsilon_eur = 0.0` (disable) is the honest setting.

**Methodological flaw found in this work's own bench:** it ran mip_gap 0.02 / epsilon 0.17 while
ven-2 runs 0.06 / 1.00. At 0.02 the bench's phase 1 *times out*, so its phase 2 inherits a poor
incumbent and a `c_star` derived from it — not the situation on a live VEN, whose phase 1 finishes.
Every phase-2 conclusion here was drawn on that unrepresentative configuration and is being re-run
across both gaps.

**Root cause of phase 1's time: horizon DURATION, not model size (2026-09-30).** Four controls,
all on the heater+EV site, phase 1 only, `bench_phase1_vs_horizon` /
`bench_phase1_flat_vs_priced_far_horizon` / `bench_phase1_count_vs_duration`:

| grid | slots | hours | phase 1 | status |
|---|---|---|---|---|
| 96 x 300 s | 96 | 8 | 0.39 s | GapLimit |
| **288 x 300 s** | **288** | **24** | **2.14 s** | GapLimit |
| **192 x 900 s** | **192** | **48** | **23.15 s** | GapLimit |
| 96x300 + 96x600 + 96x900 (**production**) | 288 | 48 | **60.03 s** | **TimeLimit** |

288 slots solve in 2 s over 24 h; 192 slots take 23 s over 48 h. **The cost scales with the span of
time modelled, not the number of integer decisions.** Production sits at 48 h and times out.

Three mechanisms were proposed and refuted along the way, each by its own control — worth recording
so they are not re-proposed:
- *EV comfort-band count* — merging equal-bid bands changed nothing (6406 -> 6752 ms).
- *Away-slot variables* — tightening them was inside cross-process variance, and the phase split
  later showed phase 1 spends 40 ms on the EV regardless.
- *Flat far-horizon pricing* (the GB-42 interaction) — 288 slots time out at 60 s whether the far
  half is flat-held or fully priced (60.03 s both ways). Pricing does improve plan *quality*
  (-5.08 -> -7.04 EUR) but not solve time.

Unverified hypothesis for *why* duration dominates: the tank cycles roughly every 100 min, so 48 h
holds about twice as many near-interchangeable thermostat cycles to coordinate as 24 h, and
interchangeable patterns are what stop branch-and-bound pruning. Not tested.

**It is waste, not harm — an earlier claim here is retracted.** `bench_does_the_far_horizon_harm_execution`
compares the part of the plan that actually runs before the next cycle replaces it:

| horizon | phase 1 | first-8 h cost | EV kWh (8 h) | heater switches (8 h) |
|---|---|---|---|---|
| 24 h | 2.05 s | 1.4994 EUR | 22.00 | 28 |
| 48 h (production) | 60.03 s | 1.4875 EUR | 22.00 | 28 |

Identical EV energy and switch count, and the 48 h plan's executed cost is marginally *lower*. So
the long horizon costs ~58 s per cycle and changes nothing the VEN carries out.

An interim claim that the 48 h horizon produced *worse* plans was wrong, and the reasoning behind it
was invalid: it argued a 48 h optimum could replicate a 24 h plan and then idle, making -7.04 vs
-9.98 EUR proof of suboptimality. The second day carries its own unavoidable base load and tank
losses, so the two objectives span different periods and cannot be compared that way.

**Candidate fix, not yet validated:** `plan_horizon_h` 48 -> 24 would buy ~28x on phase 1 for no
measured change in executed behaviour — the first change that addresses the fleet's actual timeout
rather than a neighbouring cost. Before proposing it as a default: repeat the executed-window
comparison across all ten `HEATER_VARIANTS` (one instance is not a result), and establish what the
far horizon is currently relied on for — EV deadlines beyond 24 h, VTN capacity obligations, and the
far-horizon zone's documented role in `VEN_ARCHITECTURE.md`.

**Still missing, and the reason this took a wrong turn first:** `plan_history` records
`solver_ms` but nothing about the *inputs*. A slow solve cannot be replayed. The targeted fix is
an input digest recorded when a solve exceeds a threshold — slot count, distinct tariff levels,
EV availability pattern and required energy, binary count — so the next occurrence is
reproducible offline instead of inferred from correlations.

**Where:** `VEN/src/controller/milp_planner/` (two-phase solve, `solver_timeout_s` default 60 s
per phase).

Solves of **20.8 s and 63.6 s** were recorded during E2E on 2026-09-27 on a host under load —
against a 60 s per-phase timeout, i.e. at and past the ceiling that produced GB-38 (three VENs
hitting TIME_LIMIT on essentially every solve for 24 h and never charging their EVs). GB-38 is
marked resolved for that specific run, but nothing has reduced the underlying solve cost, so
the same failure is one busy host away.

**To resolve:** measure where the time goes before tuning anything — heater tier binaries and
EV semi-continuous constraints are the usual suspects — then decide between a cheaper
formulation, a coarser far-horizon zone, or an honest raise of the timeout with a visible
TIME_LIMIT surface. Related: R-93 (a per-slot EV SoC variable would *add* variables, so it
must be costed against this).

