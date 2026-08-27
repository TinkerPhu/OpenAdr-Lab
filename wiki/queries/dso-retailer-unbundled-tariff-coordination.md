---
title: DSO/Retailer Coordination on Tariffs When Grid and Energy Sales Are Legally Unbundled
type: query
created: 2026-07-31
updated: 2026-07-31
synced_commit: aff3f57
sources:
  - docs/openadr_3_1_specs/
  - openleadr-rs/openleadr-wire/src/program.rs
  - VEN/src/entities/tariff_snapshot.rs
  - VEN/src/controller/arbiter.rs
tags: [business-case, dso, retailer, unbundling, tariff, dlmp, regulation, programs]
---

# DSO/Retailer Coordination on Tariffs When Grid and Energy Sales Are Legally Unbundled

> Question: many jurisdictions legally separate grid maintenance from energy sales — the
> grid company (DSO) may be forbidden from selling energy, yet it operates the OpenADR
> VTN and is only interested in publishing power envelopes. Tariffs are a stronger
> behavioural lever than envelopes. How can the DSO cooperate with energy retailers to
> get a correct tariff forecast alongside the grid envelopes, without crossing the
> unbundling line?

## Framing: [[distributor-business-case-tiers]] assumes the wrong org chart

That page's tiered rollout (open tariff → CPP → envelopes → capacity products/VPP) is
staged as if *one* distributor owns all four tiers. Under legal unbundling that's wrong:
the DSO can own the envelope tier (Tier 3) outright — that's grid operation — but must
not own the tariff tiers (1, 2), which are energy-sales activities reserved for the
retailer. [[openadr-programs]] already assumes multi-actor programs in passing ("often
from different actors — retailer, DSO, aggregator"), and the protocol backs this
structurally: `ProgramContent` carries `retailer_name`/`retailer_long_name` as fields
distinct from the VTN operator identity (`openleadr-rs/openleadr-wire/src/program.rs`),
and [[tariffs-and-capacity]]'s `TariffSnapshot` merges `PRICE` payloads **per interval
regardless of which program/provider emitted them**, so the VEN's planner is already
provider-agnostic — one DSO and one retailer can each run their own program against the
same VTN (or federated VTNs) with zero protocol change.

## First pass: give the DSO its own price lever — but is that honest?

The obvious fix is to let the DSO publish a second `PRICE`-type program (a dynamic
**network fee**, not an energy price) alongside the retailer's commodity-price program;
[[tariffs-and-capacity]]'s merge-by-summation already handles two independently-set
€/kWh components landing on one bill. Real precedent: Germany's **§14a EnWG** — a
regulated dynamic network-fee component, separate from and additive to the retailer's
commodity price, tied to controllable-device compliance
([why §14a EnWG is key to the energy transition](https://enqt.de/en/blog/why-%C2%A714a-enwg-is-the-key-to-the-energy-transition/),
[§14a EnWG Modul 3 — time-variable grid charges](https://kiwigrid.com/en/article/14a-enwg-modul-3-how-electricity-customers-can-benefit-from-time-variable-grid-charges),
[Germany's Paragraph 14a EnWG — ESIG](https://www.esig.energy/germanys-paragraph-14a-enwg/)).

**But this only works if the DSO's price is *real* — a critical failure mode otherwise.**
If the DSO's tariff is merely a **virtual/unbilled shadow signal** with no real
settlement, nothing bounds its magnitude: the DSO can set it arbitrarily high to force
whatever compliance it wants, at zero cost to itself. In the VEN's cost-minimization
objective a sufficiently large fake cost coefficient always dominates a real, bounded
one — this is the "exact penalty method" from constrained optimization: a soft penalty
large enough becomes mathematically equivalent to a hard bound, just relabelled as a
cost. The retailer's real price becomes irrelevant noise underneath it, and the VEN's
"cost optimization" stops being about real money. In that failure mode, publishing a
plain hard `EXPORT_CAPACITY_LIMIT`/`IMPORT_CAPACITY_LIMIT` (Tier 3 of
[[distributor-business-case-tiers]], already how this lab implements envelopes) is
strictly **more honest** than dressing the same enforcement up as a fake price.

This project's own [[deviation-arbiter]] and [[milp-planner]] already compute a
structurally identical number — `marginal_cost_import/export_eur_per_kwh`, the plan's
shadow price, used purely to rank levers — but that case is safe *because* it is derived
from, and used only within, **one actor's own real tariff and own real envelope**. There
is no second party's real price for it to shadow. The moment a shadow price is
published *across* actors (DSO vs. retailer) with no settlement behind it, the honesty
problem in the previous paragraph reappears.

## Why the tariff/envelope relationship is not arbitrary — it's a shadow-price duality

The intuition that a "correct" DSO price and the envelope should be directly (inversely)
related is exactly right, and has a name: **distribution locational marginal pricing
(DLMP)**. In constrained optimization the correct price for a scarce resource is the
dual variable (shadow price) of the binding constraint — as remaining headroom (kW)
shrinks toward the envelope, the shadow price rises; at zero headroom it rises to
whatever value clears demand at that constraint. In the ideal case, a perfectly computed
real-time DLMP and a hard envelope carry the *same* information, expressed two ways — one
wouldn't need both.

- [DLMP for Congestion Management and Voltage Support (IEEE)](https://ieeexplore.ieee.org/document/8089425/)
- [DLMP under Generation and Network Scarcity Conditions](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC12705731/)
- [DLMP review — pricing mechanisms with high DER penetration](https://www.sciencedirect.com/science/article/pii/S2772671125001901)

The honest version of DLMP is **discovered by clearing a real market** (matching actual
generation offers and load bids against the true physical constraint) or derived from a
genuine optimal-power-flow dual — it converges to whatever value clears supply and
demand at the constraint, not to an arbitrarily inflated number. The duality breaks from
the *ideal* case in practice for three reasons, which is why real deployments keep both
instruments instead of collapsing into one:

1. **Guarantee vs. nudge.** Price is statistical — enough households respond on average,
   but no individual is compelled. A feeder about to trip cannot rely on average
   response; it needs the hard cap as backstop. Economic dispatch (price) and
   redispatch/curtailment (hard limit) coexist in real grid operation for this reason,
   not because one is obsolete.
2. **Aggregate vs. individual.** The true shadow price is a property of the whole
   feeder's joint load, computable only with a live load-flow/OPF model of everyone on
   it. The envelope sent to one VEN can be set locally and conservatively without
   solving that joint problem — cheaper to compute, cruder in effect.
3. **Latency/granularity.** Prices are typically published day-ahead in blocks (the
   spec's `bindingEvents` concept — often fixed once transmitted); a physical constraint
   can emerge on a minutes timescale that a republish cycle can't track.

## Resolution: the relation between the two prices is set by a regulator, not negotiated

The real answer to "who decides how the DSO's price relates to the retailer's price" is:
**neither party does — a regulator bounds it, via revenue-cap / incentive regulation,
the same authority that enforces the unbundling rule in the first place.** Real dynamic
network tariffs (§14a EnWG) are filed with and approved by the national regulator
(Bundesnetzagentur), which caps the DSO's *total* allowed network revenue independent of
how it is time-sliced — the DSO cannot extract more money by raising the dynamic
component; it can only reshape *when* the same pre-approved total is collected. That
structurally forecloses "set it arbitrarily high to force compliance": the price has to
average out to the regulator-approved revenue.

- [Revenue Cap regulation overview (ScienceDirect)](https://www.sciencedirect.com/topics/engineering/revenue-cap)
- [Aligning business models with public interest — network revenue regulation (RAP)](https://blueprint.raponline.org/deep-dive/aligning-business-models-with-public-interest-rethinking-revenue-regulation-for-network-companies/)
- [German Regulation overview (TenneT)](https://www.tennet.eu/de-en/markets/german-regulation)

## Three cases, not one blurry "DSO tariff"

1. **Internal shadow price, single actor, no cross-party competition.** This project's
   own `marginal_cost_import/export_eur_per_kwh` ([[deviation-arbiter]],
   [[milp-planner]]). Safe because it is derived from, and used only within, one
   household's own real tariff plus own real envelope.
2. **Cross-party virtual price, no settlement, no regulatory cap.** Structurally
   dishonest — collapses into a disguised hard constraint that shadows the retailer's
   real price. Should just *be* the hard envelope (Tier 3) instead; publishing a fake
   price adds no information and hides what's actually happening.
3. **Cross-party real price, regulator-approved and revenue-capped.** The only
   legitimate form of a DSO tariff that coexists with a retailer's tariff — its
   magnitude is set by an external authority (§14a's Bundesnetzagentur approval), not by
   the DSO's own compliance target. [[tariffs-and-capacity]]'s summation-by-interval
   already handles two such real, independently-owned €/kWh components on one bill.

## Complementary mechanism: congestion-forecast data exchange upstream of both parties

Independent of which tariff design is used, a DSO and retailer still need to avoid their
signals actively fighting each other (e.g., the retailer's cheap hour landing exactly on
the DSO's tight feeder window). The real-world pattern for this is a **shared
market/coordination platform sitting upstream of both parties' publishing**, not a
protocol-level fix inside OpenADR itself:

- **GOPACS** (Netherlands) — a joint TSO/DSO congestion-management platform where any
  market party (retailers, aggregators) can see congestion locations and trade
  flexibility to resolve them, without merging DSO and retailer legal roles. Cited by
  the EU as a blueprint case study.
  ([GOPACS — what it is](https://www.gopacs.eu/en/what-is-gopacs/),
  [EU report: GOPACS as a blueprint](https://www.gopacs.eu/en/news/eu-report-gopacs-as-a-blueprint-for-the-power-grid-of-the-future/))
- **Market Model 3.0** — an emerging EU regulatory framework explicitly clarifying data
  flows and roles between DSOs, retailers, and aggregators, relaxing the older
  requirement that tied every aggregator to a specific retailer/balance-responsible
  party.
  ([Scaling Demand-Side Flexibility Through Dynamic Tariffs (arXiv)](https://arxiv.org/html/2606.13401),
  [ACER — DSO revenue-setting report 2026](https://www.acer.europa.eu/sites/default/files/documents/Publications/ACER-2026-DSO-revenue-setting-report.pdf))
- A peer-reviewed study validating OpenADR specifically at DSO level, supporting the
  claim that a DSO can be VTN operator without being tariff-setter:
  [Flexibility Services Based on OpenADR Protocol for DSO Level (MDPI/Sensors)](https://mdpi.com/1424-8220/20/21/6266/htm).

## Summary

- Multi-provider, multi-program on one VTN is native to OpenADR 3, not a workaround —
  [[openadr-programs]], `ProgramContent.retailer_name`, [[tariffs-and-capacity]]'s
  per-interval merge.
- A DSO tariff is only honest if it is either (a) purely internal to one actor's own
  optimization (this lab's [[deviation-arbiter]] shadow price) or (b) a real,
  regulator-approved, revenue-capped charge (§14a EnWG). An unbounded cross-party
  virtual price is indistinguishable from a disguised hard constraint and should be
  replaced by an actual hard envelope (Tier 3) instead.
- The theoretically "correct" DSO price *is* the shadow price of the envelope constraint
  (DLMP) — they are dual expressions of the same physical limit — but real deployments
  keep both because price only nudges statistically while the envelope guarantees
  safety, and true real-time DLMP computation is still research-stage, not deployed at
  retail scale.
- What decides the DSO-price/retailer-price relationship is regulation (revenue-cap
  methodology), not negotiation between the two parties or protocol design — which is
  the same authority that enforces the unbundling rule this whole question started from.
- Congestion coordination between DSO and retailer, independent of tariff design, runs
  through an upstream data-exchange/market platform (GOPACS-style), not through OpenADR
  itself.

Related: [[distributor-business-case-tiers]] (single-owner tiered rollout — the
assumption this query corrects), [[openadr-programs]] (multi-actor program structure),
[[tariffs-and-capacity]] (merge mechanics), [[deviation-arbiter]] (this lab's own
internal shadow-price precedent), [[milp-planner]] (marginal-cost extension §5.2).
