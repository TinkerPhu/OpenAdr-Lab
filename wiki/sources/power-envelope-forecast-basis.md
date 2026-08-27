---
title: "Source: Power Envelope Forecast Basis (external research)"
type: source
created: 2026-08-11
updated: 2026-08-11
synced_commit: 75e199d
sources: [docs/external_research/power-envelope-forecast-basis.md]
tags: [capacity, envelope, doe, dso, external-research, open]
---

# Source: Power Envelope Forecast Basis (external research)

Summary of `docs/external_research/power-envelope-forecast-basis.md` — a two-round,
explicitly **open/unfinished** external web-research conversation (user + a websearch
subagent, 2026-08-11) on Dynamic Operating Envelopes (DOEs): why real-world
import/export capacity-limit forecasts vary hour-by-hour, what real DNSPs base the
calculation on, and a sourced critique of a proposed static/symmetric equal-share
alternative. ~20 external sources cited inline; see the source document for the full
list and the two open questions it leaves unresolved.

## Why it matters here

This lab's VEN parses the Dynamic Operating Envelope (`IMPORT_CAPACITY_LIMIT`/
`EXPORT_CAPACITY_LIMIT`) as a **transport-only** consumer — `parse_capacity_schedule`
(`VEN/src/controller/rate_schedule.rs`) faithfully relays whatever schedule a VTN
publishes, with no opinion on how that schedule should be computed. This research thread
exists because the lab's own VTN currently has **no generator** for that schedule at all
(`GET /capacity/schedule` has nothing upstream feeding it a realistic curve) — the
open question the source document ends on is exactly what a toy generator for this lab
should be based on, informed by what real DNSPs actually do.

## Key claims (see source doc for full citations)

1. A real envelope varies hour-by-hour because voltage-rise (midday PV export) and
   thermal loading (evening demand peak) are the binding constraints, and they move in
   *opposite* directions across the day — not because the physical line itself changes.
2. Real DNSPs (SA Power Networks, live since 2021) compute it via a power-flow model
   over the LV network, refreshed roughly every 15 minutes, with an equal-share
   allocation rule across connected customers on top.
3. A flat, symmetric, worst-case-conservative limit (the "safe but restrictive"
   alternative) is essentially the pre-DOE status quo; the *diversity/coincidence
   factor* is the concrete engineering reason it wastes real headroom.
4. Import/export symmetry doesn't hold even on a thermally-symmetric line — the
   statutory voltage band itself is asymmetric, and PV export is more correlated across
   neighboring customers than demand is.
5. "Round-robin" capacity allocation is a real, documented fairness rule in the DER
   curtailment literature, but converges with continuous dynamic equal-share DOEs at the
   high-refresh-frequency limit — not a separate strategy.

## Related wiki pages

[[tariffs-and-capacity]] (this lab's own `OadrCapacityState`/capacity-limit-schedule
domain model — the concrete code this research feeds), [[distributor-business-case-tiers]]
(Tier 3 "power envelopes" in this lab's own tiered-rollout framing),
[[dso-retailer-unbundled-tariff-coordination]] (a related, also-open research thread —
DLMP/shadow-price duality between tariffs and envelopes, same "who actually
computes/owns this number" question from the price side).

## Status

**Open.** The source document's own two Open Questions (this lab's eventual test-envelope
generator choice; one secondary source needing a primary-standard citation) are not yet
resolved. Re-ingest `docs/external_research/power-envelope-forecast-basis.md` when either
is answered.
