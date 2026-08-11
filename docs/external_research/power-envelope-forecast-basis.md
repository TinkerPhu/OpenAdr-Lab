# Power Envelope Forecasts — Why They Vary, What Real DNSPs Base Them On, and a Critique of Static Equal-Share Allocation

**Participants:** the user (project owner, wiki keeper) and a websearch research agent
(general-purpose subagent using WebSearch/WebFetch), in a two-round conversation held
2026-08-11 while this lab's VEN was gaining Dynamic Operating Envelope support
(`VEN/src/controller/rate_schedule.rs`'s `parse_capacity_schedule`; history persistence
via `history-envelope-persistence`, `docs/history/project_journal.md` 2026-08-11 entry).

**Status: ongoing, not closed.** This document preserves the back-and-forth structure —
claim, counter-argument, source — rather than flattening it into one finished answer,
because the underlying question (what a VTN operator's envelope-generation algorithm
should actually be, for *this* lab) is still open. Treat "Conclusions" as best-answer-so-far
with citations, not a settled design decision. See Open Questions at the end.

## Round 1 — Question

> Why do envelope forecasts differ during the day? The grid stays the same, so it must
> be the power that changes. What would be the basis of creating a power envelope
> forecast? Do you have real-world examples with sources?

### Why the physical line constraint isn't the daily variable

The wire/transformer's thermal rating is fixed — what varies hour to hour is **how much
of that fixed capacity everyone else on the same low-voltage segment is using at that
moment**. A dynamic envelope is, structurally:

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
   - Source: "A Review of Dynamic Operating Envelopes: Computation, Applications and
     Challenges" (MDPI) — https://www.mdpi.com/2673-3951/6/2/29
   - Source: ARENA/ANU "On the calculation and use of dynamic operating envelopes" —
     https://arena.gov.au/assets/2020/09/on-the-calculation-and-use-of-dynamic-operating-envelopes.pdf
2. **An allocation rule on top of the model.** Once total feeder/transformer headroom is
   computed, it's split across connected customers. **SA Power Networks'** live system
   (South Australia, running since Sept 2021) uses simple **equal allocation per
   customer at the LV transformer**, republished every **15 minutes**, over IEEE
   2030.5/CSIP-AUS.
   - Source: ARENA-funded "Flexible Exports for Solar PV Trial Final Report" —
     https://arena.gov.au/assets/2024/01/SA-Power-Networks-Flexible-Exports-for-Solar-PV-Trial-Final-Report.pdf
   - Source: SA Power Networks lessons-learnt report —
     https://www.sapowernetworks.com.au/public/download.jsp?id=320259

**Forecast horizon matters empirically**: ANU/CSIRO research found close-to-real-time
(≈30-minutes-ahead) envelopes outperform day-ahead ones, because day-ahead voltage
forecasts are the weakest input — real systems favor short-horizon, frequently-refreshed
envelopes over one long day-ahead curve.
- Source: ARENA/ANU DOE report (above)
- Source: "Operating Envelopes under Probabilistic Electricity Demand and Solar
  Generation Forecasts" (arXiv) — https://arxiv.org/pdf/2207.09818

Other named examples:
- Energex/Energy Queensland "Dynamic Connections" —
  https://www.energex.com.au/our-services/connections/residential-and-commercial-connections/solar-connections-and-other-technologies/dynamic-connections-for-energy-exports/about-dynamic-connections
- Australia-wide smart-meter uptake context: AER "Insights into Australia's growing
  two-way energy system" (2024) —
  https://www.aer.gov.au/system/files/2024-12/Insights%20into%20Australia's%20growing%20two-way%20energy%20system%20-%20Export%20services%20network%20performance%20report%202024_1.pdf
