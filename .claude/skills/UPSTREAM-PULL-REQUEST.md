# Skill: Create Upstream PR for openleadr-rs

Use this skill when creating a pull request against `OpenLEADR/openleadr-rs` from the `TinkerPhu/openleadr-rs` fork.

## Pre-flight Checklist

Before creating a branch or writing any code, verify:

1. **Git identity** — `git config user.email` must be `TinkerPhu@users.noreply.github.com` (not just `TinkerPhu`)
   ```bash
   git config user.email "TinkerPhu@users.noreply.github.com"
   git config user.name "TinkerPhu"
   ```
2. **Base branch** — always branch from `upstream/main`, not from `dev` or `main`
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
| DCO | Wrong email in signoff | `git commit --amend --signoff --reset-author --no-edit` + force-push |
| Format | Code not formatted | `cargo fmt` + amend + force-push |
| Build/test panic (exit 101) | Test fixture missing `"users"` | Add `"users"` to fixture list |
| Build compile error | SQLx cache hash mismatch | Regenerate cache on Pi4, verify hash matches |

Force-push after fixing:
```bash
git push --force-with-lease origin fix/my-feature
```

## Common Mistakes
- `git config user.email` is per-repo — set it in the submodule, not just globally
- Do NOT include co-authoring footers (Claude or otherwise) in commits or PR descriptions
- Do NOT push from `main` — always use a feature branch
- Amending a commit does NOT auto-update the signoff email unless `--reset-author` is passed
