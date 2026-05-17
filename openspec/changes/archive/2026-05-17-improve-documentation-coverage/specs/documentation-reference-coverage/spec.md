## ADDED Requirements

### Requirement: Glossary section exists before §1
DOCUMENTATION.md SHALL contain a §0 Glossary section before the Purpose & Overview section. It MUST define all domain actors (VEN, VTN, BFF, DSO, TSO, Aggregator, Prosumer), key domain entities (`EnergyPacket`, `FlexibilityEnvelope`, `UserRequest`, `AssetLedger`), and the sign convention (positive = import from grid, negative = export to grid) with an ASCII energy-flow diagram.

#### Scenario: Glossary is present and complete
- **WHEN** a reader opens DOCUMENTATION.md
- **THEN** they find a §0 Glossary section before §1 that lists VEN, VTN, BFF, DSO, TSO, Aggregator, Prosumer with one-sentence definitions
- **AND** the sign convention is stated explicitly with the rule "positive = import from grid"
- **AND** an ASCII diagram illustrating energy flow direction is included

#### Scenario: Sign convention is findable without reading the full doc
- **WHEN** a reader searches DOCUMENTATION.md for "sign convention"
- **THEN** the search leads directly to §0 Glossary

---

### Requirement: Packet role is explicitly stated
DOCUMENTATION.md SHALL contain a paragraph clarifying that energy packets are intent-tracking and reporting metadata, not MILP scheduling variables. The paragraph MUST appear in §2 or §4 near the first mention of energy packets.

#### Scenario: Contributor understands packet role
- **WHEN** a contributor reads the section describing energy packets
- **THEN** the text explicitly states that MILP decision variables drive the schedule
- **AND** the text explicitly states that packets track intent and feed reporting only

---

### Requirement: Functional requirement codes appear inline
DOCUMENTATION.md §2 feature sections SHALL include inline FR code references in parentheses (e.g., `(FR-SIM-03)`) so features are traceable to `docs/REQUIREMENTS.md §4`. Each feature subsection (§2.1 through §2.10) MUST reference at least one FR code.

#### Scenario: Feature is traceable to requirements
- **WHEN** a reader finds a feature description in §2
- **THEN** they can see at least one FR code reference in parentheses
- **AND** they can look up the full requirement text in docs/REQUIREMENTS.md §4

---

### Requirement: Time-series alignment documented in §2.11
DOCUMENTATION.md SHALL contain a §2.11 Time-Series Architecture subsection. It MUST explain: the `TimeSeries<T>` abstraction, the `Interpolation` enum (`Step` / `Linear`), how tariff boundary alignment works, per-interval capacity flattening, and slot classification (`FIRM` vs `FLEXIBLE`). The section SHALL end with a reference link to `docs/architecture/VEN_ARCHITECTURE.md §5`.

#### Scenario: Developer understands tariff query semantics
- **WHEN** a developer reads §2.11
- **THEN** they understand that `Step` interpolation holds the last value across the interval until the next boundary
- **AND** they understand that `FIRM` slots cannot be changed by the planner once committed

#### Scenario: Capacity flattening is explained
- **WHEN** a developer reads §2.11
- **THEN** they understand that per-interval capacity limits are flattened (taking the minimum) across overlapping event intervals

---

### Requirement: Heater min_run_slots and min_off_slots in §5 config reference
DOCUMENTATION.md §5 heater profile table SHALL include `min_run_slots` and `min_off_slots` parameters with their descriptions, units (number of 5-minute slots), and defaults. §2.4 heater description SHALL also mention these parameters.

#### Scenario: Operator configures heater cycling constraints
- **WHEN** an operator reads the §5 heater config table
- **THEN** they find `min_run_slots` and `min_off_slots` with descriptions and defaults
- **AND** they understand the unit is number of 5-minute planning slots

---

### Requirement: Reference links at the end of major sections
DOCUMENTATION.md SHALL end each major §2 subsection (§2.1 through §2.12) and each major §4 subsection (§4.1 through §4.7) with a `> **Reference:** [Document name](relative/path)` blockquote pointing to the canonical architecture document for that topic.

#### Scenario: Reader can navigate from doc to architecture source
- **WHEN** a reader finishes reading any §2 or §4 subsection
- **THEN** they find a reference blockquote at the bottom linking to the relevant architecture doc
- **AND** the link is a relative path that resolves from the DOCUMENTATION.md location
