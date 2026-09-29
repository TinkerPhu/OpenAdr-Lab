# Technical Debts Register

> **Next ID: R-98.** Use this number for the next new item filed, then increment this
> line to R-99. (Corrected 2026-09-27: the line still said R-86 while R-87..R-97 were
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

## Cross-register priority view (2026-09-27)

One ordered list across **both** registers — this file (`R-*`) and `docs/BACKLOG.md` (`GB-*`) —
because the two rate things differently: debts carry a priority emoji, while the backlog's last
column holds effort, not severity. Ratings for `R-*` items that already had one are reproduced
unchanged; `GB-*` severities are assigned here.

**This is an index, not a second source of truth.** Each row points at the item; the item owns
its detail. Re-rate in the item, then here.

| Severity | ID | Goal it blocks | One line |
|---|---|---|---|
| 🔴 High | GB-41 | reliable system | **Re-scoped 2026-09-27**: mechanism reproduced offline (comfort-rate core reward below tariff + 0.22 import malus, plus `MayRun`'s all-or-nothing core). Symptom is bypassed on the fleet by `usage_forecast`'s `MustRun`, not fixed — any user soft-deadline request still takes the old path. Remaining: confirm the campaign's `comfort_rates` and decide the product fix. |
| 🔴 High | GB-50 | VTN stimulus | No quantity/unit contract on the wire: W vs kW contradictions, `USAGE` treated as power where the spec says energy, no `payloadDescriptors`, and a `* duration` fudge in `kpi.py` compensating. Every VTN-facing number and every KPI rests on this. |
| 🟠 Med-High | R-97 | reliable system | Solves of 20.8 s and 63.6 s against a 60 s per-phase timeout — the mechanism behind GB-38 is one busy host away from recurring. |
| 🟠 Med-High | R-76 | VTN stimulus | Reservation-capacity reports likely carry swapped or conceptually wrong values, silently; self-consistent tests give no signal. |
| 🟠 Med-High | GB-42 | VTN stimulus | The **default** stale-rate policy is a stub behaving as `LastKnown`: on a 48 h horizon with ~24 h of rates, half the plan is priced at one repeated number. |
| 🟠 Med-High | R-93 | VTN stimulus | No per-slot EV SoC variable: the planner cannot reason about SoC mid-horizon, so no post-return recharge planning. Structural blocker under R-92. |
| 🟡 Medium | R-86 | VTN stimulus | `randomizeStart` parsed but ignored — the whole fleet responds on the same instant, which is what the field exists to prevent. |
| 🟡 Medium | R-21 | reliable system | `cargo test` heap corruption around the HiGHS tests: undermines the instrument every other conclusion rests on. |
| 🟡 Medium | R-96 | reliable system | The capacity-limit E2E step waits for a freshly *adopted* plan; cost two 65-minute runs on 2026-09-27 with failures that looked like product defects. |
| 🟡 Medium | R-87 | VTN stimulus | Stricter than the 3.1 schema on `intervalPeriod.start` — the one failure a conformance lab must not have. |
| 🟡 Medium | R-92 | VTN stimulus | Only the next departure gets a charging goal, while availability is per-slot. |
| 🟡 Medium | R-94 | transparent UI | The MILP does not model the heater deadband, so the asset can refuse planned dispatch with nothing on screen saying why. |
| 🟡 Medium | R-85 | VTN stimulus | Two measurement-report builders with diverging behaviour; resolution already decided, not yet done. |
| 🟡 Medium | GB-46 | VTN stimulus | Missing VEN-side instrumentation (tariff source event id, effective-limit column), so compliance cannot be proven from the data without harness reconstruction. |
| 🟡 Medium | GB-52 | reliable system | The E2E broker runs anonymous while production requires credentials — the test bed does not exercise the auth path. |
| 🔵 Low | R-74 | transparent UI | The Dashboard Simulation card hardcodes ev/heater/pv; battery, base load and shiftable loads are invisible there. Small, and the generic pattern already exists in `Controller.tsx`. |
| 🔵 Low | R-77, R-90, R-89, R-33, R-71, R-36, R-38, R-39, R-44, R-47, R-48, R-66, R-73, R-91, R-95 | — | Hygiene, naming, dead code, coverage gaps and future-proofing. Do opportunistically, per this file's own "refactor first if Trivial/Small" rule. |
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
| R-85 | Two measurement-report builders with diverging behaviour: the timer path (`reporter.rs::build_measurement_report`, driven by `tasks/sim_tick/publish.rs` from `grid.net_power_w`) and the obligation path (`build_measurement_report_for_obligation`, from `report_intervals.rs::build_net_site_power_ts`). The timer path omits `intervalPeriod`, sends `STORAGE_CHARGE_LEVEL` as a string and `SIMPLE` as a constant 1.0. Resolution decided (fleet-monitor phase 0 D-5): the timer path is deleted once the standing monitoring event exists, leaving the obligation path as the only builder. | `VEN/src/controller/reporter.rs`, `VEN/src/controller/report_intervals.rs`, `VEN/src/tasks/sim_tick/publish.rs` | Medium | Low | 🟡 | Medium — one net-power derivation for reports and fleet telemetry |
| R-76 | `reporter.rs`'s VTN report-payload mapping for `IMPORT_RESERVATION_CAPACITY`/`EXPORT_RESERVATION_CAPACITY` reads `SiteFlexibilityEnvelope.up_kw` for the *Import* payload and `.down_kw` for the *Export* payload — backwards by the struct's own field doc comments (`up_kw`: ability to *reduce* consumption/export more; `down_kw`: ability to *increase* consumption/import more), so the mapping looks swapped. The OpenADR 3.1 spec itself (`docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md`, line 1367) defines these payload types as "Amount of additional import/export capacity **requested**" — a VEN-initiated capacity request, a different concept than "live headroom available right now" that `SiteFlexibilityEnvelope` represents; it's not established whether re-purposing this struct's fields for these payloads was ever correct, independent of the swap question. Found while investigating a site-headroom UI bug (2026-09-07) — flagged, not fixed: touches VTN-facing wire behavior and needs its own OpenADR-spec-literate investigation before changing, not a byproduct of an unrelated fix. Scoping decision (`asset-competence-audit`, Phase 0 of the now-completed/deleted asset-competence-assurance master plan — see `docs/history/project_journal.md`'s 2026-09-10 Phase 0 entry, 2026-09-10): this is a site-level aggregate-interpretation question, not a case of one asset's own state/forecast being second-guessed by another module — it doesn't have a single owning asset to consolidate authority into, so it stayed a separate, independent investigation rather than becoming a phase of that master plan. | `VEN/src/controller/reporter.rs` (`IMPORT_RESERVATION_CAPACITY`/`EXPORT_RESERVATION_CAPACITY` arms) | Small | Medium (a real VTN report likely carries backwards or conceptually-wrong values today, silently — self-consistent tests give no signal) | 🟠 | Medium — protocol-correctness risk in a live report, not yet confirmed either way |
| R-77 | Project-wide "envelope" naming audit, deferred from the 2026-09-07 site-headroom fix (`naming-envelope-vs-headroom` in `.claude/CLAUDE.md`): a grep found ~48 `VEN/src` files referencing "envelope"/"Envelope". This session renamed only the two files it was already touching (`envelope.rs`→`site_headroom.rs`, `capacity_envelope.rs`→`capacity_headroom.rs`). Remaining candidates needing individual triage (not a blanket rename): `entities::plan::FlexibilityEnvelope` (per-device-session envelope, built by `milp_planner/envelopes.rs::build_plan_envelopes`) and `SiteFlexibilityEnvelope`/`SiteFlexibilitySample` (this fix's own structs, kept as-is since renaming a `Serialize`d struct mirrored by a TypeScript type in `VEN/ui/src/api/types.ts` is a larger, separately-reviewable change) are likely internal-HEMS concepts that should rename to "headroom"; `entities/capacity.rs`'s "Dynamic Operating Envelope" and `reporter.rs`'s reservation-capacity handling are genuine OpenADR-boundary uses that should keep "envelope"; `milp_planner/envelopes.rs`'s own mathematical scheduling-constraint sense of "envelope" is ambiguous and needs its own judgment call. | `VEN/src` (~48 files, see grep for `envelope\|Envelope`) | Medium (many small renames, no logic changes) | Low (naming-only, mechanical once triaged) | 🟡 | Low-Medium — code-comprehension/consistency gain, not a correctness fix |
| R-21 | `cargo test` intermittently crashes with heap corruption (SIGABRT, varying malloc messages) around the two heaviest HiGHS tests (`run_planner_n48_full_horizon`, `solve_ven3_heater_three_tier_zones_feasible`). Same tests pass clean in isolation every time; also crashes with `--test-threads=1`, so it is allocator/heap-state-dependent in the native HiGHS library, not a plain data race. Test-infra only — no production path. Workaround: run the affected module in isolation when the full suite crashes. | `VEN/src/controller/milp_planner/` (HiGHS FFI via `good_lp`), test harness only | Medium | Low (flake) | 🟡 | Medium — CI/test-suite trust, no production impact |
| R-33 | UI test gaps: `VTN/ui/src/pages/Metrics.tsx` is the only untested page in either UI; `JsonDialog.tsx` is byte-identical in both UIs (50 lines — accept the copy with a twin-note header, or fold into a shared package if one materializes). | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | 🟡 | Low — test-coverage gap |

## Low priority (🔵) — by topic

### Architecture & type placement

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-39 | `state/mod.rs` mixes app wiring (`AppState`) with domain-ish value types (`EvSettings`, `HemsState`). Decide whether the two value types move to entities/ (as `AssetLedgerEntry` did) or stay — record the conclusion either way. | `VEN/src/state/mod.rs` | Trivial | Mechanical | Low — architecture clarity, no behavior change |
| R-47 | `AppState` keeps accumulating flat diagnostic fields (VTN connection status, storage-ok flag, per-task status map, etc.) added ad hoc per WP (T1/T3). No grouping/namespacing, so it will keep growing linearly with every future observability WP. Consider a `diagnostics: DiagnosticsState` sub-struct. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/mod.rs` | Small | Low | Low-Medium — prevents compounding maintenance debt on every future observability WP |

### Code & repo hygiene

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-27 | Hard-coded tuning constants: task intervals (`state_persist.rs:8` 15 s, `progress_ticker.rs:15` 1 s). Name them and/or expose via config/PlannerParams. **The MILP-tolerance half is discharged (2026-08-28)**: `with_mip_gap` at all three solve sites now reads the per-profile `planner.mip_gap_target` (default 0.02) carried on `MilpInputs`, and the `MIP_GAP_TARGET` constant is gone. It was briefly configurable in August and reverted (5b8923c3) for line-count reasons rather than on its merits; restored here once `PlannerConfig` moved to its own module and the quality cost was actually measured (GB-40). | tasks/ | Trivial | Low | Low — config flexibility only |
| R-36 | Lint/doc hygiene bundle: (a) module-wide `#![allow(dead_code)]` without justification in `entities/capacity.rs:5`, `entities/design_vocabulary.rs:7`; (b) 12 eslint warnings (exhaustive-deps, mixed exports); (c) ~~eslint lints the generated `VTN/ui/coverage/` dir~~ — fixed 2026-08-05: turned out worse than lint noise, the whole generated dir (27 files, 423K) was actually committed to git because `VTN/ui/.gitignore` was missing the `coverage/` line `VEN/ui/.gitignore` already had; untracked and added; (d) `solve_ven3_heater_three_tier_zones_feasible` runs >60 s in debug `cargo test` — consider a smaller horizon variant; (e) "Stage 5 —" phase labels in `entities/user_request.rs` / `controller/user_request.rs` doc comments — drop the prefixes. | entities/, VEN/ui, VTN/ui, milp_planner/tests | Small | Low | Low — mostly cosmetic; (d) has a small developer-friction upside (faster test runs) |
| R-71 | `#[allow(...)]` without a same-line justification (this repo's own linting rule requires one), found in the assets/simulator area: `assets/mod.rs:350`, `assets/ev_milp.rs:226`, `assets/heater.rs:138`, `simulator/mod.rs:181`, `simulator/plan_context.rs:59`, `simulator/tests.rs:272`, plus `assets/asset_trait.rs:18,32,121` (justified only by a paragraph above the attribute, not on its line). The pattern works correctly right next door in `assets/heater_milp.rs:172,229` and `controller/simulator_port.rs:118,131`, which do have same-line justifications — so the convention is known, just inconsistently applied. A repo-wide grep for just `too_many_arguments` (excluding tests) finds 32 instances total, so this is a fraction of a wider pattern. Found during 2026-09-03 architectural audit. Two of the unjustified sites (`build_milp_context`'s 14 params, `tick`'s 19 params) are candidates for either a same-line justification or a params-struct refactor. | `VEN/src/assets/`, `VEN/src/simulator/` (listed above) | Trivial | Low | None — pure hygiene, mechanical |
| R-38 | (a) `VEN/Cargo.toml` carries blueprint-era comments (commented-out `openleadr-client` etc.); (b) verify `VTN/data/db` (runtime artifact) is gitignored. | `VEN/Cargo.toml`, `VTN/data/` | Trivial | Low | None — pure hygiene |
| R-44 | `/health` handler (`routes/system.rs::health`) deep-clones the full `VtnConnectionStatus` and active `Plan` on every poll just to read a couple of fields. Cheap today but grows with `Plan` size; consider a narrower state accessor. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/system.rs` | Trivial | Low | Low — cheap today, future-proofing only |
| R-73 | **Partially resolved (ev-usage-forecast, 2026-09-26): `EvCharger::soc_trajectory` is consolidated away — `controller/milp_planner/asset_port.rs::ev_soc_trajectory` is now the single EV SoC-trajectory integrator, and it gained the exogenous-drop input that change needed.** Still open for the other three pairs: `Battery::future_state_values`, `EvCharger::future_state_values_at`, `Heater::future_state_values` are never called — `asset_port.rs` has separate, actually-used "Mirrors X" reimplementations (`battery_future_state`, `ev_future_state_at`, `heater_future_state`). Confirmed pre-existing via `git stash` + grep. Currently `#[allow(dead_code)]`'d with a same-line note pointing here. Fix: either delete the remaining dead methods, or delete `asset_port.rs`'s duplicates and make callers use the trait methods directly (diff them before choosing — the EV one had not drifted). | `VEN/src/assets/battery.rs`, `VEN/src/assets/ev.rs`, `VEN/src/assets/heater.rs`, `VEN/src/controller/milp_planner/asset_port.rs` | Trivial | Low | Low — dead code plus a possible silent duplication/drift risk between the two implementations |
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
| R-40 | File-size near-cap watch (production lines, 2026-07-16): `simulator/mod.rs` 470/500, `milp_planner/results.rs` 415/500, `tasks/poll_events.rs` 162/200, `tasks/planning.rs` ~198/200. Split proactively when next touched; `scripts/audit_file_sizes.py` is the authority. (`state/mod.rs` crossed the cap 2026-08-10 while adding the capacity-limit envelope and was split — its tariff/capacity/alert/SIMPLE/dispatch-window `AppState` accessors moved to `state/grid_signals.rs`, following the existing `state/obligations.rs`/`state/arbiter.rs` split-impl pattern. `services/planning.rs` crossed the cap 2026-08-23 during R-29's `solve_plan` panic-fallback fix and was split the same way — its `impl PlanningService` block moved to `services/planning/service.rs`. `tasks/sim_tick/tick.rs` sits exactly at the 200/200 cap as of R-59 — two lines were hoisted into `tasks/sim_tick/context.rs` to make room for the new comms-loss params; any future addition to the tick pipeline needs a consolidation pass or a split before it can land. `profile/schema.rs` sits at 499/500 as of GB-40's `mip_gap_target` field, which briefly pushed it to 507 until its doc comment was trimmed — the next field added to any config struct there needs a split first, most naturally `PlannerConfig` into its own module.) | N/A — monitoring only, not an actionable fix until a cap is actually crossed |

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

## R-92 — `engage_charge_planning` can only ever target one departure per solve

**Severity: 🟡 Medium** · Effort Medium · Risk Medium · Gain Medium — the fleet plans around
the next trip only, so a second departure inside the same horizon gets availability but no
urgency. Blocked in practice by R-93.

**Where:** `VEN/src/assets/ev_usage_forecast.rs::target_next_predicted_departure`,
`EvMilpContext.t_dead_step`/`e_core_kwh` (`controller/milp_planner/asset_port.rs`),
introduced by `ev-usage-forecast` (design.md Non-Goals).

`usage_forecast` gives the planner truthful per-slot availability for *every* trip inside the
horizon — a 30–48 h horizon routinely contains two departures — but the charging *goal* is
still a single scalar pair (`t_dead_step`, `e_core_kwh`), so only the next departure is
targeted. The plan is correct (it never charges while the car is away) but not urgent about
the second trip: a profile that leaves twice in one horizon gets no pre-charge pressure for
the later one until a replan brings it within "next".

**Why it is debt rather than a bug:** the scalar deadline is pre-existing — `usage_sim`'s
`engage_charge_planning` and every real user session have the same single-deadline shape, so
nothing regressed. It only became *visible* here, because availability is now per-slot while
the goal is not.

**To resolve:** generalize the pair into a list of (deadline step, core energy) obligations in
`EvScalars`/`MilpInputs` and make the EV constraint set emit one cumulative-energy constraint
per obligation instead of one. Materially larger than this change; the availability side needs
no work, it already handles N trips.

## R-93 — the EV MILP still has no per-slot SoC variable

**Severity: 🟠 Medium-High** · Effort Medium-Large · Risk Medium · Gain Medium-High — the
structural blocker under R-92 and under `clamp_core_to_reachable_energy`: the solver cannot
reason about SoC mid-horizon, so it cannot plan a post-return recharge at all. Matters most
when a VTN event lands near an away window.

**Where:** `VEN/src/assets/ev_milp.rs` (constraints are total-energy),
`controller/milp_planner/asset_port.rs::ev_soc_trajectory` (post-solve projection),
surfaced by `ev-usage-forecast` (design.md Decision 3a, Non-Goals).

The EV is modelled as total energy delivered before a deadline; there is no per-slot SoC
variable and no SoC-balance constraint, so the projected SoC curve — including
`ev-usage-forecast`'s trip drops — is reconstructed *after* the solve. The solver therefore
cannot reason about SoC mid-horizon: it cannot plan the recharge that a predicted *return*
makes possible, only avoid charging while the car is away.

**Why it is debt rather than a bug:** every shipped behaviour is correct with the projection
approach; what is missing is planning capability, not correctness. The clamp in
`clamp_core_to_reachable_energy` exists because of this shape too — with a real SoC balance
plus slack the shortfall would be expressible in the model instead of pre-clamped.

**To resolve:** add `soc_ev[t]` variables with a balance constraint (charge minus exogenous
drop, floored at `min_soc`), make the deadline obligation a bound on `soc_ev[t_dead]`, and
delete the post-solve reconstruction. Pairs naturally with R-92 — both are the same
"generalize the EV model" work.

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

## R-96 — the capacity-limit E2E step requires a freshly *adopted* plan

**Severity: 🟡 Medium** · Effort Small · Risk Low · Gain Medium — costs whole 65-minute runs
and produces failures that look like product defects.

**Where:** `tests/features/steps/uc_steps.py:14`
(`I wait for the VEN /plan to have slots with import_cap_kw at most {cap}`), used by UC-10b
(`ven_uc_edge_cases.feature:59`) and UC-12b (`ven_uc_stress.feature:44`).

The step polls for a plan that both satisfies the cap **and** was created after the limit was
sent. Plan adoption is deliberately sticky (adoption threshold, decay, switch penalty) and the
solve itself can take 20-60 s, so a plan that already satisfies the cap — because a previous
scenario set the same or a tighter one, or because the new plan was not better enough to adopt
— can leave the poll waiting 300 s for a plan that has no reason to be recomputed. Observed
twice on 2026-09-27, each run failing a *different* one of the two scenarios, both times with
the final plan carrying the correct `import_cap_kw` in every slot; a third run passed both.

**To resolve:** make the scenario force a replan (a trigger the VEN cannot ignore) rather than
waiting for one, or have the step accept a plan that satisfies the cap when the cap was already
in effect before the scenario started. Changing the assertion to drop the freshness requirement
outright would weaken a real guarantee — that the limit reached the planner — so pick one of the
two above instead.

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

