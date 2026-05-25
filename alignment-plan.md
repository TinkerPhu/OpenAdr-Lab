# Alignment Plan: Project vs. `docs/guidelines/AI-SW-Development.md`

> **Purpose:** Step-by-step gap analysis and remediation plan. Check a box (`[x]`) to include a step in your next alignment pass, or leave it unchecked (`[ ]`) to skip it. Items marked **✓ Already done** are informational only.

---

## How to use this file

1. Review each section. Mark items you want to tackle with `[x]`.
2. Work through them top-to-bottom (roughly priority order within each section).
3. Once an item is resolved, replace the checkbox with **✓ Done** and note the date.
4. Re-read this plan every few months as a "controlling" pass (guideline: *re-iterate regularly*).

---

## Section 1 — AI Constitution (`CLAUDE.md`)

The guideline lists nine mandatory topics for CLAUDE.md. Current state covers four well; five are missing or shallow.

### ✓ Already in CLAUDE.md
- Infrastructure details (WSL, Pi4-Server, SSH, docker)
- Pre-existing issues / test-failure policy
- Key-learnings workflow
- Architecture invariants, naming conventions, DTO passthrough, port rules

### Gaps to fill

| # | Item | Priority | Action |
|---|------|----------|--------|
| 1.1 | **Git branching & commit strategy** | High | Add a `branching:` section to CLAUDE.md: branch naming convention (e.g. `feature/<id>-<slug>`, `fix/<slug>`), merge target (main), when to push, PR rules. Current practice (feature branches, no force-push to main) is implied by usage but never written down. |
| 1.2 | **Test procedures and categories** | High | Add a `testing:` section linking to `docs/guidelines/TESTING.md` and summarising: unit / integration / BDD pyramid, how to run each (`wsl cargo test`, `run_all_tests.sh`, docker BDD), when tests must pass before commit. `TESTING.md` exists but is not referenced in CLAUDE.md. |
| 1.3 | **Linter and code-coverage rules** | Medium | Document in CLAUDE.md: which linter (`cargo clippy`, `eslint`), required clippy lint level (warnings as errors?), current coverage posture, and what to do with findings (fix vs. suppress with justification). |
| 1.4 | **Build pipeline instructions** | Medium | Add a `build:` section: local build steps for VEN (`wsl cargo build`), VTN BFF, VTN UI (`npm run build`), VEN UI; and how the Pi4 docker stack builds. Currently scattered across README, docker-compose files, and VTN blueprints. |
| 1.5 | **Issue tracking and backlog handling** | Low | Document where the backlog lives (`docs/BACKLOG.md`), how items are added (by AI vs. human), and what "in backlog" means for AI sessions. Prevents AI from silently ignoring known issues. |
| 1.6 | **Tool installation** | Low | Add a one-liner section listing: speckit, openspec, Claude Code CLI, tmux (Pi4), and any scripts in `scripts/`. Helps onboard a new AI session or human contributor. |

**Action format for each:** Edit `.claude/CLAUDE.md` directly. Keep additions terse — bullets, not prose.

---

## Section 2 — Session-Start Onboarding Checklist

Guideline: *"maintain a structured session-start checklist … bring a fresh session up to speed in minutes."*

**Current state:** No dedicated checklist document. Knowledge is spread across CLAUDE.md, README, and project_journal.md.

| # | Item | Priority | Action |
|---|------|----------|--------|
| 2.1 | **Create `docs/reference/SESSION_START.md`** | High | Write a short checklist an AI reads at the start of every session: (1) read CLAUDE.md, (2) read last 10 entries of project_journal.md, (3) read KEY_LEARNINGS.md, (4) check BACKLOG.md for open items, (5) run `git status` + `git log --oneline -10`, (6) check current branch name and purpose. Add a reference to this file in CLAUDE.md under `workflow:`. |

---

## Section 3 — Documentation Artifacts

Guideline recommends seven documentation artifacts at project start. Status:

