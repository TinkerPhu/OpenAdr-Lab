---
title: Distributor Business Case — Tiered VTN/VEN Rollout
type: query
created: 2026-07-06
updated: 2026-07-06
synced_commit: afe2450
sources:
  - docs/openadr_3_1_specs/
  - docs/REQUIREMENTS.md
  - tests/features/
tags: [business-case, dso, rollout, programs, flexibility]
---

# Distributor Business Case — Tiered VTN/VEN Rollout

> Question: what arguments convince an energy distributor to develop and introduce a
> VTN and VENs? Start with the simplest low-cost version that can create profit, then
> show further tiers extending the system and the profits. Background: increasing
> volatility and PV in the grid, causing both energy oversupply and local power
> overload.

## Framing: the PV problem is really two problems

Increasing PV creates two *distinct* pains, and OpenADR 3 has a distinct signal type
for each ([[tariffs-and-capacity]] — "two kinds of capacity, don't conflate"):

1. **Energy oversupply** — a *market* problem, system-wide, priced in €/kWh. The tool
   is `PRICE`/`EXPORT_PRICE` events.
2. **Power overload** — a *physics* problem, local to specific feeders/transformers,
   bounded in kW. The tool is `IMPORT_/EXPORT_CAPACITY_LIMIT` events.

A distributor that only builds tariffs never fixes the feeder overload; one that only
curtails inverters statically wastes energy the market would happily absorb. The
argument for OpenADR is that **one VTN carries both signal families to the same
customer devices**, deployable in tiers of increasing cost and increasing return. The
program model makes the tiers additive: each tier is simply a new program next to the
existing ones, and one customer can be enrolled in several simultaneously
([[openadr-programs]], User Guide §6.4).

## Tier 1 — Open dynamic tariff publication (cheapest, profitable from day one)

**What it is:** a VTN publishing day-ahead hourly prices as `PRICE` events in one open
program. The spec explicitly permits a **non-authenticating VTN for public-information
programs**: no enrollment, no credentials, no reports ([[openadr-security]], User Guide
§6.1). That removes the expensive parts — no onboarding flow, no customer-database
integration, no report ingestion. Deployment is one server publishing data the
distributor already has (its dynamic tariff).

**Where the profit comes from:**

- Customers with HEMS shift load into oversupply hours (midday PV) and out of expensive
  hours — directly reducing **procurement and imbalance costs** and monetizing
  negative-price hours instead of suffering them. This is the lab's proven core loop:
  `PRICE` events → MILP cost objective → shifted consumption
  ([[openadr-spec-use-cases]] §8.3 ✅, [[milp-planner]]).
- **The VEN cost is not the distributor's.** OpenADR is an open standard: HEMS,
  wallbox, and battery vendors implement the client side; the distributor publishes,
  they consume. A proprietary API would mean funding every integration.
- Marketing asset: a machine-readable tariff differentiates for exactly the customers
  (PV + battery + EV owners) who cause and can solve the volatility.

**Expectation setting:** response is voluntary and unverified — settlement is simply
the meter bill. Statistical load shifting, not guaranteed kW
([[openadr-programs]], example 1).

## Tier 2 — Enrolled critical-peak / load-shed program (verified response)

**What it adds:** OAuth2 credentials, customer enrollment, and **reports flowing back**
(`USAGE`, baseline-vs-actual M&V — [[demand-response]]). Now a CPP program becomes
possible: a discounted base rate in exchange for shedding on ~10 stress days per year
([[openadr-programs]], example 2), with the response **verified and countable** because
measured usage is compared against a baseline.

**Where the profit comes from:**

- Verified, contractible peak reduction lets flexibility count as firm capacity in
  **grid planning** — deferring reinforcement and reducing peak procurement.
- **Targeting** scopes events to the customers on a specific stressed feeder
  ([[openadr-security]] targeting; [[openadr-spec-use-cases]] §8.13 ✅) — shed is
  bought only where it helps.
