---
title: OpenADR Programs
type: concept
created: 2026-07-06
updated: 2026-07-06
synced_commit: 8c220b3
sources:
  - docs/openadr_3_1_specs/
  - VEN/src/tasks/poll_programs.rs
  - VEN/src/entities/capacity.rs
tags: [openadr, program, enrollment, dr, spec]
---

# OpenADR Programs

*"A program is a Demand Response offering of an energy provider. In OpenADR 3, a tariff
is simply a type of program."* (User Guide §6.4,
`docs/openadr_3_1_specs/3_OpenADR 3.1.0_User_Guide_20250801.md`)

A program is the **commercial product** — the thing advertised on a utility's website
("save money by letting us manage your EV charging"). The Program *object* in the VTN is
only the machine-readable metadata sheet for that product. Useful analogy: the program is
the **contract + rulebook**, events are the **daily instructions issued under that
contract**, reports are the **proof of performance the contract demands**. Protocol
entities: [[openadr-3]]; business actors and value chain: [[demand-response]].

## Structural rules

Two spec rules make the program load-bearing in the protocol (User Guide §6.4):

- **Every event and report belongs to exactly one program.** The program is the
  namespace/context that tells the VEN *how to interpret* the events inside it — the
  fast-moving events carry only numbers, the program carries the meaning.
- **Program metadata changes rarely** — "perhaps once a year or less". Programs are
  stable; events are the fast-moving content inside them. `program.programName` must be
  unique per VTN instance (Definition §programName,
  `docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md`).

## Five worked examples

### 1. Dynamic tariff program ("hourly day-ahead prices")

- **VTN/provider view:** "I'm an electricity retailer. My wholesale costs vary hourly.
  I publish a `PRICE` event every afternoon with 24 intervals for tomorrow. Customers
  who shift load to cheap hours reduce my procurement risk — I don't need them to
  *promise* anything, price is the incentive."
- **VEN/customer view:** "I have a battery and an EV. On a flat tariff my flexibility
  is worthless. On this program I charge at 06:00 solar-surplus prices instead of
  19:00 peak prices. My HEMS reads the prices and optimizes" — exactly this lab's core
  loop: `PRICE` events → MILP cost objective → `USAGE` reports
  ([[openadr-spec-use-cases]] §8.3, [[milp-planner]]).
- **Expectations:** VTN expects nothing *guaranteed* — response is voluntary, settlement
  happens through the meter bill. VEN expects prices delivered reliably and on schedule.
  Pure tariff programs can be **open to anyone without credentials and without reports**
  (User Guide §6.1) — there is nothing secret about public prices.

### 2. Critical Peak Pricing / Load-shed program (User Guide §8.2)

- **VTN view:** "I'm a utility facing ~10 stress days per year (heat waves). Building
  peaker plants for 40 hours/year is absurdly expensive. Instead I enroll customers who
  agree to shed load when I call an event, in exchange for a discounted base rate or
  per-event payments. When stress hits, I create a `SIMPLE` level or CPP price event
  for 16:00–20:00 tomorrow."
- **VEN view:** "I accepted a slightly binding deal: most of the year I pay less; on
  the few event days my HEMS pre-cools the house, delays the dishwasher, and discharges
  the battery during the event window. Automation keeps the comfort loss small."
- **Expectations:** here the VTN *does* expect measurable reduction — this is where
  **baseline vs. actual M&V** matters ([[demand-response]]): metered usage is compared
  against the baseline to verify (and pay for) the shed. The VEN expects advance notice,
  a bounded number of events per year (set in the human contract), and compensation.

### 3. EV managed-charging program

- **VTN view:** "I'm a DSO. Feeders in suburbs with many EVs overload at 18:00–21:00.
  Reinforcing cables costs millions; shifting charging to 01:00–05:00 costs almost
  nothing. I offer a rebate to customers who let me send charging windows or capacity
  limits to their wallbox."
- **VEN view:** "My car sits plugged in 12 hours but only needs 3 hours of charge. I
  don't care *when* it charges, only that it's full by 07:00. Selling that indifference
  earns me €X/month. My HEMS encodes it as a deadline+energy constraint" — this lab's
  `EvSession` model ([[hems-planning]]).
- **Expectations:** VTN expects charging to actually move out of the peak and wants
  `STORAGE_CHARGE_LEVEL`/`USAGE` reports to verify. VEN expects the car is *always*
  full by the deadline — the program must never override that red line.

### 4. Capacity-management / dynamic operating envelope program (User Guide §8.10)

- **VTN view:** "I'm a DSO in a high-PV area. On sunny Sundays my feeder voltage rises
  above limits. Rather than statically capping every inverter at 50% forever, I send
  dynamic `EXPORT_CAPACITY_LIMIT` events only when needed — customers export more
  energy overall than under a static cap."
