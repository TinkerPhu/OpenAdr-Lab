---
title: Demand Response — Domain Primer
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/REQUIREMENTS.md]
tags: [domain, dr, energy]
---

# Demand Response — Domain Primer

The energy-market context for [[openadr-lab]], distilled from the project glossary
(docs/REQUIREMENTS.md §2 — the single source of truth for vocabulary).

## Actors

**Utility** (runs DR programs), **DSO** (distribution grid, e.g. EWZ, Enedis),
**TSO** (transmission grid, e.g. Swissgrid, TenneT), **Aggregator** (bundles many small
DER portfolios into market-sized flexibility), **Prosumer** (consumes *and* produces —
the household this lab's VENs model) (§2.1).

## Core ideas

- **DR (Demand Response)**: consumers shift/reduce load in response to grid signals —
  prices, curtailment, emergencies (§2.4). Signals travel over [[openadr-3]].
- **DER**: any small-scale generation/storage at the edge — PV, battery, EV charger,
  controllable loads (§2.4); modelled here by the [[asset-layer]].
- **Flexibility**: the power range a site can shift per time slot, published as
  `FlexibilityEnvelope` so aggregators can predict available DR capacity (§2.3).
- **Baseline vs Forecast** — deliberately distinct (§2.3): the *Baseline* is expected
  consumption **without** DR intervention, used in **M&V** to verify delivered reduction;
  the *Forecast* is the planner's forward-looking per-slot prediction. Conflating them
  breaks M&V.
- **Tariff vs rate** (§2.4): tariff = €/kWh (energy), rate = €/h (power over time).
  Everywhere in this project "tariff" means €/kWh — see [[tariffs-and-capacity]].

How a household turns these signals into action is the subject of [[hems-planning]].
