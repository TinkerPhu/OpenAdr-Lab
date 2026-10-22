# Wiki Index — OpenADR Lab

Catalog of all wiki pages. Updated on every ingest/sync. Rules: `CLAUDE.md` · scope: `purpose.md`.

## Overview
- [[openadr-lab]] — system-level summary: VTN stack, VEN HEMS, what flows between them
- [[vision-and-roadmap]] — swarm behaviour, certification readiness, 3.1 migration, upstream PRs

## Architecture
- [[ven-hexagonal-architecture]] — ring map, ports, enforced invariants
- [[vtn-stack]] — openleadr-rs 3.1, PostgreSQL, single-credential BFF, signal workbench, operator UI
- [[deployment-topology]] — Node1 docker stacks, port map, WSL dev environment, Node2 build/test-offload host (no local UI)
- [[testing-strategy]] — four suites, VEN test pyramid, test-first and determinism rules
- [[ci-pipeline]] — Node3 runner: handoff tickets, suite selection by path, multi-host lanes, deploy tickets, web board, merge gates
- [[fleet-monitoring]] — lab-mqtt fleet channel (telemetry/trace/plan/objective), per-VEN ACL, BFF fleet modules, Fleet page, lab-core crate

## Components
- [[milp-planner]] — two-phase HiGHS MILP on one shared model skeleton, assets declare their own part (no kind in the core), adoption gate, absolute replan grid with per-VEN phase, comfort curve, peak-demand penalty, file map
- [[dispatcher]] — build_setpoints on the 1 s sim tick, surplus-EV overlay, shiftable-load runtimes
- [[openadr-interface]] — wire-crate event types, PayloadReader units, wire rejection and granted-scope notice, one capacity schedule and interval-timing rule, change detection and replan triggers, obligation-only accumulated reports declaring their units
- [[simulator]] — roster + tick (`SimState`, `TickInputs`, per-asset feeds), site meter derivation, ports behind `SimHandle`, persistence, `/sim/inject` behaviour classes (double-option null-clear, loud logging), asset_simulation.md drift
- [[asset-layer]] — one `Asset` trait with declared kind/lever/rank (no config enum), capability traits, site meter is not an asset, PV generation-limit sources and per-array weather/snow, heater thermostat deadband + planner pins, EV schedule, determinism
- [[ven-ui]] — React SPA, multi-VEN context, timeline with now-point, History page (last-24h default, forecast-accuracy overlay), shared chart-kit (TimeSeriesChart/StackedTimeSeriesChart/CurveChart), nullable-slider convention
- [[reliability-and-config]] — supervised background tasks + `/tasks/status`, `DomainError` variants and the `routes/error.rs` mapping, poll backoff, boot-time profile validation, config knobs (profile vs env)
- [[history-store]] — HistoryPort + SqliteHistoryStore (schema v12), 1-minute sampler, forecast-accuracy and plan-quality history, EV plugged/curtailment-source rows, BFF recorder (single credential, health status, fleet telemetry link), SettingsPort
- [[fleet-tooling]] — fleet.sh (up/down/status), bulk VEN registration, GB-06/GB-09
- [[experiment-harness]] — scenario YAMLs, real-time runner, KPI extraction, comparison reports
- [[notifications]] — user notification feed: ring + SSE + persistence, edge-triggered producers, dedup_key rolling-window collapse + history viewer
- [[heuristics-pipeline]] — base-load learner: seven weekday buckets shrunk toward the overall mean, EWMA window, profile knobs, daily job, asset-owned forecast, forecast accuracy, BASELINE reports
- [[opportunistic-objective]] — non-planning dispatch on the same site: one `opportunistic_step` live and as forecast, for planner-vs-baseline comparison
- [[deviation-arbiter]] — two passes on one lever machinery (deviation correction, limit enforcement), per-asset levers, tick order and dispatch stages, held-correction release, lag bridge, residual escalation to replan, `/arbiter-settings` and diagnostics
- [[weather-forecast]] — MQTT-ingested external forecast → physics-based PV generation, wired into both the planner and the live simulator's PV ground truth; source-liveness E2E coverage (R-56)
- [[real-measurement-mqtt]] — real PV/baseline-load meter readings over MQTT (ven-1 only), 3-tier PV precedence (measured > weather > sin), two-gate (env var + profile) enablement, live-tick-only (never feeds the planner)

