---
description: Archive a completed feature's spec artifacts by moving them from specs/<feature-id>/ to specs/archive/<feature-id>/ and committing the result.
---

## Archive Feature

This command closes out a completed speckit feature by moving its spec directory into the archive and committing the change.

**When to use**: After all tasks are complete, the retrospective checklist is done, and the feature branch is ready to merge (or already merged) into main.

## Execution Steps

1. **Setup**: Run `.specify/scripts/powershell/check-prerequisites.ps1 -Json` from repo root and parse JSON for `FEATURE_DIR`.
   - Derive `FEATURE_ID` as the directory name (last path segment of `FEATURE_DIR`).
   - Derive `ARCHIVE_DIR` as `specs/archive/<FEATURE_ID>` (absolute path).
   - If `FEATURE_DIR` does not exist, stop and tell the user: "No active feature found. Is there a feature branch checked out?"
   - If `ARCHIVE_DIR` already exists, stop and tell the user: "Archive already exists at <ARCHIVE_DIR>. Was this feature already archived?"

2. **Confirm with user**: Before moving anything, output a one-line summary:
   ```
   Archive: specs/<FEATURE_ID>/ → specs/archive/<FEATURE_ID>/
   ```
   Then ask: "Proceed?" — wait for confirmation before continuing.

3. **Create archive parent if needed**: Ensure `specs/archive/` directory exists (create silently if not).

4. **Move the spec directory**:
   - Use the Bash tool: `mv <FEATURE_DIR> <ARCHIVE_DIR>`

5. **Stage and commit**:
   - `git add -A specs/`
   - Commit with message: `chore: archive spec kit artifacts for <FEATURE_ID>`

6. **Ask the user if merge to main is requested**
   - if afirmative, rebase the branch on top of main and fast-forward merge it to main.

7. **Report**: Output the result:
   - Path archived to
   - Commit hash
   - Remind the user that `postponed_features.md` is preserved inside the archive and should be referenced when starting the next feature
