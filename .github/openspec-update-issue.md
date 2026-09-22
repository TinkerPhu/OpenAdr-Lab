`.claude/skills/openspec-*/SKILL.md` records `generatedBy: "${CURRENT}"`; npm now publishes **${LATEST}**.

To update:

```
npm i -g @fission-ai/openspec@latest
npx openspec update
```

Then re-delete what `openspec update` reinstates, because it contradicts workflow rule 3
(a finished change is waved into `docs/` and deleted, never archived, and `openspec/specs/`
is not a documentation destination):

- `.claude/skills/openspec-archive-change/`
- `.claude/skills/openspec-sync-specs/` and `.claude/commands/opsx/sync.md`

Review the diff to the remaining skills and commands before committing — the generated text
carries behavioural instructions, not just version bumps.

_Filed by `.github/workflows/openspec-version-check.yml`. Body: `.github/openspec-update-issue.md`._
