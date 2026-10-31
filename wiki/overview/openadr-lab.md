---
title: OpenADR Lab — System Overview
type: overview
created: 2026-07-04
updated: 2026-10-31
synced_commit: 102f7fa
sources: [docs/REQUIREMENTS.md, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/VTN_ARCHITECTURE.md, docs/history/project_journal.md]
tags: [overview, openadr, hems]
---

# OpenADR Lab — System Overview

A Raspberry Pi–hosted lab for **OpenADR 3 demand response** experimentation: one VTN stack and a standing
fleet of 20 VEN containers (ven-1..3 on Node1, ven-4..20 on Node2; [[deployment-topology]]), each VEN modelling a
residential site with a HEMS controller (docs/history/project_journal.md §Project Overview). Not
production-grade, but deliberately production-patterned (docs/REQUIREMENTS.md §1).

## The two sides

- **VTN side** — [[vtn-stack]]: the upstream `openleadr-rs` server (OAuth2 + OpenADR 3 REST,
  PostgreSQL), fronted by a BFF (one business credential) and a React operator UI whose Fleet page watches
  every site at once ([[fleet-monitoring]]) (docs/architecture/VTN_ARCHITECTURE.md §1–3).
- **VEN side** — a Rust/Axum application per site with two subsystems
  (docs/architecture/VEN_ARCHITECTURE.md §1):
  - the **HEMS Controller**: [[openadr-interface]] (30 s VTN poll), user-request manager,
    monitor/ledger, [[milp-planner]] (slow loop), [[dispatcher]] (1 s fast loop);
  - the **Simulator**: physics-based device models behind the [[asset-layer]] abstraction
    ([[simulator]]).

## What flows between them

The VTN publishes programs and typed events (`PRICE`, `GHG`, capacity limits, alerts —
see [[openadr-3]]); the VEN translates them into internal signals, replans with the
[[milp-planner]], dispatches setpoints to simulated assets (battery, EV, heater, PV,
base load), and reports telemetry back (`USAGE`, `DEMAND`, `STORAGE_CHARGE_LEVEL`, …)
(docs/architecture/VEN_ARCHITECTURE.md §2.1). The scenarios this must serve are catalogued
in [[system-use-cases]].

## Where things stand

The whole stack is deployed; every change reaches `main` through the Node3 runner, which runs the audits and the
four test suites ([[ci-pipeline]], [[testing-strategy]]). The VEN plans on a 3-tier variable-step grid
([[three-tier-plan-grid]]) or, as a comparison, dispatches without a plan ([[opportunistic-objective]]).
Direction and open ambitions live in [[vision-and-roadmap]].
