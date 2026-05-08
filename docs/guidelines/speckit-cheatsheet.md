## Core commands (main flow, in order)

- /speckit.constitution – Create or update project governing principles and development guidelines.[github]
- /speckit.specify – Define what you want to build (requirements and user stories).[github]
- /speckit.plan – Create technical implementation plans with your chosen tech stack.[github]
- /speckit.tasks – Generate actionable task lists for implementation.[github]
- /speckit.implement – Execute all tasks to build the feature according to the plan.[github]

## Optional commands (quality / refinement, with intended position)

- /speckit.clarify – Clarify underspecified areas, recommended before /speckit.plan (formerly /quizme).[github]
- /speckit.analyze – Cross‑artifact consistency and coverage analysis, run after/speckit.tasks, before /speckit.implement.[github]
- /speckit.checklist – Generate custom quality checklists to validate requirements completeness, clarity, and consistency.[github]
“Ideal” end‑to‑end order including optionals

1. /speckit.constitution
2. /speckit.specify
3. /speckit.clarify (optional, here)
4. /speckit.plan
5. /speckit.tasks
6. /speckit.analyze (optional, here)
7. /speckit.checklist (optional, can be run once you have a spec/plan/tasks)
8. /speckit.implement

That matches the “Available slash commands” section you linked to.