# Purpose — OpenADR Lab Wiki



## What this project is

OpenADR Lab is my lab for learning and prototyping **OpenADR 3 demand response** end to end:
one VTN and up to three VENs on a Raspberry Pi 4 (more in the future to study swarm behaviour), 
where each VEN models a residential site with a HEMS controller managing simulated assets 
(battery, EV charger, heater). The VEN plans with a 3-tier variable-step **MILP optimizer** (HiGHS) 
and dispatches setpoints in a 1-second real-time loop. 
It is not production-grade, but it deliberately follows production-inspired
patterns: hexagonal/clean architecture, ports, injectable clocks, a four-layer test pyramid,
BDD E2E suites.

## Why the wiki exists

The repository already has good *documents* (`docs/`, `specs/`, `openspec/`), but no single
place where the **code reality, the domain, the decisions, and the vision** are connected.
The wiki is that place. Its two audiences:

1. **Me** — to re-orient quickly, spot contradictions between docs and code, and keep the
   big picture coherent as the system grows.
2. **Future AI sessions** — durable, pre-synthesized context so each session doesn't
   re-derive the architecture from scratch. The wiki is context infrastructure.

## What to emphasize

- **Code over prose**: when docs and code disagree, the code is the fact and the wiki flags
  the drift. BDD features (`tests/features/`) are executable use-case documentation — treat
  them as first-class sources.
- **Why, not just what**: decisions (why MILP over greedy, why hexagonal rings, why HiGHS,
  why DTO pass-through) matter more than restating structure. Mine `specs/`,
  `docs/history/project_journal.md`, and commit history for them.
- **OpenADR 3 correctness**: map lab concepts to spec concepts precisely (programs, events,
  reports, resources). Long-term interests: OpenADR certification readiness and upstream
  contributions to `openleadr-rs`. Note that the OpenADR specs in the project folder are 
  actually for version 3.1 which has breaking changes to the current 3.0 implementation. 
  The migration of the project to 3.1 is a distant goal.
- **The energy domain**: HEMS planning, flexibility, tariffs, sign conventions — the wiki
  should make the domain understandable, not only the software.
- **OpenADR use cases**: suggest use cases implied in the OpenADR specifications. 
  Analyze if they can be handled with the current code base or if not, suggest new features 
  that would enable those use cases.

## Non-goals

- No duplication of `docs/` content — synthesize and link.
- No speculative content about features that don't exist; unbuilt ideas belong in
  `docs/BACKLOG.md`, not here (the wiki may *link* to backlog themes in overview pages).
- The wiki never becomes a second spec: `docs/REQUIREMENTS.md` stays the vocabulary
  authority.