- UK's more reactive, less forecast-curve-based Active Network Management / "Flexible
  Connections": UK Power Networks —
  https://www.ukpowernetworks.co.uk/new-electricity-connections/distributed-energy-resources-der-generation/flexible-connections
  — and Energy Networks Association "Flexibility Connections: Explainer and Q&A" —
  https://www.energynetworks.org/assets/images/Resource%20library/ON21-WS1A%20Open%20Networks%20Flexibility%20Connections%20Explainer%20and%20Q&A%20(19%20Aug%202021).pdf
- OpenADR 3's own `IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT` payloads are explicitly
  just the *transport*: the VTN/utility is expected to run whatever calculation
  internally and publish the resulting series. Source: OpenADR Alliance "DER & Grid Code
  Management with OpenADR 3" —
  https://www.openadr.org/assets/OpenADR%203%20and%20DER%20Management_v1.0.pdf

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
utilisation and unnecessary curtailment."
- Source: ScienceDirect DOE review —
  https://www.sciencedirect.com/science/article/pii/S030626192600824X
- Source: ARENA DOE workstream —
  https://arena.gov.au/knowledge-innovation/distributed-energy-integration-program/dynamic-operating-envelopes-workstream/

**Against flat allocation, concretely:** the *diversity factor* (or coincidence factor)
— network engineering has never sized feeders for "every customer at full allocation
simultaneously," because the ratio of coincident peak demand to the sum of individual
peak demands is normally well below 1 and falls further as customer count grows.
- Source: Wikipedia, "Diversity factor" — https://en.wikipedia.org/wiki/Diversity_factor
- Source: Electrical Installation Guide, kVA demand estimation —
  https://www.electrical-installation.org/enwiki/Estimation_of_actual_maximum_kVA_demand

The 50kW/10-households/5kW-each model implicitly assumes coincidence factor = 1, all day,
every day. SA Power Networks' real numbers show the cost of that assumption: their
**static** export limit is 1.5 kW/customer; their **dynamic** limit varies up to 10
kW/phase, and "participating customers have more than doubled their exports" under
dynamic vs. the static cap.
- Source: SA Power Networks Flexible Exports —
  https://www.sapowernetworks.com.au/industry/flexible-exports/

### 2. "Direction doesn't matter if the line can bear 50kW" — doesn't hold up

Thermal rating of a conductor/transformer genuinely is direction-agnostic, but the
usually-binding constraint is *voltage*, not thermal — and voltage is asymmetric for two
independent reasons:

- The statutory voltage band itself is asymmetric. Australia's AS 60038 LV band is 230V
  **+10%/−6%** (216–253V) — export has *less* headroom to push voltage up than import
  has to pull it down. Real feeders also typically already sit above nominal (~240V)
  before any PV export starts, eating further into the rise headroom.
  - Source: AS 60038-2012 (ANSI listing) — https://webstore.ansi.org/standards/sai/600382012
  - Source: IEC/AS 60038 voltage levels — https://myelectrical.com/notes/entryid/203/voltage-levels-to-iec-60038
  - Source: SolarQuotes, voltage rise/drop explainer — https://www.solarquotes.com.au/blog/solar-voltage-rise-drop/
- Export is more *correlated* across customers than import. Everyone's panels peak near
  solar noon on a clear day; cooking/EV/heating times are comparatively diverse. The same
  diversity-factor argument above, applied per-direction, pushes worst-case coincidence
  higher for export than for import at equal aggregate installed capacity — a documented
  reason DNSPs often permit a higher combined import ceiling than export ceiling
  relative to line rating.
  - Source (secondary — see Open Questions): DNSP voltage-rise design-margin discussion,
    IET EngX — https://engx.theiet.org/f/wiring-and-regulations/21339/dno-voltage-limits-and-solar-pv-generation

### 3. "Round robin forecast" — real concept, but it's a fairness rule, not a competing forecast strategy

