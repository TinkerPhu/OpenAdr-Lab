# Goals & Principles of Work

- **Highest goal:** Invest maximum effort in getting source code right and correct on the first attempt — corrections are expensive. Therefore, planning is everything. **Remember: accumulated experience, learnings, and rules live in your head, not in AI agents.** Agents are rich in options but lack preferences, and they have no memory between sessions. **Write everything down as explicit instructions.**

- Use AI to support all the practices described below. For example, ask ChatGPT to explain a problem domain and suggest options with pros and cons so you can make a well-informed choice for your project.

- **Re-iterate regularly** (e.g., create a recurring calendar event): as your project grows, revisit and extend all aspects — documentation, architecture, tests, tooling. Use the tools made for this purpose.

---

## Preparation & Infrastructure

- Set up a linter and code-coverage tooling; decide on a test strategy before writing any production code.

- Prepare your workspace for Markdown document reading (most AI assistants consume `.md` files as context).

- Establish baseline settings in the AI constitution (e.g., `CLAUDE.md`). Key topics to cover:
  - Pre-existing issues are not an excuse to ignore them.
  - Key-learnings workflow.
  - Infrastructure details: WSL, servers, SSH connections, available tools.
  - Tool installation: speckit, superpowers, etc.
  - Git commit and merge strategy.
  - Test procedures and test categories.
  - Linter and code-coverage usage — including what to do with findings.
  - Build pipeline instructions.
  - Issue tracking and backlog handling.

  → See existing `CLAUDE.md` files in this project as concrete examples.

- **Remote work on a VM:** SSH sessions drop when the connection is interrupted. Use `tmux` to keep sessions alive. Use `tail -f <file>` to follow logs without blocking the terminal.

- **UNCONFIRMED:** Keeping source files small may strongly improve AI-agent efficiency (avoids god classes and large context windows).

- Define a documentation concept up front. Recommended artifacts:
  - Architecture docs
  - User story doc
  - Requirements docs
  - Interface docs
  - Security concept
  - Risk analysis
  - AI workload estimation *(UNTESTED)*

  Add the following to your ongoing workflow:
  - `Key-Learnings.md`
  - `Project-Journal.md`
  - `Technical-Debts.md`

---

## Prompting Recommendations

**Useful prompting vocabulary:** concise, comprehensive, deep dive, contradiction, duplication / inline, dead/unused code, violation, missing details.

**Useful prompting techniques:**
- *"Paraphrase my description so I can confirm I expressed myself correctly."*
- *"Summarize, analyze the issues, and suggest multiple options on how to solve or fix them."*

---

## Controlling

- Conduct regular code reviews.
- Have AI create a Mermaid schematic of the module architecture. Display all entities, classes, and their dependencies. List all architectural violations. Search for and resolve technical debt.

---

## Claude

Autopilot-like mode — enables bypass of permission prompts:

```
claude --permission-mode bypassPermissions
```

---

## Speckit

### Project Preparation

```
0. /speckit.constitution
```

> Note: may need updating as the project evolves — verify before use.

### Feature Iteration

```
1. /speckit.specify
2. /speckit.clarify       (optional)
3. /speckit.plan
4. /speckit.tasks
5. /speckit.analyze       (optional)
   /speckit.checklist     (optional — requires spec/plan/tasks to exist)
6. /speckit.implement
```

---

## Project Management / Workflow

1. **Start with Requirements and User Stories.**

2. **Research technology and topology** — understand the problem space before committing to a stack.

3. **Detail requirements engineering** — close ambiguities before they become bugs.

4. **Decide on an architecture framework** (e.g., *Hexagonal Architecture / Ports and Adapters* combined with *Clean Architecture* layering). Define coding guidelines, workflow conventions, linter rules, and code-coverage targets — write all of this into the constitution.

5. **Decide on a testing pyramid architecture:** BDD end-to-end tests (Gherkin/Cucumber), API tests, (maybe unit tests).

6. **Test-first approach:** make it a workflow rule to write tests first, run them to confirm they fail, then implement until they are green.

7. **Set up modules and interfaces.** Define and document dependencies according to your architectural framework.

8. **Create workflow rules and enforce them:** write the project journal, record key learnings and technical debts, look for inline duplication that can be extracted to utility functions, remove dead code, and keep tools and libraries up to date.

9. **Plan phases and expected results.** Prepare tests or contracts that define what "done" means for each phase.

10. **Fine-grained implementation planning** — break work into small, independently verifiable steps.

11. **Review the plan for missing details, duplication, contradictions, and conciseness** before starting implementation.

12. **Ensure tests are deterministically reproducible.** Any behavior that depends on the current date or time must be controllable via an injectable clock or configuration parameter.

---

## Documentation Drift

- **Keep requirements and documentation synchronized with the code.** The safest approach: treat documentation updates as part of the implementation workflow. Track documentation changes alongside code changes.
  - Consider storing concise source-level descriptions in file headers so the source itself acts as an AI-readable wiki.

- **Old plans:** create an `archive/` folder and explicitly document its purpose in the project constitution so it is not mistaken for current material.

- **Closing the gap between code and docs:**
  1. Have the AI generate a description of the current code.
  2. Compare it to the existing documentation.
  3. Search for gaps, contradictions, coverage holes, and obsolete sections.
  4. Incorporate relevant gaps, archive or delete obsolete parts, and resolve contradictions.