- **VEN view:** "Joining is often the *condition for connecting* my oversized PV system
  at all. In exchange for accepting occasional export caps, I get to install 15 kWp on
  a feeder that would otherwise only permit 5 kWp. My HEMS turns the limit into a hard
  MILP constraint and diverts surplus into the battery/heater" — exactly what this lab
  implements ([[tariffs-and-capacity]], [[openadr-interface]]).
- **Expectations:** VTN expects hard compliance (grid-safety, not a financial nudge)
  and may require capacity-reservation reports. VEN expects limits only when genuinely
  needed and maximal export freedom otherwise.

### 5. Battery/VPP dispatch program (User Guide §8.5/§8.12 direction)

- **VTN view:** "I'm an aggregator bidding a fleet of home batteries into the balancing
  market as a virtual power plant. I need to *dispatch* — send `DISPATCH_SETPOINT`
  telling batteries to discharge 2 kW for 15 min — and I need state-of-charge reports
  to know what capacity I can bid."
- **VEN view:** "My battery earns money doing nothing most of the day. I lease its idle
  capacity to the aggregator for a monthly fee; my HEMS reserves a SoC band for my own
  use and lets the aggregator control the rest."
- **Expectations:** tightest coupling of all: VTN expects near-real-time obedience and
  telemetry; VEN expects a guaranteed private SoC reserve and payment. In this lab
  `DISPATCH_SETPOINT` is unhandled ([[openadr-spec-use-cases]] §8.5 ❌).

## Why a VEN joins multiple programs

The spec states it directly (User Guide §6.4): *"A provider might offer several programs
at the same time, such as a dynamic pricing program that executes concurrently with a
load shed program. A single customer may be enrolled in multiple programs simultaneously,
e.g. a battery program and an EV program."* Two reasons stack:

1. **Programs monetize different assets or different value streams.** The dynamic tariff
   (example 1) prices your *energy*; the EV program (example 3) pays for *when* you
   charge; the export-envelope program (example 4) is the price of your *grid
   connection*. These are orthogonal deals — often from different actors (retailer, DSO,
   aggregator). A household enrolled in only one leaves the other revenue streams on the
   table.
2. **The one-event-one-program rule forces separation even from a single provider.**
   A provider running both continuous pricing and episodic emergency shed *must* model
   them as two programs — payload types, cadence, and obligations differ. The VEN joins
   both and its planner merges the signals (in this lab: all events land in one
   `TariffSnapshot`/capacity state and one MILP — [[openadr-interface]] →
   [[milp-planner]]).

## Enrollment: out-of-band, contractual, long-lived

The protocol deliberately **does not define joining**. User Guide §6.2: *"OpenADR 3
assumes an enrollment or registration process has happened prior to interactions of a
VEN with a VTN… The OpenADR 3 standard does not define how this process is implemented…
every energy provider must develop their own process."*

- **Enrollment is a business relationship first** — sign-up on the provider's web page,
  app, or paper, tied to customer account, billing, and metering. Only afterwards does
  the provider hand over (User Guide §5.3, §6.2): the **program description** (human
  rulebook), the **VTN base URL**, a **clientID + API credentials**, and
  **ven/resource IDs**. Credential mechanics and object privacy: [[openadr-security]].
- **There is no in-protocol "join program" call.** `GET /programs` is *discovery*, not
  enrollment — the VEN reads which programs its credentials give access to and selects
  the applicable one (User Guide §5.4). Access is governed by credentials and targeting
  set up at enrollment time.
- **Duration:** as long as the underlying commercial relationship — typically a contract
  year or an open-ended subscription with notice period, matching the "once a year or
  less" metadata cadence. Not per-day, not per-event. Leaving is again out-of-band; the
  protocol-visible effect is that events stop or credentials are revoked.
- **The one exception is open tariff programs** (User Guide §6.1): public price feeds
  may require *no* enrollment and no credentials — anyone can point a VEN at the URL and
  read prices. No reports flow back because the VTN doesn't know the VEN exists. That is
  the only genuinely "spontaneous" case the spec anticipates.

**Expectation asymmetry in one line:** the VTN offers a program because flexibility is
cheaper than infrastructure (peakers, cable reinforcement, balancing purchases); the VEN
joins because its flexibility is worthless unexposed; the program object is the standing
agreement that gives both sides a stable vocabulary.

## Where this lab sits

- Program **discovery** is implemented: the VEN polls `GET /programs` every
  `POLL_PROGRAMS_SECS` (default 300 s, `VEN/src/tasks/poll_programs.rs`) into shared
  state; events are polled and interpreted per [[openadr-interface]].
- Program **enrollment** is simulated by hand-authored profile YAML with baked-in
  `clientID`/`clientSecret` — no BL-driven onboarding flow
  ([[openadr-spec-use-cases]], Customer row).
- `OadrProgramConfig` (`VEN/src/entities/capacity.rs`) is quarantined design vocabulary
  for per-program VEN configuration (signal types, capacity participation), not current
  behaviour — tracked in `docs/BACKLOG.md` (BL-14 family).

Q&A form of this material, as originally filed: [[openadr-programs-explained]].