**For:** round-robin (or "rota") allocation is documented in the DER-curtailment
literature as one of several competing *fairness rules* for who gets curtailed first
under congestion — alongside pro-rata (proportional to capacity) and LIFO
(last-connected curtailed first). LIFO concentrates nearly all curtailment on
late-connecting/low-priority DER; rota/round-robin spreads the burden more evenly across
repeated events.
- Source: Ilo et al., game-theoretic curtailment rules (arXiv) — https://arxiv.org/pdf/1908.10313
- Source: non-firm connection allocation (arXiv) — https://arxiv.org/pdf/2606.03887
- Source: ScienceDirect, connection queues with flexible agreements —
  https://www.sciencedirect.com/science/article/pii/S0306261925009900

**Against treating it as separate from dynamic equal-share:** round-robin is a
*discrete-time, binary* (on/off, or full-share/no-share) rule applied *within* a
congestion event, whereas SA Power Networks-style DOEs recompute a *continuous*
equal(ish)-share value every ~15 minutes from live power-flow state. As the refresh
interval shrinks while the allocation stays proportionally equal-share, the two converge
— round-robin is the low-frequency, binary-valued limit of the same underlying idea;
continuous DOE is the high-frequency, real-valued version. The real design axis is
*discrete/binary-turns vs. continuous-value* and *infrequent/static vs.
frequent/state-aware*, not "round robin vs. envelope" as separate categories.

## Conclusions (best answer so far)

1. **A flat, symmetric, worst-case-conservative allocation is a real, well-documented
   baseline** — it is essentially what DNSPs did before DOEs, and the literature's own
   critique of it (inefficient, unnecessarily curtailing) matches the user's own "safe
   but very restrictive" framing.
   Sources: ScienceDirect DOE review, ARENA DOE workstream (Round 2 §1).
2. **The reason it's restrictive has a name — diversity/coincidence factor** — and real
   deployed numbers (SA Power Networks: 1.5 kW static vs. up to 10 kW dynamic, customers
   doubling exports) quantify how much headroom a flat allocation leaves on the table.
   Sources: Wikipedia diversity factor, SA Power Networks Flexible Exports (Round 2 §1).
3. **Symmetric import/export limits are not physically justified even on a
   thermally-symmetric line** — the statutory voltage band is itself asymmetric
   (+10%/−6% around nominal in Australia), and export coincidence is structurally higher
   than import coincidence because solar output is correlated across a neighborhood in a
   way demand usually isn't.
   Sources: AS 60038 voltage band, SolarQuotes voltage rise/drop (Round 2 §2).
4. **"Round robin" is a real, named alternative in the literature — but it's the same
   idea as dynamic equal-share DOEs, viewed at low refresh frequency and binary
   granularity**, not a third, unrelated strategy. The user's own instinct that it
   connects to "the other approach" was correct.
   Sources: game-theoretic curtailment rules (arXiv), SA Power Networks trial report
   (15-minute continuous refresh, as the "other end" of the same spectrum) (Round 2 §3).
5. **The actual real-world basis for a power-envelope forecast is a power-flow model
   over a network model, fed by load/PV/weather forecasts, refreshed frequently (SA
   Power Networks: 15 min) rather than computed once per day** — not a fixed number, and
   not (in deployed systems) a machine-learning black box; it's closer to a
   physics-constrained optimization re-solved on a short cycle.
   Sources: MDPI DOE computation review, ARENA/ANU DOE calculation report (Round 1).

## Open Questions

- None of the above settles what *this lab's* VTN should actually do to generate a
  plausible test envelope schedule (`GET /capacity/schedule`'s upstream data today has
  no generator at all). Candidates worth a follow-up round: (a) a toy
  equal-share-of-a-fixed-line-rating generator (closest to the user's original proposal,
  cheapest to build, and honestly labeled as the *pre-DOE* baseline rather than a real
  DOE), (b) a simple time-of-day sinusoid/step curve loosely shaped like the SA Power
  Networks pattern (export-tight at midday, import-tight in the evening) with no real
  network model behind it, or (c) a genuine small power-flow toy model. No decision has
  been made; this document should be updated once one is chosen.
