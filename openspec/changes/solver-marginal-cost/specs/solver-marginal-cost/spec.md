## ADDED Requirements

### Requirement: SolverPort exposes a per-slot marginal-cost shadow price
The system SHALL compute, once per planning cycle after the winning MILP solve, a per-slot
shadow price on the power-balance constraint by re-solving the same problem as a pure LP with all
binary decisions fixed to the winning solution's values, and SHALL expose it on `PlanTimeSlot` as
`marginal_cost_import_eur_per_kwh` / `marginal_cost_export_eur_per_kwh`.

#### Scenario: No asset or grid constraint is binding
- **WHEN** a slot's plan has no asset at a power/energy limit and no active capacity constraint
- **THEN** `marginal_cost_import_eur_per_kwh` SHALL equal that slot's `import_tariff_eur_kwh`
  within solver tolerance

#### Scenario: An asset is pinned at a binding power limit
- **WHEN** a slot's plan has an asset (e.g. battery) operating at its maximum power bound
- **THEN** `marginal_cost_import_eur_per_kwh` SHALL differ from the plain `import_tariff_eur_kwh`,
  reflecting the additional cost of the binding constraint

#### Scenario: The dual LP solve fails
- **WHEN** the second (binaries-fixed) LP solve returns an error
- **THEN** the planning cycle SHALL NOT fail — both marginal-cost fields SHALL fall back to the
  slot's plain import/export tariff, and a warning SHALL be logged

#### Scenario: Persisted plans predating this change deserialize cleanly
- **WHEN** a `Plan` JSON blob written before this change (no marginal-cost fields) is deserialized
- **THEN** both fields SHALL default to `0.0` via `#[serde(default)]`, not fail deserialization

### Requirement: The shadow price does not affect planning decisions
The system SHALL treat the marginal-cost computation as a read-only diagnostic: it SHALL NOT
influence `p_imp`/`p_exp`, any asset allocation, or any other field already produced by the winning
MILP solve.

#### Scenario: Identical plan decisions with and without the dual solve
- **WHEN** the dual LP solve is skipped (e.g. by returning an error) and the tariff fallback is
  used instead
- **THEN** every other field of the resulting `Plan` (allocations, `net_import_kw`, costs, etc.)
  SHALL be identical to a run where the dual solve succeeds

### Requirement: VEN UI surfaces the marginal cost per slot
The VEN UI's Planner "Decision Matrix" SHALL display the per-slot marginal cost alongside the
existing tariff row, satisfying the project's ui-transparency rule for this newly-derived state.

#### Scenario: Decision matrix renders the marginal-cost row
- **WHEN** a plan with populated `marginal_cost_import_eur_per_kwh` values is loaded
- **THEN** the Decision Matrix SHALL render one heatmap cell per slot for the marginal cost, with
  a tooltip showing both the marginal cost and the plain tariff for that slot
