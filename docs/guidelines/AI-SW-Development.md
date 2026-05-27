# Goals & Principles of Work

- **Highest goal:** Invest maximum effort in getting source code right and correct on the first attempt — corrections are expensive. Therefore, planning is everything. **Remember: accumulated experience, learnings, and rules live in your head, not in AI agents.** Agents are rich in options but lack preferences, and they have no memory between sessions. **Write everything down as explicit instructions.**

- Use AI to support all the practices described below. For example, ask ChatGPT to explain a problem domain and suggest options with pros and cons so you can make a well-informed choice for your project.

- **Re-iterate regularly** (e.g., create a recurring calendar event): as your project grows, revisit and extend all aspects — documentation, architecture, tests, tooling. Use the tools made for this purpose.

---

## Before-Project Setup

Do these steps once, before any project work begins.

- **AI constitution (CLAUDE.md):** Establish baseline settings before writing a single line of code. Key topics to cover:
  - Pre-existing issues are not an excuse to ignore them.
  - Key-learnings workflow.
  - Infrastructure details: WSL, servers, SSH connections, available tools.
  - Tool installation: speckit, superpowers, etc.
  - Git commit, merge, and branching strategy.
  - Test procedures and test categories.
  - Linter and code-coverage usage — including what to do with findings.
  - Build pipeline instructions.
  - Issue tracking and backlog handling.

  → See existing `CLAUDE.md` files in this project as concrete examples.

- **Workspace:** Prepare your workspace for Markdown document reading (e.g. set up md preview, most AI assistants consume `.md` files as context).

- **Remote work on a VM:** SSH sessions drop when the connection is interrupted. Use `tmux` to keep sessions alive. Use `tail -f <file>` to follow logs without blocking the terminal.

- **Define a documentation concept.** Recommended artifacts:
  - Architecture docs
  - User story doc
  - Requirements docs (functional and non-functional)
  - Interface docs
  - Security concept
  - Risk analysis
  - AI workload estimation *(UNTESTED)*

  Add the following to your ongoing workflow:
  - `Backlog.md` or `Tasks.md` (if external platform, enable write access)
  - `Key-Learnings.md`
  - `Project-Journal.md`
  - `Technical-Debts.md`

- **Onboarding a new AI session:** Because AI agents have no memory between sessions, maintain a structured session-start checklist: load CLAUDE.md, read the project journal, read key learnings, check open tasks/backlog, and confirm current branch and working state. This brings a fresh session up to speed in minutes.

- **UNCONFIRMED:** Keeping source files small may strongly improve AI-agent efficiency (avoids god classes and large context windows).

---

## Prompting Recommendations

**Useful prompting vocabulary:** 
multiple options, concise,  comprehensive, deep dive, contradiction, duplication / inline, dead/unused code, violation, missing details, review, creap/bleed in.

**Useful prompting techniques:**
- *"Paraphrase my description so I can confirm I expressed myself correctly."*
- *"Summarize, analyze the issues, and suggest multiple options on how to solve or fix them. (choose the solution that fits the purpose and requirements best and review it once more before implementing it.)"*
- *"do not engage in ... yet/before ..."*
- *"be critical and thorough"*

**Prompt library:** Maintain a living collection of prompts that have proven effective, organized by use case (architecture review, code review, test generation, documentation sync, etc.). Given the document's emphasis on writing everything down, this is a natural extension of the key-learnings workflow.

**Handling AI hallucinations:** AI agents confidently state incorrect things. Always compile and run tests after AI-generated code. Cross-check generated architecture diagrams against actual imports. When in doubt, prompt the AI to cite sources or flag uncertainty.

---

## Project Management / Workflow

### 1. Start with Requirements, User Stories, and Non-Functional Requirements

Capture both what the system must do (functional) and how well it must do it (non-functional): performance, scalability, availability, reliability, security constraints. NFRs often drive architectural choices (caching, async patterns, deployment topology) and must be captured before architecture decisions are made.

### 2. Research Technology and Topology

Understand the problem space before committing to a stack. Use AI to survey multiple options with pros and cons.

### 3. Detail Requirements Engineering

Close ambiguities before they become bugs. Resolve conflicting requirements and missing edge cases at this stage.

### 4. Decide on Architecture, Guidelines, Branching, and Workflow Rules

- Choose an architecture framework (e.g., *Hexagonal Architecture / Ports and Adapters* combined with *Clean Architecture* layering). Define coding guidelines, consequently add UI test IDs to UI components from start, linter rules, and code-coverage targets — write all of this into the constitution.
- Define a **branching model** (GitFlow, trunk-based, feature-branch naming, PR rules). AI agents need explicit rules about what branch to target and when to push.
- Establish **workflow rules** now — before any implementation begins: write the project journal, record key learnings and technical debts, look for inline duplication that can be extracted to utility functions, remove dead code, and keep tools and libraries up to date. These habits cannot be retrofitted after the project is underway.

