docker: docker runs on ssh Pi4-Server. run all tasks with docker on Pi4-Server via ssh in directory /srv/docker/openadr_lab.

dto: avoid DTO normalization. pass through upstream field names (e.g. OpenADR spec names) across all layers — backend, BFF, UI. one vocabulary everywhere reduces boilerplate and debugging friction.

workflow: 1. always keep a project_journal.md in projects where you write for each large step what you did, why you did it and what issues/key-learnings you had. it shall explain, how the project was implemented. The journal lives at docs/history/project_journal.md.
2. write key learnings into KEY_LEARNINGS.md (at docs/reference/KEY_LEARNINGS.md) and consider them when making decissions.

NEVER stop docker containers that are not involved in this project without asking. They are productive containers. 

When researching about OpenADR reference, only use OpenADR 3 resources. General Questions can be researched from any versions.

Do not add co-authoring footers to commit messages or PR descriptions. they might get rejected.

Only consider upstream PR and commits after the code is tested completely without failure and the commits are ready for the upstream CI acceptance tests.
After creating upstream PR, wait for the CI to actually run and report before drawing any conclusions about main branch being pre-broken. If anything fails, we investigate it properly rather than writing it off.

test failures: NEVER dismiss test failures as "pre-existing" or "unrelated" without verifying. Always read the actual error message and check whether our changes could have caused it (e.g. a backend response shape change breaking UI code). Only classify as pre-existing after confirming the same failure exists on the base branch. The cost of investigating a real regression is far lower than the cost of shipping one.

docs/specs/pdf/: do not read, search, or reference any files under this directory. Use the markdown versions in docs/specs/ instead.

naming: variables and function names for physical quantities must include the unit as suffix (e.g. `power_kw`, `energy_kwh`, `temperature_c`, `tariff_eur_per_kwh`, `soc_pct`). When adding new code, check nearby code or nearby source files for existing suffixes to stay consistent.

ven-architecture: VEN/src/ follows Hexagonal + Clean Architecture. Dependency rule: inner rings NEVER import outer rings.

  Ring map (outer → inner):
    Adapters   : routes/, tasks/
    Application: services/
    Domain     : entities/, controller/
    Infra      : simulator/, vtn.rs, controller/milp/

  Port obligations — use traits, never bypass with concrete types:
    SimulatorPort    : domain/services → simulator (snapshot, inject)
    SolverPort       : services → controller/milp (solve)
    VtnPort          : services → vtn.rs (fetch programs/events/obligations)
    AssetMilpContext : milp_planner accepts Vec<Box<dyn AssetMilpContext>> — NEVER import A_BAT/A_EV/A_HTR directly

  Profile rule: no `use crate::profile` in entities/, controller/, or routes/. Profile values are
  injected as typed parameter structs (e.g. BatteryParams) constructed in the application/infra layer.

  File size: no VEN/src/ file > 500 lines. tasks/ files must stay < 200 lines.

  Verifiable invariants — run before any VEN PR:
    grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes  → must be empty
    grep -r "use crate::assets::" VEN/src/controller/milp                             → must be empty
    grep "serde_json::Value" VEN/src/vtn.rs                                           → must be empty or internal only

  Reference: docs/plans/ven_backend_architecture_refactoring.md