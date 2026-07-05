# Wiki Index — OpenADR Lab

Catalog of all wiki pages. Updated on every ingest/sync. Rules: `CLAUDE.md` · scope: `purpose.md`.

## Overview
- [[openadr-lab]] — system-level summary: VTN stack, VEN HEMS, what flows between them
- [[vision-and-roadmap]] — swarm behaviour, certification readiness, 3.1 migration, upstream PRs

## Architecture
- [[ven-hexagonal-architecture]] — ring map, ports, enforced invariants
- [[vtn-stack]] — openleadr-rs, PostgreSQL, dual-credential BFF, operator UI
- [[deployment-topology]] — Pi4 docker stacks, port map, WSL dev environment
- [[testing-strategy]] — four suites, VEN test pyramid, test-first and determinism rules

## Components
- [[milp-planner]] — two-phase HiGHS MILP, adoption gate, heater anchor, cross-asset interactions, file map
- [[dispatcher]] — build_setpoints on the 1 s sim tick, surplus-EV overlay, shiftable-load runtimes
- [[openadr-interface]] — event→signal translation tables, report obligations
- [[simulator]] — physics models behind SimulatorPort, /sim endpoints (UI only)
- [[asset-layer]] — Asset trait + AssetConfig dispatch, history ring buffers, AssetMilpContext
- [[ven-ui]] — React SPA, multi-VEN context, timeline with now-point
- [[reliability-and-config]] — task supervision, typed DomainError, profile validation, config knobs

## Concepts
- [[openadr-3]] — protocol entities, event types, certification profiles, 3.0 vs 3.1 skew
- [[demand-response]] — actors (utility/DSO/TSO/aggregator/prosumer), DER, baseline vs forecast, M&V
- [[hems-planning]] — two-speed loop, FIRM/FLEXIBLE slots, user requests, sessions as constraints
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

## Sources
_none yet — seed pages cite repo files directly; per-document summary pages are created by `/wiki-ingest`_

## Queries
- [[device-session-common-interface]] — why EvSession/HeaterTarget/ShiftableLoad stay separate structs, not one trait
- [[ven-code-vs-docs-audit]] — full VEN/src read vs docs: what matches, 10 confirmed drifts, ranked refactoring candidates

---
Pages: 28 · Last sync: 2026-07-05 (VEN code deep-ingest at e138861: audit page + 8 pages corrected against source)