### 5. Decide on Testing Strategy and Enforce Test-First

- Choose a **testing pyramid architecture:** BDD end-to-end tests (Gherkin/Cucumber), UI tests, API tests, and unit tests. Unit tests are not optional — the test-first process (see below) is almost exclusively applied at the unit level, and a pyramid without a unit-test base contradicts standard testing theory.
- **Test-first:** make it a workflow rule to write tests first, run them to confirm they fail, then implement until they are green.
- **Deterministic reproducibility:** any behavior that depends on the current date or time must be controllable via an injectable clock or configuration parameter. Design for this from the start.

### 6. Plan Phases, Expected Results, and Project-Level Definition of Done

- Break the project into phases. For each phase, prepare tests or contracts that define what "done" means.
- Define a **project-level definition of done** — the criteria for when the project is ready to ship, hand off, or archive (e.g., all acceptance tests green, security review passed, documentation synchronized, technical-debt register reviewed).

### 7. Fine-Grained Implementation Planning

Break work into small, independently verifiable steps. At this stage you have enough context to define module boundaries and interface contracts.

### 8. Review the Plan

Review the plan for missing details, duplication, contradictions, and conciseness before starting implementation.

### 9. Set Up Modules and Interfaces

Define and document dependencies according to your architectural framework. The plan from steps 7–8 drives what modules are needed.

### 10. Set Up CI/CD, Linter, and Code Coverage

Stand up CI/CD at project start, before the first meaningful commit. Ensure the pipeline runs the same test commands that work locally. Keep linter and coverage configuration in the repository so they are enforced consistently. Decide what to do with each type of finding (error vs. warning, coverage floor, etc.).

### 11. Dependency and Package Management

Pin external dependency versions from day one. Schedule regular vulnerability scans (e.g., `cargo audit`, `npm audit`). Define an update cadence and a license-compliance policy. AI-generated code frequently introduces new dependencies — review every new import.

### 12. Implement

Follow the test-first rule. After every AI-generated batch: compile, run tests, review the diff. Do not accumulate unreviewed AI output.

### 13. Security Review

Conduct security reviews at defined checkpoints (e.g., before each release, after each major feature). Use the security concept from the documentation artifacts as a checklist. Feed findings back into the backlog as tracked items, not as comments in code.

### 14. Refactoring Policy

Refactor before adding new features in an affected area. Require all tests to pass before and after a refactor. Track refactoring candidates in `Technical-Debts.md` and schedule them explicitly — do not let debt accumulate silently.

---

## Controlling

- Conduct regular (automatic) code reviews.
- Have AI create a Mermaid schematic of the module architecture. Display all entities, classes, and their dependencies. List all architectural violations. Search for and resolve technical debt.
- Periodically close the gap between code and docs:
  1. Have the AI generate a description of the current code.
  2. Compare it to the existing documentation.
  3. Search for gaps, contradictions, coverage holes, and obsolete sections.
  4. Incorporate relevant gaps, archive or delete obsolete parts, and resolve contradictions.

---

## Documentation Drift

- **Keep requirements and documentation synchronized with the code.** The safest approach: treat documentation updates as part of the implementation workflow. Track documentation changes alongside code changes.
  - Consider storing concise source-level descriptions in file headers so the source itself acts as an AI-readable wiki.

- **Old plans:** create an `archive/` folder and explicitly document its purpose in the project constitution so it is not mistaken for current material.

---

## Tooling Reference

### Claude

Autopilot-like mode — enables bypass of permission prompts:

```
claude --permission-mode bypassPermissions
```

### Speckit

#### Project Preparation

```
0. /speckit.constitution
```

> Note: may need updating as the project evolves — verify before use.

#### Feature Iteration

```
1. /speckit.specify
2. /speckit.clarify       (optional)
3. /speckit.plan
4. /speckit.tasks
5. /speckit.analyze       (optional)
   /speckit.checklist     (optional — requires spec/plan/tasks to exist)
6. /speckit.implement
```

**Speckit maps to the main workflow as follows:** A Speckit feature cycle covers Workflow steps 7–8 (fine-grained planning and plan review) and drives step 12 (implementation). `/speckit.specify` and `/speckit.clarify` align with step 3 (requirements engineering) for the feature scope. `/speckit.plan` and `/speckit.tasks` replace steps 7–8. `/speckit.implement` executes step 12. Steps 1–6 and 9–11 of the main workflow are project-level concerns that Speckit does not cover.
