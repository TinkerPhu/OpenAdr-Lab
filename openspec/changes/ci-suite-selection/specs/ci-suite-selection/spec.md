# Spec Delta

## Purpose
Defines which test suites the CI runner runs for a handed-off change, so that a change is not
tested with suites it demonstrably cannot affect, without ever skipping a suite it might affect.

## ADDED Requirements

### Requirement: Suites are selected from the changed paths
The runner SHALL derive the set of suites to run from the paths changed between `origin/main` and
the rebased commit, using one declared path-to-suites map. The selected set is the union over all
changed paths.

#### Scenario: A change confined to the VEN code
- **WHEN** every changed path lies under `VEN/src/`
- **THEN** the VEN cargo tier, E2E and resilience run, and the openleadr-rs cargo tests do not

#### Scenario: A change confined to a UI directory
- **WHEN** every changed path lies under `VEN/ui/`, `VTN/ui/` or `lab-ui/`
- **THEN** E2E runs, and the VEN cargo tier, the openleadr-rs tests and resilience do not

#### Scenario: Mixed change
- **WHEN** the changed paths select different suites
- **THEN** the union of those suites runs

### Requirement: Unknown paths run everything
A changed path that no rule of the map matches SHALL select every suite.

#### Scenario: A new top-level directory
- **WHEN** a change touches a path no rule names
- **THEN** all suites run

### Requirement: The always-on checks are not selectable
The audits and the CI mechanism's own tests SHALL run for every ticket regardless of the map.

#### Scenario: Docs-only change
- **WHEN** every changed path selects no suite
- **THEN** only the audits and the CI mechanism tests run, and a `merge` ticket may merge

### Requirement: Skips are reported
The runner SHALL record, per ticket, the suites it ran, the suites it skipped, and the changed paths
that decided it, in `result.md`; the CI web board SHALL show them.

#### Scenario: Ticket result
- **WHEN** a ticket finishes
- **THEN** `result.md` lists suites run, suites skipped and the deciding paths, and `/api/tickets` exposes them

### Requirement: A selected-suites pass is a merge pass
A `merge` ticket SHALL merge when every selected suite passed on the rebased commit.

#### Scenario: Skipped suite does not block
- **WHEN** the openleadr-rs tests are skipped and all selected suites pass
- **THEN** the ticket merges

### Requirement: Skipping is audited
The repository SHALL carry a recurring job that runs the full suite on `main`, so a wrong skip is
found by a scheduled run rather than by a later ticket.

#### Scenario: Job due
- **WHEN** the interval of the job has passed
- **THEN** `python scripts/jobs.py due` lists it