| Artifact | Status | Gap |
|----------|--------|-----|
| Architecture docs | ✓ Comprehensive (`docs/architecture/`) | — |
| Requirements doc | ✓ (`docs/REQUIREMENTS.md`) | — |
| Interface docs | Partial (specs exist per-feature; no single interface overview) | See 3.1 |
| User story doc | ❌ Missing | See 3.2 |
| Security concept | ❌ Missing | See 3.3 |
| Risk analysis | ❌ Missing | See 3.4 |
| AI workload estimation | ⚠️ Guideline marks as UNTESTED — skip | — |

| # | Item | Priority | Action |
|---|------|----------|--------|
| 3.1 | **Interface overview doc** | Medium | Create `docs/architecture/INTERFACES.md` that lists all public API surfaces (VEN REST routes, VTN BFF routes, OpenADR wire types). Can be auto-generated from route files. Prevents interface drift going unnoticed. |
| 3.2 | **User story doc** | Low | Create `docs/USER_STORIES.md`. Extract existing use-case scenarios from `docs/use-cases/` into user-story format. Focus on the three VEN personas: grid operator, VEN manager, and automated controller. Low priority because use-cases docs partially fill this role. |
| 3.3 | **Security concept** | Medium | Create `docs/SECURITY.md`. Minimal content: threat model (authentication, token handling, OAuth flows), known risks, and mitigations in place. Feed any open items directly into BACKLOG.md. Required by guideline step 13 (security review checkpoints). |
| 3.4 | **Risk analysis** | Low | Create `docs/RISK_ANALYSIS.md`. Capture operational risks (Pi4 single-point-of-failure, upstream openleadr-rs changes, HiGHS solver license). One-pager is fine. |

---

## Section 4 — Ongoing Workflow Documents

| Artifact | Status | Gap |
|----------|--------|-----|
| `BACKLOG.md` | ✓ (`docs/BACKLOG.md`) | — |
| `KEY_LEARNINGS.md` | ✓ (`docs/reference/KEY_LEARNINGS.md`) | — |
| `project_journal.md` | ✓ (`docs/history/project_journal.md`) | — |
| `Technical-Debts.md` | ❌ Missing | `docs/plans/refactoring_backlog.md` is close but not a formal register |
| `CHANGELOG.md` | ❌ Missing | Guideline doesn't mandate it but it's standard; project_journal partially fills the role |
| Prompt library | ❌ Missing | Guideline: *"maintain a living collection of prompts"* |

| # | Item | Priority | Action |
|---|------|----------|--------|
| 4.1 | **`docs/reference/TECHNICAL_DEBTS.md`** | High | Migrate the relevant items from `docs/plans/refactoring_backlog.md` into a structured debt register. Format per item: description, affected module, estimated effort, date identified. Reference it from CLAUDE.md: *"before adding a feature in an affected area, check TECHNICAL_DEBTS.md first."* |
| 4.2 | **`docs/reference/PROMPT_LIBRARY.md`** | Low | Collect prompts that have proven effective into a categorised library (architecture review, code review, test generation, doc-sync). Start small: 5–10 prompts. Grows organically. |
| 4.3 | **`CHANGELOG.md` at root** | Low | Optional. If preferred over the project_journal, create a standard Keep-a-Changelog format file. Otherwise, add a note to CLAUDE.md that the project_journal serves as changelog and link to it. |

---

## Section 5 — Process Gaps

These are workflow rules that the guideline mandates but that are not currently documented in CLAUDE.md or any enforced checklist.