- The same infrastructure carries `GHG` signals for green-tariff products.

**Incremental cost:** the enrollment process (out-of-band, contractual sign-up on the
distributor's customer portal — [[openadr-programs]] enrollment section), token
issuance, and report storage. The VTN is the same one from Tier 1.

## Tier 3 — Dynamic operating envelopes (the answer to feeder overload)

**What it adds:** `EXPORT_CAPACITY_LIMIT` / `IMPORT_CAPACITY_LIMIT` events per feeder —
dynamic kW caps sent *only when the feeder actually needs them*
([[openadr-programs]], example 4; User Guide §8.10). This tier directly attacks the PV
power-overload problem.

**Where the profit comes from — the big one:**

- **Hosting capacity without copper.** The alternatives are rejecting PV connection
  requests, capping inverters statically (e.g. 50–70% forever), or reinforcing the
  feeder. Dynamic envelopes connect **2–3× the PV on existing cables**, because the cap
  only binds on the few critical sunny hours. Avoided reinforcement capex is typically
  the largest single number in any DSO flexibility business case.
- Customers accept it because it is the *condition for connecting oversized PV at all*,
  and their HEMS absorbs the cap gracefully — diverting surplus into battery, EV, and
  heater instead of losing it. The lab demonstrates precisely this:
  `EXPORT_CAPACITY_LIMIT` becomes a hard MILP constraint and the planner reroutes the
  surplus ([[tariffs-and-capacity]], [[openadr-interface]], [[system-use-cases]] #2 —
  BDD-tested).
- Less curtailment compensation paid out, and faster connection approvals (a regulated
  quality metric in many jurisdictions).

**Expectation setting:** a grid-safety program — hard compliance expected, possibly
with capacity-reservation reports back; the VEN side expects caps only when genuinely
needed ([[openadr-programs]], example 4).

## Tier 4 — Capacity products and VPP dispatch (new revenue, not just avoided cost)

**What it adds:** the contractual capacity layer (`IMPORT_CAPACITY_SUBSCRIPTION` /
`_RESERVATION` — guaranteed-kW products) and eventually direct dispatch
(`DISPATCH_SETPOINT`, SoC telemetry) to aggregate home batteries into a virtual power
plant for balancing/redispatch markets ([[openadr-programs]], example 5).

**Where the profit comes from:**

- Capacity subscriptions turn grid-connection capacity into a **priced, differentiated
  product** instead of a flat fee — customers who accept lower firm capacity pay less;
  the distributor gains headroom certainty.
- VPP dispatch converts the fleet's aggregate battery capacity into
  **balancing-market revenue** — the first tier that *earns* money rather than avoiding
  costs. Also the most demanding: near-real-time control, tight telemetry, guaranteed
  customer SoC reserves.
- Maturity note from the lab's gap analysis: subscriptions/reservations are parsed but
  don't yet constrain the planner here, and `DISPATCH_SETPOINT` handling is unbuilt
  ([[openadr-spec-use-cases]] §8.10 🟡, §8.5/§8.12 ❌) — consistent with this being the
  last tier commercially, too.

## Cross-cutting arguments

- **One investment, four products.** Every tier reuses the same VTN, protocol, and
  customer devices. Programs are additive by design — a customer enrolls in the tariff
  program *and* the envelope program *and* the EV program simultaneously
  ([[openadr-programs]], multi-program section).
- **The standard shifts cost off the distributor's balance sheet.** VENs are built by
  device vendors against a published spec. No lock-in on either side.
- **Risk-staged rollout.** Tier 1 goes live without touching billing, security
  infrastructure, or customer contracts; each later tier is triggered only when the
  previous one shows measured response — which Tier 2's M&V machinery provides the data
  to prove.
- **The trend is one-directional.** PV volatility and local overload grow; the
  alternatives (reinforcement, static curtailment, rejected connections) all get more
  expensive, while flexibility gets cheaper as HEMS penetration rises.