## Concepts
- [[openadr-3]] — protocol entities, event types, certification profiles, 3.0 vs 3.1 skew
- [[openadr-programs]] — commercial DR offering: 5 worked examples, 4 purposes (interpretation/auth/business/discovery), customer visibility, enrollment
- [[demand-response]] — actors (utility/DSO/TSO/aggregator/prosumer), DER, baseline vs forecast, M&V
- [[hems-planning]] — two-speed loop, FIRM/FLEXIBLE slots, user requests, sessions as constraints, comfort curve now carried onto sessions (BL-34), unified /user-requests supersedes per-device CRUD routes (BL-41)
- [[three-tier-plan-grid]] — zones A/B/C, alignment rule, the three "nows"
- [[sign-convention]] — grid-boundary signs, units, unit-suffix naming
- [[tariffs-and-capacity]] — TariffSnapshot, capacity limits vs capacity state, stale-rate fallback
- [[openadr-security]] — OAuth2 scopes, object privacy/targeting, TLS/webhook requirements
- [[wire-contract]] — declared units/descriptors on every payload, `PayloadReader`, profile stricter than the spec
- [[wiki-maintenance]] — how this wiki stays current (sync/ingest/query/lint workflow)

## Use Cases
- [[system-use-cases]] — DR scenario catalogue mapped to lab signals and BDD coverage
- [[openadr-spec-use-cases]] — spec-implied VEN use cases, gap-checked (✅/🟡/❌) against the code

## Decisions
- [[milp-over-greedy]] — why the planner uses a two-phase MILP instead of greedy scheduling
- [[hexagonal-refactoring]] — spec series 015–029, ports for testability and swappability
- [[dto-pass-through]] — OpenADR spec field names pass through all layers unnormalised
- [[superpowers-not-adopted]] — agentic framework evaluated 2026-06-25; only the worktree-per-feature pattern borrowed
- [[docker-host-lease-lock]] — lease lock on Node1 (and, via wsl_lock.sh, the shared WSL instance) serializes parallel sessions' builds/tests; why not a queue file

## Sources
- [[power-envelope-forecast-basis]] — **open/unfinished**: summary of `docs/external_research/power-envelope-forecast-basis.md`, a two-round sourced web-research thread on why Dynamic Operating Envelope forecasts vary hour-by-hour, how DNSPs (SA Power Networks, Energex) actually compute them, and a critique of a proposed static equal-share alternative

## Queries
- [[device-session-common-interface]] — why EvSession/HeaterTarget/ShiftableLoad stay separate structs, not one trait; BL-41 update
- [[ven-code-vs-docs-audit]] — full VEN/src read vs docs: what matches, 10 confirmed drifts, ranked refactoring candidates
- [[openadr-programs-explained]] — what a Program is, 5 worked examples (VTN vs VEN view), multi-program enrollment, contractual out-of-band joining
- [[distributor-business-case-tiers]] — arguments for a distributor to adopt VTN/VEN: 4 rollout tiers from open tariff publication to VPP dispatch
- [[history-store-persistence-format]] — SQLite schema (v12: plan_snapshots dropped R-63, forecast_accuracy_samples, plan_history, capacity/headroom and plugged columns), epoch-second time encoding, per-asset-per-minute rows, flat retention pruning, docker bind-mount path
- [[planner-tab-purpose]] — what the Planner tab is for (user vs. debug view), keep-don't-dismantle verdict, improvements filed as BL-36..38
- [[dso-retailer-unbundled-tariff-coordination]] — legally-unbundled DSO/retailer tariff coordination: multi-program protocol support, why an unbounded virtual DSO price is dishonest (shadows the real tariff), DLMP shadow-price duality with the envelope, regulator revenue-cap as the actual answer to "who sets the relation"

---
Pages: 48 · Last sync: 2026-10-22 (scoped: ven-ui, milp-planner, asset-layer, deviation-arbiter, openadr-interface, history-store, simulator, reliability-and-config, heuristics-pipeline re-verified; other pages see review.md).