| # | Item | Priority | Action |
|---|------|----------|--------|
| 5.1 | **Test-first rule in CLAUDE.md** | High | Add explicitly: *"Write tests first; run them to confirm they fail; then implement until green."* This is partially implied by existing test-failure policy but not stated as a forward rule. |
| 5.2 | **Deterministic test clock policy** | Medium | Document in CLAUDE.md: any code depending on the current date/time must accept an injectable clock parameter. Already partially in place for MILP/simulator; codify it as a project-wide rule so new features inherit it. |
| 5.3 | **Dependency audit cadence** | Medium | Add to CLAUDE.md: run `cargo audit` and `npm audit` before each release. Document action for found vulnerabilities: add to BACKLOG.md with severity. Current state: no documented cadence or policy. |
| 5.4 | **Refactoring-before-feature rule** | Medium | Add to CLAUDE.md: *"Check TECHNICAL_DEBTS.md before adding a feature in an affected area. Refactor first if applicable tech debt exists. All tests must pass before and after any refactor."* |
| 5.5 | **Definition of Done** | Medium | Add a `definition-of-done:` section to CLAUDE.md (or to `docs/reference/SESSION_START.md`). Minimum criteria: all tests green, linter clean, project_journal updated, BACKLOG.md updated, architecture invariants verified. |
| 5.6 | **Security review cadence** | Low | Add to CLAUDE.md: *"Run `/security-review` before each release and after each major feature."* Document where findings go (BACKLOG.md). References the yet-to-be-created `docs/SECURITY.md`. |
| 5.7 | **License compliance policy** | Low | Add a one-liner to CLAUDE.md: acceptable licenses for Rust crates (MIT, Apache-2.0, BSD) and npm packages. AI-generated code frequently introduces new deps — this makes the review rule explicit. |

---

## Section 6 — Controlling

Guideline: *"conduct regular (automatic) code reviews … generate Mermaid architecture diagrams … periodically close the gap between code and docs."*

| # | Item | Priority | Action |
|---|------|----------|--------|
| 6.1 | **Architecture diagram generation** | Medium | Add a periodic task (calendar event or cron reminder) to run: *"Generate a Mermaid module dependency diagram from VEN/src/ and compare to `docs/architecture/module_dependency_graph_post_refactoring.md`. List any architectural violations."* Run via `/review` or a custom prompt in the prompt library (4.2). |
| 6.2 | **Periodic doc-to-code sync** | Medium | Schedule a quarterly pass: (1) generate a code-level description with AI, (2) compare to `DOCUMENTATION.md` and architecture docs, (3) update `DOCUMENTATION.md`, archive obsolete sections. Add this to `SESSION_START.md` as a quarterly reminder item. |
| 6.3 | **Automated code review hook** | Low | Configure a Claude Code hook (via `/update-config`) to run `/review` automatically before PRs to main. Currently code review is manual and ad-hoc. |

---

## Section 7 — Documentation Drift

Guideline: *"treat documentation updates as part of the implementation workflow … store concise source-level descriptions in file headers."*

| # | Item | Priority | Action |
|---|------|----------|--------|
| 7.1 | **Archive folder documentation** | Low | Add a `docs/archive/` section to CLAUDE.md explaining that `docs/plans/archive/`, `docs/architecture/archive/`, and `specs/archive/` hold completed/superseded material and should not be treated as current. Prevents AI from loading stale content. |
| 7.2 | **File header descriptions for key modules** | Low | Add a one-line `//! <description>` doc comment to the top of each major VEN module file (services, entities, milp_planner, vtn.rs). Acts as an AI-readable wiki for fast orientation. Start with files > 200 lines. |
| 7.3 | **`Requirement-Gaps.md` maintenance** | Low | `Requirement-Gaps.md` exists at root but lacks a clear owner or update trigger. Add a note to CLAUDE.md: *"Update `Requirement-Gaps.md` after each feature implementation cycle."* |

---

## Summary Table

| # | Item | Priority | Estimated effort |
|---|------|----------|-----------------|
| 1.1 | Branching strategy in CLAUDE.md | High | 15 min |
| 1.2 | Testing section in CLAUDE.md | High | 20 min |
| 1.3 | Linter/coverage rules in CLAUDE.md | Medium | 15 min |
| 1.4 | Build instructions in CLAUDE.md | Medium | 20 min |
| 1.5 | Backlog handling in CLAUDE.md | Low | 10 min |
| 1.6 | Tool installation in CLAUDE.md | Low | 10 min |
| 2.1 | `SESSION_START.md` | High | 30 min |
| 3.1 | `INTERFACES.md` | Medium | 1 h |
| 3.2 | `USER_STORIES.md` | Low | 1 h |
| 3.3 | `SECURITY.md` | Medium | 1 h |
| 3.4 | `RISK_ANALYSIS.md` | Low | 30 min |
| 4.1 | `TECHNICAL_DEBTS.md` | High | 45 min |
| 4.2 | `PROMPT_LIBRARY.md` | Low | 30 min |
| 4.3 | `CHANGELOG.md` | Low | 15 min |
| 5.1 | Test-first rule in CLAUDE.md | High | 5 min |
| 5.2 | Deterministic clock policy in CLAUDE.md | Medium | 5 min |
| 5.3 | Dependency audit cadence | Medium | 10 min |
| 5.4 | Refactoring-before-feature rule | Medium | 5 min |
| 5.5 | Definition of Done | Medium | 15 min |
| 5.6 | Security review cadence | Low | 5 min |
| 5.7 | License compliance policy | Low | 5 min |
| 6.1 | Architecture diagram generation cadence | Medium | 30 min |
| 6.2 | Quarterly doc-to-code sync | Medium | 30 min |
| 6.3 | Automated code review hook | Low | 20 min |
| 7.1 | Archive folder in CLAUDE.md | Low | 10 min |
| 7.2 | File header descriptions | Low | 1 h |
| 7.3 | `Requirement-Gaps.md` maintenance rule | Low | 5 min |

