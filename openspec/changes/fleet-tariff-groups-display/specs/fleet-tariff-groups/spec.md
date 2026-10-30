# Spec Delta

## Purpose
What the Fleet page and the signals feed say about the tariff each VEN is under, so the tariff drawn next to
the fleet power curve is the one the VENs act on, also when VENs are priced differently.

## ADDED Requirements

### Requirement: The signals feed lists only the winning band per VEN
`GET /api/fleet/signals` SHALL return, for each VEN and each payload type, the bands of the event that wins
wherever events targeting that VEN overlap in time, using the ranking the VEN applies: lower `priority` number
wins, an absent priority ranks last, equal priority is decided by the newer `createdDateTime`. A band SHALL
NOT be returned where another event outranks it.

#### Scenario: A priority-1 event overrides an event without priority
- **WHEN** an unprioritised event sets PRICE 0.25 for 08:00-09:00 and a priority-1 event targeting ven-3 sets
  PRICE 0.12 for the same hour
- **THEN** ven-3's bands for that hour carry only 0.12 and every other VEN's carry 0.25

#### Scenario: A partial overlap splits the lower event
- **WHEN** a priority-1 price covers 08:20-08:40 inside an unprioritised price for 08:00-09:00
- **THEN** the VEN's bands are 08:00-08:20 low-priority, 08:20-08:40 priority-1, 08:40-09:00 low-priority

#### Scenario: Different payload types do not outrank each other
- **WHEN** a priority-1 event carries only PRICE and a lower event carries PRICE and EXPORT_PRICE
- **THEN** the PRICE band comes from the priority-1 event and the EXPORT_PRICE band from the lower one

#### Scenario: The feed and the VEN agree
- **WHEN** the same events are resolved by the signals feed and by the VEN's tariff parser
- **THEN** both name the same event as the winner for every instant

### Requirement: One tariff curve per distinct tariff
The Fleet page's tariff card SHALL draw one import curve and one export curve for every distinct set of
price bands among the VENs, and SHALL NOT drop a set because fewer VENs share it. VENs with identical
PRICE and EXPORT_PRICE bands over the window SHALL be one tariff, whichever event ids carry them. The import
curves SHALL share one chart and the export curves a second chart, and a tariff SHALL have the same colour in
both.

#### Scenario: Five tariffs
- **WHEN** 20 VENs fall into five distinct price patterns
- **THEN** the import chart has five curves and the export chart five curves, each pair in one colour

#### Scenario: One tariff for everybody
- **WHEN** all VENs have identical price bands
- **THEN** the card shows one import curve and one export curve, labelled with all of the VENs

#### Scenario: VENs without a price are not a tariff
- **WHEN** a VEN has no PRICE band in the window
- **THEN** it belongs to no curve and the card counts it as not priced

#### Scenario: Many tariffs
- **WHEN** more than eight distinct tariffs exist
- **THEN** the eight with the most VENs are drawn and the card names how many more exist and their VENs

### Requirement: Each curve names its VENs
Each tariff curve SHALL carry a label made of a short name derived from the event name of its winning PRICE
band and the number of VENs on it, and SHALL list its VENs on hover. When several tariffs have event names
that share a leading part, the shared part SHALL be left out of the short names. A tariff whose name is not
available SHALL be labelled by its VEN count alone.

#### Scenario: Short names from shared event-name prefix
- **WHEN** five tariffs come from events named `exp-flat-PRICE-g0` to `exp-flat-PRICE-g4`
- **THEN** their labels read `g0 · 1 VEN`, `g1 · 4 VENs` and so on, and the full event name is on hover

#### Scenario: Hover lists the VENs
- **WHEN** the pointer is over the label or the curve of a tariff with six VENs
- **THEN** the six VEN names are shown in the usual order of the Fleet page

### Requirement: A deliberate split is not reported as disagreement
The tariff card SHALL NOT describe VENs on different tariffs as dissenting or differing from "the" tariff.
The card SHALL still show how many known VENs are on drawn tariffs.

#### Scenario: Targeted tariffs
- **WHEN** five targeted events price five VEN groups differently
- **THEN** no notice about VENs having been sent a different tariff is shown, and the card reads "20 of 20 VENs"

### Requirement: The tariff card shows the tariff as the VTN holds it now
The tariff card SHALL draw the events as they are on the VTN at the time of the request, for the whole window,
and SHALL say so. It SHALL NOT claim to show the tariff a VEN had adopted at a past time.

#### Scenario: The card states its basis
- **WHEN** the tariff card is shown
- **THEN** its caption says the curves are the VTN's events as of now
