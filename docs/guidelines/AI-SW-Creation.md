# Goals & Principle of work

- highest goal - use all effort to create source code right and correct in the first attempt - correction is expensive. therefore planning is everything. **Remember - Many experience learnings and rules exist in our head, but not in AI Agents**, they are full of options but not necessary with preferences, they have no memory. **Write everything down as instructions.**

- Use AI to help with all the following advices. e.g. Ask ChatGPT to explain and suggest options with pros and cons so you can make a well-informed choice for your project.

- re-iterate (e.g. make a re-occuring event in outlook): if your project grows, think about growing and extending in all aspects. Use the tools made for it.

## Preparation & Infrastructure

- set up linter, code-coverage, decide for test-strategy

- make basic settings in the AI constitutions: e.g. pre-existing is not an excuse to ignore, create key-learnings, let agent know the infrastructure (wsl, servers, ssh connections, tools), install speckit, maybe superpowers, git commit and merge strategies, test procedures and categories, instruct use of linter, code-coverage tools - and the processing of its findings, instruct build pipelines, issue trackings, backlog handling
 -> examples for CLAUDE.md

- remote working on VM: ssh will stop when connection interrupted. use `tmux` to keep the session alive. use `tail -f <file>` to track logs.

- UNCONFIRMED: small source code files could strongly improve efficiency (no god classes)

- prepare a documentation concept
    Architecture docs,
    User story doc,
    Requirements docs,
    Interfaces docs,
    Security concept,
    Risc-Analysis,
    AI-Workload estimation (UNTESTED),

    Add into workflow:
        Key-Learnings.md,
        Project-Journal.md,
        Technical-Depths.md


## Promptng recommendations

- Prompting Vocabulary: concise, comprehensive, deep dive, contradiction, duplication / inline, dead/unused code, violation, missing details

- Prompting Technique: Paraphrase my description so I can confirm that I expressed myself correctly, Summarize, analyze issues and sugest multiple options on how to solve/fix them.

## Controlling

- Reviews
- Create mermaid schematic of module architecture. Display all entities and classes and their dependencies. List all architectural violations. Search and solve technical depths.

## Claude
autopilot like, this enables the bypass mode:
`claude --permission-mode bypassPermissions`

## Speckit

### project preparation
0. /speckit.constitution
UNCLEAR: might need updating

### feature iteration
1. /speckit.specify
2. /speckit.clarify (optional, here)
3. /speckit.plan
4. /speckit.tasks
5. /speckit.analyze (optional, here)
( /speckit.checklist (optional, can be run once you have a spec/plan/tasks))
6. /speckit.implement

## Project management / workflow

- Start with Requirements and UserStories

- Research Technology and Toppology

- Detail Requirements engineering

- Decide for Architecture framework (e.g. 'follows **Hexagonal Architecture** (Ports and Adapters) combined with **Clean Architecture** layering'), Guidelines (coding, workflow), Linter, CodeCoverage - write it into constitution.

- Decide for testing pyramid Architecture

- Set up Modules and interfaces. Decide dependencies (based on your architectural framework)

- Create workflow and make rules to follow it (write project journal, key-findings, technical-depts, look for inline code duplication that could be extracted to utility functions, dead code, update tools and libs, etc.)

- plan Phases and expected results, prepare Tests or Contracts to be fullfilled

- Fine planning of implementation

- Review planning for missing details, duplication, contradictions, concisivness

- Ensure tests are deterministically reproducable. Any daytime dependent behaviour needs to be time adjustable.

## Documentation drift

- documentation: UNSOLVED, how to keep your requirements and documentation synchronized with your code. Maybe by regular reviews. Change-tracking in workflow. Source code description in source header (source as AI wiki)?

- old plans: create an archive folder and explicitly explain its purpose in the project constitution

- let code be documented, then compare to available docs, search for gaps, contradictions, coverage, obsolete parts in new document. take over gaps where relevant, archive or delete obsolete, solve contradictions.