**Total High-priority items:** 5 items ≈ 2 h  
**Total Medium-priority items:** 11 items ≈ 4 h  
**Total Low-priority items:** 11 items ≈ 4 h

---

## Recommended execution order

Work top-to-bottom within each pass. After each item: update this file (mark Done + date).

### Pass 1 — Constitution hardening (~2 h, High items only)
1. ✓ Done 2026-05-25 — 1.1 Add branching strategy to CLAUDE.md
2. ✓ Done 2026-05-25 — 1.2 Add testing section to CLAUDE.md (link to TESTING.md + pyramid summary)
3. ✓ Done 2026-05-25 — 5.1 Add test-first rule to CLAUDE.md
4. ✓ Done 2026-05-25 — 2.1 Create `docs/reference/SESSION_START.md` + reference from CLAUDE.md
5. ✓ Done 2026-05-25 — 4.1 Create `docs/reference/TECHNICAL_DEBTS.md` from refactoring_backlog.md

### Pass 2 — Process and coverage (~4 h, Medium items)
6. ✓ Done 2026-05-25 — 1.3 Add linter/coverage section to CLAUDE.md
7. ✓ Done 2026-05-25 — 1.4 Add build instructions to CLAUDE.md
8. ✓ Done 2026-05-25 — 5.2 Add deterministic clock policy to CLAUDE.md
9. ✓ Done 2026-05-25 — 5.3 Add dependency audit cadence to CLAUDE.md
10. ✓ Done 2026-05-25 — 5.4 Add refactoring-before-feature rule to CLAUDE.md
11. ✓ Done 2026-05-25 — 5.5 Definition of Done (in SESSION_START.md §5 from Pass 1)
12. ✓ Done 2026-05-25 — 3.3 Create `docs/SECURITY.md`
13. ✓ Done 2026-05-25 — 3.1 Create `docs/architecture/INTERFACES.md`
14. ✓ Done 2026-05-25 — 6.1 Architecture diagram cadence (in SESSION_START.md §4 from Pass 1)
15. ✓ Done 2026-05-25 — 6.2 Quarterly doc-to-code sync (in SESSION_START.md §4 from Pass 1)

### Pass 3 — Nice-to-have (~4 h, Low items)
16. [ ] 1.5 Backlog handling in CLAUDE.md
17. [ ] 1.6 Tool installation in CLAUDE.md
18. [ ] 3.2 Create `docs/USER_STORIES.md`
19. [ ] 3.4 Create `docs/RISK_ANALYSIS.md`
20. [ ] 4.2 Create `docs/reference/PROMPT_LIBRARY.md`
21. [ ] 4.3 CHANGELOG.md decision (or journal-as-changelog note in CLAUDE.md)
22. [ ] 5.6 Security review cadence in CLAUDE.md
23. [ ] 5.7 License compliance policy in CLAUDE.md
24. [ ] 6.3 Configure automated code review hook
25. [ ] 7.1 Archive folder explanation in CLAUDE.md
26. [ ] 7.2 File header descriptions on key VEN modules
27. [ ] 7.3 Requirement-Gaps.md maintenance trigger in CLAUDE.md
