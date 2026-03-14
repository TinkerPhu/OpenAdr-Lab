╭────────────────────────────────────────────────── Agent Folder Security ───────────────────────────────────────────────────╮
│                                                                                                                            │
│  Some agents may store credentials, auth tokens, or other identifying and private artifacts in the agent folder within     │
│  your project.                                                                                                             │
│  Consider adding .claude/ (or parts of it) to .gitignore to prevent accidental credential leakage.                         │
│                                                                                                                            │
╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────── Next Steps ────────────────────────────────────────────────────────╮
│                                                                                                                            │
│  1. You're already in the project directory!                                                                               │
│  2. Start using slash commands with your AI agent:                                                                         │
│     2.1 /speckit.constitution - Establish project principles                                                               │
│     2.2 /speckit.specify - Create baseline specification                                                                   │
│     2.3 /speckit.plan - Create implementation plan                                                                         │
│     2.4 /speckit.tasks - Generate actionable tasks                                                                         │
│     2.5 /speckit.implement - Execute implementation                                                                        │
│                                                                                                                            │
╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
╭─────────────────────────────────────────────────── Enhancement Commands ───────────────────────────────────────────────────╮
│                                                                                                                            │
│  Optional commands that you can use for your specs (improve quality & confidence)                                          │
│                                                                                                                            │
│  ○ /speckit.clarify (optional) - Ask structured questions to de-risk ambiguous areas before planning (run before           │
│  /speckit.plan if used)                                                                                                    │
│  ○ /speckit.analyze (optional) - Cross-artifact consistency & alignment report (after /speckit.tasks, before               │
│  /speckit.implement)                                                                                                       │
│  ○ /speckit.checklist (optional) - Generate quality checklists to validate requirements completeness, clarity, and         │
│  consistency (after /speckit.plan)                                                                                         │
│                                                                                                                            │
╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
