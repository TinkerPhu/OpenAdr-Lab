# Wiki Index — OpenADR Lab

Catalog of all wiki pages. Updated on every ingest/sync. Rules: `CLAUDE.md` · scope: `purpose.md`.

## Overview
- [[openadr-lab]] — system-level summary: VTN stack, VEN HEMS, what flows between them
- [[vision-and-roadmap]] — swarm behaviour, certification readiness, 3.1 migration, upstream PRs

## Architecture
- [[ven-hexagonal-architecture]] — ring map, ports, enforced invariants
- [[vtn-stack]] — openleadr-rs, PostgreSQL, dual-credential BFF, operator UI
- [[deployment-topology]] — Node1 docker stacks, port map, WSL dev environment, Node2 build/test-offload host (no local UI)
- [[testing-strategy]] — four suites, VEN test pyramid, test-first and determinism rules

## Components
- [[milp-planner]] — two-phase HiGHS MILP, adoption gate, heater anchor, cross-asset interactions, session comfort curve (BL-34), peak-demand penalty threshold (BL-09), file map
- [[dispatcher]] — build_setpoints on the 1 s sim tick, surplus-EV overlay, shiftable-load runtimes
- [[openadr-interface]] — event→signal translation tables, report obligations, poll_events/detect.rs split, events received now recorded to history (R-64)
- [[simulator]] — physics models behind SimulatorPort, /sim endpoints (UI only), injectable clock/seedable RNG (R-24), sim-inject double-option null-clear fix, sim_inject_enabled gate + call-source logging + SIGTERM persist fix (R-65)
- [[asset-layer]] — Asset trait + AssetConfig dispatch (R-08 macro forwarder + file split), history ring buffers, AssetMilpContext, PV generation_limit_kw rename + manual override + rated_kw/inverter_max_kw fix, site_meter.rs deleted (R-62)
- [[ven-ui]] — React SPA, multi-VEN context, timeline with now-point, History page (last-24h default, forecast-accuracy overlay), shared chart-kit (TimeSeriesChart/StackedTimeSeriesChart/CurveChart), nullable-slider convention
- [[reliability-and-config]] — task supervision, typed DomainError, profile validation, config knobs, EvSessionService removed (BL-41)
- [[history-store]] — VEN HistoryPort/SqliteHistoryStore + sampler + routes, VTN BFF recorder, forecast-accuracy tracking (schema v8), plan_snapshots removed (R-63)
- [[fleet-tooling]] — fleet.sh (up/down/status), bulk VEN registration, GB-06/GB-09
- [[experiment-harness]] — scenario YAMLs, real-time runner, KPI extraction, comparison reports
- [[notifications]] — user notification feed: ring + SSE + persistence, edge-triggered producers, dedup_key rolling-window collapse + history viewer
- [[heuristics-pipeline]] — learned base-load baselines: trapezoid appliance noise, weekday/weekend EWMA profiles, planner/forecast consumers
- [[deviation-arbiter]] — single real-time reconciler: marginal-cost lever ranking, residual escalation to replan, replaces the twice-removed absorber + opportunistic overlay; battery/EV runaway-correction bug found and fixed, `/arbiter-diagnostics` readout added
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
- [[history-store-persistence-format]] — SQLite schema (now v8: plan_snapshots dropped R-63, forecast_accuracy_samples added), epoch-second time encoding, per-asset-per-minute rows, flat retention pruning, docker bind-mount path
- [[planner-tab-purpose]] — what the Planner tab is for (user vs. debug view), keep-don't-dismantle verdict, improvements filed as BL-36..38
- [[dso-retailer-unbundled-tariff-coordination]] — legally-unbundled DSO/retailer tariff coordination: multi-program protocol support, why an unbounded virtual DSO price is dishonest (shadows the real tariff), DLMP shadow-price duality with the envelope, regulator revenue-cap as the actual answer to "who sets the relation"

---
Pages: 44 · Last sync: 2026-08-11 (9d2a538: scoped to this change's own impact — history-envelope-persistence's capacity-limit-schedule persistence work — rather than the full 093fbd1..HEAD range, which holds a large unrelated backlog already tracked in review.md's staged batches. New source page power-envelope-forecast-basis filed from docs/external_research/ (new ingest-input directory, first page of this type), explicitly marked open/unfinished. Updated: tariffs-and-capacity (new section distinguishing parse_capacity_state's collapsed scalar from parse_capacity_schedule's full schedule + its history persistence, linking to the new source page). Preceded same day by 2026-08-09's 093fbd1..329444a sync (83 commits: BL-41 Device Sessions API removal, R-62/63/64/65 dead-code cleanup, chart-primitives refactor, forecast-accuracy-tracking schema v8, ven-1 PV-injection root cause fixed — 16 pages updated in place, no new pages; full detail in log.md).
