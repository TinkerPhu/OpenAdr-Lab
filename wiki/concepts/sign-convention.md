---
title: Sign Convention and Units
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/REQUIREMENTS.md]
tags: [domain, units, convention]
---

# Sign Convention and Units

The one convention every power value in the project obeys
(docs/REQUIREMENTS.md §2.5–2.6).

## Grid boundary

**Positive = imported from grid. Negative = exported to grid.** Applies uniformly at the
utility meter to setpoints, ledger entries, reports — everything crossing the
[[asset-layer]] interface.

`P_util` is a **single signed value**: the physical grid connection cannot import and
export simultaneously. `P_import` and `P_export` are not two measurements — they are the
same `P_util` conditioned on sign (exactly one non-zero at any instant). Within the site:
`Σ(P) = P_util − (P_consume + P_generate + P_store + P_release) = 0`. Generation and
battery discharge are negative *by definition*; they cause net export only when their
magnitude exceeds simultaneous consumption (§2.5).

## Units and naming

| Unit | Meaning |
|---|---|
| kW | power (instantaneous) |
| kWh | energy |
| €/kWh | **tariff** (energy price) |
| €/h | **rate** (power-based billing) — not the same thing as tariff |
| gCO₂eq/kWh | grid carbon intensity (GHG payloads) |
| % | state of charge |

Code rule (`.claude/CLAUDE.md` §naming): physical quantities carry the unit as suffix —
`power_kw`, `energy_kwh`, `tariff_eur_per_kwh`, `soc_pct`. The tariff/rate distinction and
its OpenADR event mapping continue in [[tariffs-and-capacity]].
