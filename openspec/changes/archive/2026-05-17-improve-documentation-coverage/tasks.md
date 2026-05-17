## 1. Preparation — read source files

- [x] 1.1 Read `docs/REQUIREMENTS.md §2` (glossary terms, sign convention) and `§4` (FR codes per feature area)
- [x] 1.2 Read `docs/architecture/VEN_ARCHITECTURE.md §5` (TimeSeries, Interpolation, slot classification, tariff alignment)
- [x] 1.3 Read `docs/architecture/packet_explanation.md` (packet role clarification and semantic shift)
- [x] 1.4 Read `VEN/src/controller/milp_planner/solver_phase1.rs` and `solver_phase2.rs` to extract variable names, objective terms, and constraints
- [x] 1.5 Read `docs/architecture/heater_tank_milp_planning_model.md` for heater relay schema and switching penalty structure
- [x] 1.6 Verify whether `min_run_slots` and `min_off_slots` are exposed as YAML fields in `VEN/src/profile.rs` and `VEN/profiles/*.yaml`

## 2. §0 Glossary and sign convention

- [x] 2.1 Add a `## 0. Glossary` section to `DOCUMENTATION.md` between the table of contents and §1, defining: VEN, VTN, BFF, DSO, TSO, Aggregator, Prosumer, EnergyPacket, FlexibilityEnvelope, UserRequest, AssetLedger
- [x] 2.2 State the sign convention in §0 as a bullet ("positive = import from grid, negative = export to grid") and reproduce the ASCII energy-flow diagram from `docs/architecture/Domain_definitions.md`
- [x] 2.3 Update the table of contents to include §0 Glossary

## 3. Packet role clarification

- [x] 3.1 Add a paragraph to §2 (after the feature overview table or within §2.5 VTN Integration) explicitly stating that energy packets are intent-tracking and reporting metadata, not MILP scheduling variables — the MILP decision variables drive the schedule; packets track intent and feed reporting

## 4. FR code cross-references

- [x] 4.1 Add at least one `(FR-OA-xx)` anchor inline in §2.5 VTN Integration section
- [x] 4.2 Add at least one `(FR-SIM-xx)` anchor inline in §2.1 Simulation Engine section
- [x] 4.3 Add at least one `(FR-ASSET-xx)` anchor inline in §2.4 User Energy Requests or relevant asset section
- [x] 4.4 Add FR anchors inline in §2.2 Energy Planning, §2.3 Deviation Absorption, §2.6 Report Obligations, §2.7 Flexibility Envelope — one per section minimum

## 5. §2.11 Time-Series Architecture

- [x] 5.1 Add a `### 2.11 Time-Series Architecture` subsection after §2.10 Observability in `DOCUMENTATION.md`
- [x] 5.2 Explain the `TimeSeries<T>` abstraction: what it is, how it is indexed by timestamp, and how lookups work
- [x] 5.3 Explain the `Interpolation` enum: `Step` holds the last value until the next boundary; `Linear` interpolates between points
- [x] 5.4 Explain tariff boundary alignment: tariff intervals are UTC-aligned; the MILP slot grid is aligned to the first slot start time
- [x] 5.5 Explain per-interval capacity flattening: overlapping VTN capacity events are merged by taking the minimum (most restrictive)
- [x] 5.6 Explain slot classification: `FIRM` slots cannot be re-planned; `FLEXIBLE` slots can be revised; near-horizon boundary and flat-rate early firm-up conditions
- [x] 5.7 Add `> **Reference:** [VEN_ARCHITECTURE.md §5](docs/architecture/VEN_ARCHITECTURE.md)` blockquote at the end of §2.11
- [x] 5.8 Update the table of contents to include §2.11

## 6. §2.12 MILP Formulation

- [x] 6.1 Add a `### 2.12 MILP Formulation` subsection after §2.11 and remove the TODO placeholder on line 34
- [x] 6.2 Write a decision-variable table with columns: Symbol, Description, Unit, Bounds — covering power setpoints per asset (`p_bat`, `p_ev`, `p_htr`, `p_pv`, `p_grid`), heater binary on/off relay variables, heater delta (switching) variables, and import/export grid split variables
- [x] 6.3 Write the Phase 1 objective: minimise total energy cost over the 24-hour horizon (tariff × import power − export tariff × export power, summed over all slots)
- [x] 6.4 Write the Phase 2 objective: minimise operational friction (battery wear cost, heater startup cost, EV tier penalty) subject to the hard constraint that total cost ≤ `c_star`
- [x] 6.5 Write the constraint summary table with columns: Family, What it enforces — covering: power balance (Kirchhoff), asset capability bounds, heater relay logic (mutual exclusivity, `min_run_slots`, `min_off_slots`), SoC continuity (battery and EV), VTN import/export limits, cost-lock (`c_star` equality)
- [x] 6.6 Add a plain-language explanation of the two-phase independence mechanism: Phase 1 finds the cost-optimal schedule and records `c_star`; Phase 2 re-optimises with `c_star` as a hard equality constraint so it cannot increase cost while reducing friction
- [x] 6.7 Add `> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md) · [heater_tank_milp_planning_model.md](docs/architecture/heater_tank_milp_planning_model.md)` blockquote at the end of §2.12
- [x] 6.8 Update the table of contents to include §2.12

## 7. §5 config reference — heater min_run/off slots

- [x] 7.1 If `min_run_slots` and `min_off_slots` are confirmed as implemented YAML fields (task 1.6): add both to the §5 heater profile table with description "Minimum number of consecutive 5-minute slots the heater must remain on/off after a switch" and their defaults
- [x] 7.2 If not yet implemented YAML fields: add both to the §5 heater table with a note "(planned — not yet a YAML parameter; hardcoded default applies)"
- [x] 7.3 Add a one-sentence mention of `min_run_slots` / `min_off_slots` to §2.4 (User Energy Requests / heater control description) if not already present

## 8. Reference links throughout §2 and §4

- [x] 8.1 Add `> **Reference:** [asset_simulation.md](docs/architecture/asset_simulation.md)` at the end of §2.1 Simulation Engine
- [x] 8.2 Add `> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)` at the end of §2.2 Energy Planning (MILP) and §2.3 Deviation Absorption
- [x] 8.3 Add `> **Reference:** [heater_tank_milp_planning_model.md](docs/architecture/heater_tank_milp_planning_model.md)` at the end of §2.4 User Energy Requests (heater section)
- [x] 8.4 Add `> **Reference:** [VTN_ARCHITECTURE.md](docs/architecture/VTN_ARCHITECTURE.md)` at the end of §2.5 VTN Integration and §2.6 Report Obligations
- [x] 8.5 Add `> **Reference:** [ven_asset_interface_spec.md](docs/architecture/ven_asset_interface_spec.md)` at the end of §2.7 Flexibility Envelope
- [x] 8.6 Add `> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)` at the end of §4.3 Ring Map, §4.4 Module Responsibilities, §4.5 Background Tasks, §4.6 State Management, §4.7 Control Flow

## 9. Final review

- [x] 9.1 Verify all table of contents links resolve to the correct anchors in `DOCUMENTATION.md`
- [x] 9.2 Verify §0 Glossary is the first section after the table of contents
- [x] 9.3 Verify the TODO placeholder (original line 34) is gone
- [x] 9.4 Verify all FR codes added in step 4 exist in `docs/REQUIREMENTS.md §4`
- [x] 9.5 Verify all reference link paths are valid relative paths from DOCUMENTATION.md (which lives at the repo root)
