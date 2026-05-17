## ADDED Requirements

### Requirement: MILP formulation chapter exists as §2.12
DOCUMENTATION.md SHALL contain a §2.12 MILP Formulation subsection (and the TODO on line 34 SHALL be removed). The section MUST include: a decision-variable table, the Phase 1 objective function, the Phase 2 objective function, a constraint summary table, and a plain-language explanation of the two-phase independence mechanism (`c_star`).

#### Scenario: New contributor understands what the planner optimises
- **WHEN** a contributor reads §2.12
- **THEN** they find a table listing all MILP decision variables with names, units, and bounds
- **AND** they understand Phase 1 minimises energy cost and Phase 2 minimises operational friction
- **AND** the TODO placeholder on line 34 of DOCUMENTATION.md is no longer present

#### Scenario: Two-phase independence is explained
- **WHEN** a contributor reads §2.12
- **THEN** the text explains that `c_star` (the Phase 1 optimal cost) is added as a hard equality constraint in Phase 2
- **AND** the text explains that this guarantees Phase 2 cannot degrade the cost-optimal outcome while reducing friction

---

### Requirement: Decision variable table is complete and accurate
The §2.12 decision variable table SHALL list at minimum: power setpoint variables per asset, binary on/off relay variables for the heater, delta (switching) variables for the heater relay schema, and import/export grid power variables. Each variable MUST show its symbol, description, unit, and feasible range.

#### Scenario: Variable table covers all asset types
- **WHEN** a contributor reads the decision variable table in §2.12
- **THEN** they find variables for battery, EV, heater (including relay and delta variables), PV, and grid
- **AND** each row shows symbol, description, unit, and bounds

---

### Requirement: Constraint families are summarised
§2.12 SHALL include a constraint summary table listing the major constraint families: power balance (site-level Kirchhoff), asset capability bounds, heater relay logic (mutual exclusivity, min-run, min-off), SoC continuity (battery and EV), VTN import/export limits, and Phase 2 cost-lock (`c_star` equality). Each constraint family MUST have a one-line description of what it enforces.

#### Scenario: Contributor can identify which constraint to modify for a new feature
- **WHEN** a contributor needs to add a new DR signal type
- **THEN** they can find the VTN import/export limits constraint family in §2.12 and understand what it enforces
- **AND** they can navigate to the source file via the reference link at the end of the section

---

### Requirement: MILP chapter ends with reference link
§2.12 SHALL end with a `> **Reference:**` blockquote linking to `docs/architecture/VEN_ARCHITECTURE.md` (two-phase MIP) and `docs/architecture/heater_tank_milp_planning_model.md` (heater relay schema).

#### Scenario: Reader can navigate to full MILP specification
- **WHEN** a reader finishes §2.12
- **THEN** they find reference links to both VEN_ARCHITECTURE.md and heater_tank_milp_planning_model.md
