---
title: Power Envelope Forecasts — Why They Vary, What Real DNSPs Base Them On, and a Critique of Static Equal-Share Allocation
type: query
created: 2026-08-11
updated: 2026-08-11
synced_commit: 17b702a
sources:
  - VEN/src/entities/capacity.rs
  - VEN/src/controller/rate_schedule.rs
  - VEN/src/routes/hems/misc.rs
  - docs/openadr_3_1_specs/
tags: [capacity, envelope, doe, dso, forecasting, external-research, open]
---

# Power Envelope Forecasts — Why They Vary, What Real DNSPs Base Them On, and a Critique of Static Equal-Share Allocation

> **Status: ongoing discussion, not yet closed.** This page is a working transcript of a
> multi-round research conversation between the **user (wiki keeper)** and a **websearch
> research agent** (general-purpose subagent, WebSearch/WebFetch), conducted while
> building this lab's Dynamic Operating Envelope support (`VEN/src/controller/
> rate_schedule.rs`'s `parse_capacity_schedule`, and the `history-envelope-persistence`
> change persisting it — `docs/history/project_journal.md`, 2026-08-11 entry). It
> intentionally preserves the back-and-forth structure —
> claim, counter-argument, source — rather than flattening it into a single finished
> answer, because the underlying real-world question (what should a VTN operator's
> envelope-generation algorithm actually be, for *this* lab) is still open. Treat the
> **Conclusions** section as "best answer so far, with citations," not as settled.

## Round 1 — Question

> Why do envelope forecasts differ during the day? The grid stays the same, so it must
> be the power that changes. What would be the basis of creating a power envelope
> forecast? Do you have real-world examples with sources?

### Why the physical line constraint isn't the daily variable

The wire/transformer's thermal rating is fixed, correct — what varies hour to hour is
**how much of that fixed capacity everyone else on the same low-voltage segment is using
at that moment**. A dynamic envelope is, structurally:

```
limit(t) = physical thermal/voltage headroom − everyone else's simultaneous usage at time t
```

Two physical drivers dominate, and they push in *opposite* directions across the day:

- **Midday — voltage rise from PV export.** LV feeders must stay within a statutory
  voltage band. When many rooftop PV systems export simultaneously near solar noon,
  voltage climbs toward that band's ceiling — export headroom shrinks exactly when
  everyone wants to export most.
- **Evening — thermal loading from the demand peak.** Import capacity is capped by
  conductor/transformer thermal limits, which bind hardest during the evening peak (EVs,
  air-con, cooking) — import limits are typically lowest then.

Because these two constraints move in opposite directions through the day, and every
other customer's simultaneous behavior is itself time-varying and only partly
predictable in advance, a forecast *curve* is the natural output, not one flat number.

### How real DNSPs actually compute it

Two layers, in practice:

1. **A power-flow model** (Optimal Power Flow, or a simplified linear/DistFlow model) of
   the LV network — network topology + transformer/conductor ratings + forecasts of
   aggregate load and aggregate PV (from weather/irradiance forecasts and smart-meter
   history) as inputs — solved per interval for the maximum import/export each
   connection point can take without breaching voltage or thermal limits.
   ([MDPI review: "A Review of Dynamic Operating Envelopes: Computation, Applications
   and Challenges"](https://www.mdpi.com/2673-3951/6/2/29); [ARENA/ANU "On the
   calculation and use of dynamic operating
   envelopes"](https://arena.gov.au/assets/2020/09/on-the-calculation-and-use-of-dynamic-operating-envelopes.pdf))
2. **An allocation rule on top of the model.** Once total feeder/transformer headroom is
   computed, it's split across connected customers. **SA Power Networks'** live system
   (South Australia, running since Sept 2021) uses simple **equal allocation per
   customer at the LV transformer**, republished every **15 minutes**, over IEEE
   2030.5/CSIP-AUS. ([ARENA-funded "Flexible Exports for Solar PV Trial Final
   Report"](https://arena.gov.au/assets/2024/01/SA-Power-Networks-Flexible-Exports-for-Solar-PV-Trial-Final-Report.pdf);
   [SA Power Networks lessons-learnt
   report](https://www.sapowernetworks.com.au/public/download.jsp?id=320259))

**Forecast horizon matters empirically**: ANU/CSIRO research found close-to-real-time
(≈30-minutes-ahead) envelopes outperform day-ahead ones, because day-ahead voltage
forecasts are the weakest input — real systems favor short-horizon, frequently-refreshed
envelopes over one long day-ahead curve. ([ARENA/ANU DOE
report](https://arena.gov.au/assets/2020/09/on-the-calculation-and-use-of-dynamic-operating-envelopes.pdf);
["Operating Envelopes under Probabilistic Electricity Demand and Solar Generation
Forecasts" (arXiv)](https://arxiv.org/pdf/2207.09818))

Other named examples: **Energex/Energy Queensland "Dynamic Connections"**
([about page](https://www.energex.com.au/our-services/connections/residential-and-commercial-connections/solar-connections-and-other-technologies/dynamic-connections-for-energy-exports/about-dynamic-connections));
Australia-wide smart-meter uptake context in the [AER "Insights into Australia's growing
two-way energy system"
(2024)](https://www.aer.gov.au/system/files/2024-12/Insights%20into%20Australia's%20growing%20two-way%20energy%20system%20-%20Export%20services%20network%20performance%20report%202024_1.pdf);
UK's more reactive, less forecast-curve-based **Active Network Management / "Flexible
Connections"**
([UK Power Networks](https://www.ukpowernetworks.co.uk/new-electricity-connections/distributed-energy-resources-der-generation/flexible-connections),
[ENA "Flexibility Connections: Explainer and
Q&A"](https://www.energynetworks.org/assets/images/Resource%20library/ON21-WS1A%20Open%20Networks%20Flexibility%20Connections%20Explainer%20and%20Q&A%20(19%20Aug%202021).pdf)).
OpenADR 3's own `IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT` payloads are explicitly
just the *transport* — the VTN/utility is expected to run whatever calculation
internally and publish the resulting series ([OpenADR Alliance "DER & Grid Code
Management with OpenADR
3"](https://www.openadr.org/assets/OpenADR%203%20and%20DER%20Management_v1.0.pdf)).

## Round 2 — Question (user's counter-proposal)

> It could also be a fixed limit in both directions, in most simple terms: the line can
> bear 50kW, direction does not matter. We have 10 households on that line — everybody
> is allowed 5kW. That would be safe but very restrictive. But I do not understand any
> other forecast strategy, unless you create a kind of round robin forecast. Assess my
> statements and try to find arguments for and against my statements on the net.

### 1. Static equal-share IS the pre-DOE status quo, and yes, restrictive

**For the user's framing:** this is essentially the documented legacy approach. Pre-DOE
limits are described as "fixed at conservative levels regardless of network capacity
because they need to account for worst case scenario conditions," causing "inefficient
utilisation and unnecessary curtailment." ([ScienceDirect DOE
review](https://www.sciencedirect.com/science/article/pii/S030626192600824X); [ARENA DOE
workstream](https://arena.gov.au/knowledge-innovation/distributed-energy-integration-program/dynamic-operating-envelopes-workstream/))

**Against flat allocation, concretely:** the *diversity factor* (or coincidence factor)
— network engineering has never sized feeders for "every customer at full allocation
simultaneously," because the ratio of coincident peak demand to the sum of individual
peak demands is normally well below 1 and falls further as customer count grows.
([Wikipedia: diversity
factor](https://en.wikipedia.org/wiki/Diversity_factor); [Electrical Installation Guide,
kVA demand
estimation](https://www.electrical-installation.org/enwiki/Estimation_of_actual_maximum_kVA_demand))
The 50kW/10 households/5kW-each model implicitly assumes coincidence factor = 1, all day,
every day. SA Power Networks' real numbers show the cost of that assumption: their
**static** export limit is 1.5 kW/customer; their **dynamic** limit varies up to 10
kW/phase, and "participating customers have more than doubled their exports" under
dynamic vs. the static cap. ([SA Power Networks Flexible
Exports](https://www.sapowernetworks.com.au/industry/flexible-exports/))

### 2. "Direction doesn't matter if the line can bear 50kW" — this doesn't hold up

Thermal rating of a conductor/transformer genuinely is direction-agnostic, but the
usually-binding constraint is *voltage*, not thermal — and voltage is asymmetric for two
independent reasons:

- The statutory voltage band itself is asymmetric. Australia's AS 60038 LV band is 230V
  **+10%/−6%** (216–253V) — export has *less* headroom to push voltage up than import
  has to pull it down. Real feeders also typically already sit above nominal (~240V)
  before any PV export starts, eating further into the rise headroom. ([ANSI/AS
  60038 listing](https://webstore.ansi.org/standards/sai/600382012); [IEC/AS 60038
  voltage levels](https://myelectrical.com/notes/entryid/203/voltage-levels-to-iec-60038);
  [SolarQuotes voltage rise/drop
  explainer](https://www.solarquotes.com.au/blog/solar-voltage-rise-drop/))
- Export is more *correlated* across customers than import. Everyone's panels peak near
  solar noon on a clear day; cooking/EV/heating times are comparatively diverse. The same
  diversity-factor argument from point 1, applied per-direction, pushes worst-case
  coincidence higher for export than for import at equal aggregate installed capacity —
  a documented reason DNSPs often permit a higher combined import ceiling than export
  ceiling relative to line rating. (Directional correlation argument corroborated by the
  DNSP voltage-rise design-margin discussion at [IET
  EngX](https://engx.theiet.org/f/wiring-and-regulations/21339/dno-voltage-limits-and-solar-pv-generation).)

### 3. "Round robin forecast" — real concept, but it's a fairness rule, not a competing forecast strategy

**For:** round-robin (or "rota") allocation is documented in the DER-curtailment
literature as one of several competing *fairness rules* for who gets curtailed first
under congestion — alongside pro-rata (proportional to capacity) and LIFO (last-connected
curtailed first). LIFO concentrates nearly all curtailment on late-connecting/low-priority
DER; rota/round-robin spreads the burden more evenly across repeated events. ([Ilo et al.,
game-theoretic curtailment rules, arXiv](https://arxiv.org/pdf/1908.10313); [non-firm
connection allocation, arXiv](https://arxiv.org/pdf/2606.03887); [ScienceDirect:
connection queues with flexible
agreements](https://www.sciencedirect.com/science/article/pii/S0306261925009900))

**Against treating it as separate from dynamic equal-share:** round-robin is a
*discrete-time, binary* (on/off, or full-share/no-share) rule applied *within* a
congestion event, whereas SA Power Networks-style DOEs recompute a *continuous*
equal(ish)-share value every ~15 minutes from live power-flow state. As the refresh
interval shrinks while the allocation stays proportionally equal-share, the two converge
— round-robin is the low-frequency, binary-valued limit of the same underlying idea;
continuous DOE is the high-frequency, real-valued version. The real design axis is
*discrete/binary-turns vs. continuous-value* and *infrequent/static vs.
frequent/state-aware*, not "round robin vs. envelope" as separate categories.

## Conclusions (best answer so far — see Open Questions below)

1. **A flat, symmetric, worst-case-conservative allocation is a real, well-documented
   baseline** — it is essentially what DNSPs did before DOEs, and the literature's own
   critique of it (inefficient, unnecessarily curtailing) matches the user's own "safe
   but very restrictive" framing.
   *Sources: [ScienceDirect DOE review](https://www.sciencedirect.com/science/article/pii/S030626192600824X),
   [ARENA DOE workstream](https://arena.gov.au/knowledge-innovation/distributed-energy-integration-program/dynamic-operating-envelopes-workstream/).*
2. **The reason it's restrictive has a name — diversity/coincidence factor** — and real
   deployed numbers (SA Power Networks: 1.5 kW static vs. up to 10 kW dynamic,
   customers doubling exports) quantify how much headroom a flat allocation leaves on
   the table.
   *Sources: [diversity factor, Wikipedia](https://en.wikipedia.org/wiki/Diversity_factor),
   [SA Power Networks Flexible Exports](https://www.sapowernetworks.com.au/industry/flexible-exports/).*
3. **Symmetric import/export limits are not physically justified even on a
   thermally-symmetric line** — the statutory voltage band is itself asymmetric
   (+10%/−6% around nominal in Australia), and export coincidence is structurally higher
   than import coincidence because solar output is correlated across a neighborhood in a
   way demand usually isn't.
   *Sources: [AS 60038 voltage band](https://myelectrical.com/notes/entryid/203/voltage-levels-to-iec-60038),
   [SolarQuotes voltage rise/drop](https://www.solarquotes.com.au/blog/solar-voltage-rise-drop/).*
4. **"Round robin" is a real, named alternative in the literature — but it's the same
   idea as dynamic equal-share DOEs, viewed at low refresh frequency and binary
   granularity**, not a third, unrelated strategy. The user's own instinct that it
   connects to "the other approach" was correct.
   *Sources: [game-theoretic curtailment rules, arXiv](https://arxiv.org/pdf/1908.10313),
   [SA Power Networks trial report](https://arena.gov.au/assets/2024/01/SA-Power-Networks-Flexible-Exports-for-Solar-PV-Trial-Final-Report.pdf)
   (15-minute continuous refresh, as the "other end" of the same spectrum).*
5. **The actual real-world basis for a power-envelope forecast is a power-flow model
   over a network model, fed by load/PV/weather forecasts, refreshed frequently (SA
   Power Networks: 15 min) rather than computed once per day** — not a fixed number, and
   not (in deployed systems) a machine-learning black box; it's closer to a
   physics-constrained optimization re-solved on a short cycle.
   *Sources: [MDPI DOE computation review](https://www.mdpi.com/2673-3951/6/2/29),
   [ARENA/ANU DOE calculation report](https://arena.gov.au/assets/2020/09/on-the-calculation-and-use-of-dynamic-operating-envelopes.pdf).*

> **OPEN QUESTION** None of the above settles what *this lab's* VTN should actually do to
> generate a plausible test envelope schedule (`GET /capacity/schedule`'s upstream data
> today has no generator at all — see the "Backend" note in
> `docs/history/project_journal.md`'s 2026-08-10 entry, Controller-tab tariff/envelope
> split). Candidates worth a follow-up
> round: (a) a toy equal-share-of-a-fixed-line-rating generator (closest to the user's
> original proposal, cheapest to build, and honestly labeled as the *pre-DOE* baseline
> rather than a real DOE), (b) a simple time-of-day sinusoid/step curve loosely shaped
> like the SA Power Networks pattern (export-tight at midday, import-tight in the
> evening) with no real network model behind it, or (c) a genuine small power-flow toy
> model. No decision has been made; this page should be updated once one is chosen.

> **OPEN QUESTION** The asymmetric-band argument (point 3) was corroborated by one
> secondary discussion-forum source (IET EngX) for the *directional correlation*
> half of the claim, not a DNSP planning standard directly — worth firming up with a
> primary DNSP or standards-body source before treating it as fully settled.

## Related

[[tariffs-and-capacity]] (this lab's own `OadrCapacityState`/capacity-limit domain
model — the concrete artifact this research feeds), [[distributor-business-case-tiers]]
(Tier 3 "power envelopes" in this lab's own tiered-rollout framing),
[[dso-retailer-unbundled-tariff-coordination]] (a related but distinct open research
thread — DLMP/shadow-price duality between tariffs and envelopes, same "who actually
computes/owns this number" question from the price side instead of the capacity side).