- The asymmetric-band argument's *directional-correlation* half (Round 2 §2, second
  bullet) was corroborated by one secondary discussion-forum source (IET EngX), not a
  primary DNSP planning standard directly — worth firming up with a primary DNSP or
  standards-body source before treating it as fully settled.

## All sources (deduplicated)

- ScienceDirect, "Dynamic operating envelopes" review — https://www.sciencedirect.com/science/article/pii/S030626192600824X
- ARENA, Dynamic Operating Envelopes Workstream — https://arena.gov.au/knowledge-innovation/distributed-energy-integration-program/dynamic-operating-envelopes-workstream/
- ARENA/ANU, "On the calculation and use of dynamic operating envelopes" — https://arena.gov.au/assets/2020/09/on-the-calculation-and-use-of-dynamic-operating-envelopes.pdf
- "A Review of Dynamic Operating Envelopes: Computation, Applications and Challenges" (MDPI) — https://www.mdpi.com/2673-3951/6/2/29
- "The Importance of Technical Distribution Network Limits in Dynamic Operating Envelopes" (arXiv) — https://arxiv.org/pdf/2304.07806
- "Operating Envelopes under Probabilistic Electricity Demand and Solar Generation Forecasts" (arXiv) — https://arxiv.org/pdf/2207.09818
- SA Power Networks, Flexible Exports — https://www.sapowernetworks.com.au/industry/flexible-exports/
- SA Power Networks, Flexible Exports Trial Final Report (via ARENA) — https://arena.gov.au/assets/2024/01/SA-Power-Networks-Flexible-Exports-for-Solar-PV-Trial-Final-Report.pdf
- SA Power Networks, lessons-learnt report — https://www.sapowernetworks.com.au/public/download.jsp?id=320259
- Energex, "About Dynamic Connections" — https://www.energex.com.au/our-services/connections/residential-and-commercial-connections/solar-connections-and-other-technologies/dynamic-connections-for-energy-exports/about-dynamic-connections
- AER, "Insights into Australia's growing two-way energy system" (2024) — https://www.aer.gov.au/system/files/2024-12/Insights%20into%20Australia's%20growing%20two-way%20energy%20system%20-%20Export%20services%20network%20performance%20report%202024_1.pdf
- UK Power Networks, "Flexible connections" — https://www.ukpowernetworks.co.uk/new-electricity-connections/distributed-energy-resources-der-generation/flexible-connections
- Energy Networks Association (UK), "Flexibility Connections: Explainer and Q&A" — https://www.energynetworks.org/assets/images/Resource%20library/ON21-WS1A%20Open%20Networks%20Flexibility%20Connections%20Explainer%20and%20Q&A%20(19%20Aug%202021).pdf
- OpenADR Alliance, "DER & Grid Code Management with OpenADR 3" — https://www.openadr.org/assets/OpenADR%203%20and%20DER%20Management_v1.0.pdf
- Wikipedia, "Diversity factor" — https://en.wikipedia.org/wiki/Diversity_factor
- Electrical Installation Guide, "Estimation of actual maximum kVA demand" — https://www.electrical-installation.org/enwiki/Estimation_of_actual_maximum_kVA_demand
- AS 60038-2012 (ANSI listing) — https://webstore.ansi.org/standards/sai/600382012
- myelectrical.com, "Voltage levels to IEC 60038" — https://myelectrical.com/notes/entryid/203/voltage-levels-to-iec-60038
- SolarQuotes, "Solar voltage rise/drop" — https://www.solarquotes.com.au/blog/solar-voltage-rise-drop/
- IET EngX discussion, "DNO voltage limits and solar PV generation" — https://engx.theiet.org/f/wiring-and-regulations/21339/dno-voltage-limits-and-solar-pv-generation
- Ilo et al., game-theoretic curtailment rules (arXiv) — https://arxiv.org/pdf/1908.10313
- Non-firm connection allocation (arXiv) — https://arxiv.org/pdf/2606.03887
- ScienceDirect, "Managing connection queues with flexible agreements" — https://www.sciencedirect.com/science/article/pii/S0306261925009900
