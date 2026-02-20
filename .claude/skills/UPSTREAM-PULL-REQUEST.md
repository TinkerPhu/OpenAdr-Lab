# Skill: Create Upstream PR for openleadr-rs

Use this skill when creating a pull request against `OpenLEADR/openleadr-rs` from the `TinkerPhu/openleadr-rs` fork.

## Pre-flight Checklist

Before creating a branch or writing any code, verify:

1. **Git identity on Pi4** — Pi4's global config is set correctly. Verify before any PR work:
   ```bash
   ssh Pi4-Server "git config --global user.name && git config --global user.email"
   # Must print: TinkerPhu / TinkerPhu@users.noreply.github.com
   # If wrong: ssh Pi4-Server "git config --global user.name 'TinkerPhu' && git config --global user.email 'TinkerPhu@users.noreply.github.com'"
   ```

2. **After any `git commit --amend`** — verify both author AND committer (amend sets committer from current git config, which can silently differ from the original author):
   ```bash
   git log -1 --format='Author: %an <%ae>%nCommitter: %cn <%ce>'
   # Both must show: TinkerPhu <TinkerPhu@users.noreply.github.com>
   ```
   If committer is wrong:
   ```bash
   GIT_COMMITTER_NAME='TinkerPhu' GIT_COMMITTER_EMAIL='TinkerPhu@users.noreply.github.com' git commit --amend --no-edit
   ```
   If author is also wrong, add `--reset-author` and matching `GIT_AUTHOR_NAME`/`GIT_AUTHOR_EMAIL` env vars.

3. **Base branch** — always branch from `upstream/main`, not from `dev` or `main`
   ```bash
   git fetch upstream
   git checkout -b fix/my-feature upstream/main
   ```

## Development Workflow

### 1. Make the change
- Edit source files
- If SQL queries changed in `event.rs` (or any file with `sqlx::query_as!`): update SQLx cache (see below)
- Run `cargo fmt` to format — CI will reject unformatted code

### 2. SQLx offline cache (if SQL changed)
Verify hashes match by running on Pi4 after push:
```bash
python3 -c "import hashlib,re; content=open('openleadr-vtn/src/data_source/postgres/event.rs').read(); queries=re.findall(r'r#\"(.*?)\"#',content,re.DOTALL); [print(i,hashlib.sha256(q.encode()).hexdigest()) for i,q in enumerate(queries)]"
```
The hashes must match the filenames in `.sqlx/`. If not, regenerate the cache files on Pi4.

### 3. Write tests (if adding a `#[sqlx::test]`)
- **Always include `"users"` before `"vens"`** in fixture lists — `vens.sql` inserts into `user_ven` which has a FK to `users`
  ```rust
  #[sqlx::test(fixtures("users", "programs", "vens", "vens-programs", "events"))]
  ```
- Run `cargo fmt` after writing tests — rustfmt reformats long `fixtures(...)` lists

### 4. Commit with DCO signoff
```bash
git commit --signoff -m "Fix: short description

Longer explanation of what and why.
"
```
Verify the signoff: `git show --no-patch --format="%ae%n%B" HEAD`
Must show `TinkerPhu@users.noreply.github.com` and a single `Signed-off-by:` line.

### 5. Push and create PR
```bash
git push origin fix/my-feature
gh pr create \
  --repo OpenLEADR/openleadr-rs \
  --head TinkerPhu:fix/my-feature \
  --base main \
  --title "Fix: short description" \
  --body "$(cat <<'EOF'
## Summary
- bullet points

## Test Plan
- [ ] what was tested

EOF
)"
```

**No co-authoring footers** in PR descriptions (they get rejected).

## Fixing CI Failures

Check status: `gh pr checks <PR_NUMBER> --repo OpenLEADR/openleadr-rs`

| Failure | Cause | Fix |
|---|---|---|
| DCO — Invalid committer email | `git commit --amend` set committer from Pi4 git config (wrong email) | `GIT_COMMITTER_NAME='TinkerPhu' GIT_COMMITTER_EMAIL='TinkerPhu@users.noreply.github.com' git commit --amend --no-edit` + force-push |
| DCO — author/SOB mismatch | Author email differs from `Signed-off-by` email | `git commit --amend --signoff --reset-author --no-edit` (with `GIT_AUTHOR_*` set) + force-push |
| Format | Code not formatted | `cargo fmt` + amend + force-push |
| Build/test panic (exit 101) | Test fixture missing `"users"` | Add `"users"` to fixture list |
| Build compile error | SQLx cache hash mismatch | Regenerate cache on Pi4, verify hash matches |

Force-push after fixing:
```bash
git push --force-with-lease origin fix/my-feature
```

## Common Mistakes
- `git commit --amend` sets the **committer** from the current git config, not from the original commit — always verify both author and committer after any amend
- Amending a commit does NOT auto-update the author email unless `--reset-author` is passed
- Do NOT include co-authoring footers (Claude or otherwise) in commits or PR descriptions
- Do NOT push from `main` — always use a feature branch
