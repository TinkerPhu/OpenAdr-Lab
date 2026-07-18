## ADDED Requirements

### Requirement: Top nav groups tabs by usage frequency
The app SHALL render a primary nav bar with Dashboard, Devices, Controller,
History, Planner, and Notifications, plus two always-visible grouped menus:
"VTN Feed" (Reports, Programs, Events) and "Diagnostics" (Metrics, Raw Data,
Tasks, Event Log).

#### Scenario: Primary tabs are directly visible
- **WHEN** the app renders
- **THEN** Dashboard, Devices, Controller, History, and Planner links are
  visible in the top-level nav without opening any menu

#### Scenario: VTN Feed tabs are grouped behind a menu
- **WHEN** the app renders
- **THEN** Reports, Programs, and Events links are not visible until the "VTN
  Feed" menu is opened
- **AND** opening the "VTN Feed" menu reveals all three links

#### Scenario: Diagnostics tabs are grouped but never hidden behind a mode flag
- **WHEN** the app renders
- **THEN** the "Diagnostics" menu button is visible unconditionally (no
  settings flag disables it)
- **AND** opening it reveals Metrics, Raw Data, Tasks, and Event Log links

### Requirement: Dashboard shows three traffic-light status rows
The Dashboard SHALL render three status rows — VTN Connection, Plan status,
and Active tasks — each collapsed to a single-line healthy state by default
and expanding inline with detail only when degraded.

#### Scenario: VTN Connection row is a single green line when connected
- **WHEN** `/vtn/status` reports `connected: true`
- **THEN** the row shows a single line indicating connected status with no
  expanded detail

#### Scenario: VTN Connection row expands with detail when disconnected
- **WHEN** `/vtn/status` reports `connected: false` with a `last_error` and
  `current_backoff_s`
- **THEN** the row shows a degraded state and, when expanded, displays the
  backoff duration and last error

#### Scenario: Plan status row is neutral when no plan exists yet
- **WHEN** no plan has been adopted yet
- **THEN** the row shows a neutral waiting state, not a degraded/red one

#### Scenario: Plan status row is degraded only when the plan is infeasible
- **WHEN** the active plan's `solve_status` is `INFEASIBLE`
- **THEN** the row shows a degraded state

#### Scenario: Plan status row is healthy when the plan is optimal
- **WHEN** the active plan's `solve_status` is `OPTIMAL`
- **THEN** the row shows a single healthy line with how long ago it was solved

#### Scenario: Active tasks row summarizes healthy tasks as one line
- **WHEN** every task in `/tasks/status` has `restart_count === 0`
- **THEN** the row shows a single line "N/N running" with no expanded detail

#### Scenario: Active tasks row expands to list unhealthy tasks
- **WHEN** at least one task in `/tasks/status` has `restart_count > 0`
- **THEN** the row shows a degraded state and, when expanded, lists each
  unhealthy task's name and restart count
