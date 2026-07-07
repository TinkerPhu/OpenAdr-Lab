---
title: OpenADR Programs
type: concept
created: 2026-07-06
updated: 2026-07-07
synced_commit: 466f792
sources:
  - docs/openadr_3_1_specs/
  - VEN/src/tasks/poll_programs.rs
  - VEN/src/entities/capacity.rs
  - openleadr-rs/openleadr-wire/src/program.rs
tags: [openadr, program, enrollment, dr, spec, authorization]
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

## What the Program object is actually *for* — four purposes, not one

"Interpretation of events" is the most visible purpose but not the only one — the spec
gives Program at least four distinct jobs, and authorization is arguably more
load-bearing in the protocol than interpretation:

1. **Semantic namespace.** A program's own `payloadDescriptors` list "provides default
   values for all events associated with a program" (User Guide §8.3) — the program
   pre-declares what payload types its events will use, so events don't have to.
2. **Authorization / entitlement boundary — separate from interpretation.** Definitions
   doc, Security section: *"Within the context of a given program, a VEN will be
   authorized to access some set of resources and associated operations."* More
   bluntly: *"a Business Logic client may create an event, but a VEN cannot. Both can
   read an event, but a VEN can only read events associated with the programs it is
   entitled to access."* Programs (and events) are also the objects that carry
   `targets`: BL grants targets to a `ven` object, then attaches matching targets to a
   program/event to gate read access at all (Definitions doc, "program and event
   objects - targeting"). This is pure access control, independent of what the events
   *mean* — see [[openadr-security]].
3. **Commercial/business container.** Program's own definition is *"the business
   context for a given usage of the VTN"* (Definitions doc, Terms and Definitions). It
   carries retailer identity, country/subdivision, program type, `bindingEvents`
   (whether events are fixed once transmitted), `localPrice` (whether values were
   adapted by a local VTN) — regulatory/commercial metadata about the offering, not
   payload-decoding hints.
4. **Discovery anchor and structural prerequisite.** `GET /programs` is how a VEN/CL
   discovers what's on offer (User Guide §5.4, §6.4). Structurally, *"a program object
   must be present on the VTN before an event may be created, as event must refer to an
   existing program"* (§8.13) — a hard foreign-key dependency, not just a semantic hint.

## Is the Program object meant to be seen by the customer, or only the VEN?

Not VEN-only — but not required to be customer-facing either. Two separate artifacts
are easy to conflate:

- **The Program Description** — a human-readable document handed over out-of-band at
  enrollment (*"specifies a usage of the OpenADR 3 object model and configuration
  details such as VTN address, program names, applicable customer types, etc."*,
  Definitions doc, Terms and Definitions). Not a protocol object; this is the primary
  "what am I signing up for" artifact.
- **The Program object** in the VTN — primarily consumed by the VEN's automation logic,
  but the spec leaves the door open: *"Some fields in the program object may be
  displayed to persons using a VEN via a VEN provided user interface, but this feature
  is not required"* (User Guide §6.4). `programName` itself is defined as *"a unique
  name for a program or tariff. May be used by customers"* (Definitions doc). Table 8
  "Program Attribute Enumerations" includes `PROGRAM_LONG_NAME` and
  `RETAILER_LONG_NAME`, both defined explicitly *"...for human readability"* — the
  schema deliberately carries display strings. `Customer Logic (CL)` is defined as
  logic that "may provide human facing features to support configuration and
  monitoring," and the Customer user stories include *"As CL, I want to read a list of
  programs or an individual program, so that I have the context necessary to understand
  what DR programs exist, or select the one appropriate to me"* (User Guide §5.3).

openleadr-rs (`openleadr-rs/openleadr-wire/src/program.rs`) reflects this by promoting
the Table 8 attributes to first-class typed `Option<String>` fields on `ProgramContent`
(`program_long_name`, `retailer_name`, `retailer_long_name`, `program_type`...) rather
than leaving them buried in the generic `attributes: valuesMap` list the raw schema
technically allows — a signal that a real implementation treats human-readability as a
normal, expected part of the object, not a fringe feature.

## Many events pointing at one program — is that pattern actually used?

Yes, and it is the *normal* case, not an edge case — a program is designed to long
outlive any single event.

- **Structural rule:** every event carries a `programID` foreign key (§6.4, enforced by
  the "program must exist before event can be created" rule in §8.13).
- **Reuse is the point.** `programName` uniqueness per VTN instance means you don't mint
  a new program per event — you create the program once and post events against it
  repeatedly.
- **Worked example in the spec itself:** §8.10.2 "Dynamic Capacity Management" walks
  through a *single* program under which BL posts a `capacity_subscription_Event`, then
  later a `capacity_reservation_Event`, then a `capacity_available_Event`, then further
  reservation-grant events over time — distinct events, same program, unfolding as an
  ongoing relationship. The worked examples across User Guide §8.2–§8.12 (CPP event,
  simple event, pricing event, inverter/curve event, load-control event, fast-DR event,
  setpoint event, capacity events) all reuse the same placeholder `"programID": "44"` —
  illustrating that one program is meant to host a whole family of event kinds over its
  lifetime.
- **Clearest real-world instance:** the dynamic-tariff case (example 1 below). One
  `Program` object ("day-ahead hourly tariff") persists for the life of the contract; a
  brand-new `PRICE` event is created every day (or more often) for as long as the
  customer is enrolled — potentially thousands of events against one program over a
  year, matching the "programs change ~yearly, events change daily" split in §6.4.
- **The other multiplicity in the spec is different and independent:** *"A provider
  might offer several programs at the same time... A single customer may be enrolled in
  multiple programs simultaneously"* (§6.4) — many **programs** per VEN (orthogonal
  deals: tariff + EV + capacity envelope), not many events per program. See "Why a VEN
  joins multiple programs" below — both fan-out patterns coexist independently.

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

Q&A form of this material, as originally filed: [[openadr-programs-explained]]. How a
distributor would stage these program types commercially, from open tariff publication
to VPP dispatch: [[distributor-business-case-tiers]].
