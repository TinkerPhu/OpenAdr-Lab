# Key Learnings

## SQLx Offline Cache

- Hash = SHA-256 of the exact query string between `r#"` and `"#` (whitespace matters)
- File naming: `.sqlx/query-{hash}.json`, `hash` field inside must match
- When modifying SQL in Rust source, must update/rename `.sqlx` cache files with new hash
- **Preferred workflow** — generate cache on Pi4-Server (safest, reads the exact bytes Rust will compile):
  1. Push Rust code changes to the fork
  2. Pull on Pi4-Server
  3. Run hash script on Pi4 reading the actual `.rs` file:
     ```python
     import hashlib, re, json
     with open('openleadr-vtn/src/data_source/postgres/event.rs') as f: content = f.read()
     queries = re.findall(r'r#"(.*?)"#', content, re.DOTALL)
     for i, q in enumerate(queries): print(i, hashlib.sha256(q.encode()).hexdigest())
     ```
  4. Copy an existing `.sqlx/query-*.json`, update `hash` + `query` fields, save as new filename
  5. Commit from Pi4 and push
- **Alternative (Windows-safe)** — define SQL inline in a Python `.py` script file, run on Windows with `python gen_sqlx_cache.py`. This avoids reading the `.rs` file (avoiding CRLF/LF ambiguity) because Python string literals use `\n` (LF), matching what Rust/SQLx sees on Pi4. Verified working in Phase 17.
- **Risk**: if the Edit tool alters whitespace inside the SQL `r#"..."#` block (e.g. strips trailing spaces), the hash changes silently and the build fails 25 min later — always verify the generated hash against what Pi4 computes
- **Symlink note**: `openleadr-vtn/.sqlx` → `../.sqlx`. Use `.sqlx/` path for `git add`, not `openleadr-vtn/.sqlx/`
- **When replacing a cache file**: (1) DELETE the old file (`git rm`), (2) create new file with correct hash as filename AND in `hash` field, (3) **update the `query` field** inside the JSON to match the actual current SQL text from the source. Copying an old file and only changing `hash` field leaves stale query text → SQLx "hash collision" error
- **The `query` field inside the cache JSON must match the SQL in the `.rs` file exactly** — if it doesn't, SQLx detects the mismatch and fails
- **Cross-platform hash verification for PRs** — when creating `.sqlx` cache files on Windows for GitHub CI (Linux), verify hashes account for CRLF→LF conversion
- A wrong hash wastes ~25 min per rebuild cycle on Pi4

## Windows Gotchas

- **NEVER use `2>nul`** in Bash tool on Windows — creates a literal file named `nul` that's hard to delete. Use `2>/dev/null` instead
- **Subst drive D: maps to C:\DriveD** — Vite/vitest resolves paths through the real filesystem, causing `D:/Tinker/...` to become `C:/DriveD/Tinker/...`. This breaks `setupFiles`, `resolve(__dirname)`, etc.
  - **Fix for vite.config.ts**: do NOT use `resolve(__dirname)` for `root`. Omit `root` entirely.
  - **Fix for tests**: always run `npm test` from the real path: `cd C:\DriveD\Tinker\OpenAdr-Lab\...\ui && npm test`
  - **Docker builds are unaffected** (they run inside the container)

## Docker & Compose

- Docker Compose project name = directory name; don't duplicate in service names
- Docker Compose `.env` files silently override `${VAR:-default}` in YAML — always check for stale `.env` values after changing defaults
- `--abort-on-container-exit` kills everything when ANY container exits — don't use one-shot containers alongside it
- When multiple containers on a shared host need ports, pick a dedicated range (e.g. 82xx) to avoid conflicts with existing services
- Stale test DB can cause mass test failures — `docker compose down -v` removes ephemeral DB volumes
- **`docker compose` working directory matters** — `docker compose -f path/to/compose.yml run ...` resolves `context: .` relative to the compose file, but the entrypoint's `WORKDIR` and behave's `paths` setting depend on the build context being correct. Running `docker compose` from the wrong directory can cause `ConfigError: No steps directory` or similar path resolution failures. Always run from the project root: `cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run ...`

## Testing

- **Gherkin `Background`** runs before EACH scenario, not once per feature — use unique test data names
- **Behave step ambiguity** — `{param}` captures greedily; use distinct wording (e.g. "targeting both") to avoid collisions between single/dual-param steps. Fix: use `use_step_matcher("re")` with `[^"]+` capture groups
- **Behave feature-level tags** — `scenario.tags` only has scenario-level tags; use `scenario.feature.tags` too for inherited tags
- **`poll_until()` with short intervals** is the right pattern for testing eventual consistency across services
- **`userEvent.type` treats `{` as a special key descriptor** — In `@testing-library/user-event`, curly braces are reserved for keyboard shortcuts. To type literal JSON with braces, use `fireEvent.change()` instead
- **Program/Event update mutations wrap payload as `{ id, input }`** — test assertions must match this shape, not just the inner input
- **Mock clearing in beforeEach** — without `mockClear()`, assertions on mocks accumulate across tests and match stale calls
- **Test race conditions** — if tests run alphabetically and a prior test leaves stale data, add a short wait or explicit cleanup before asserting

## Playwright (E2E UI Tests)

- **Playwright on Pi4 ARM64** — works with Debian-slim + `playwright install chromium --with-deps` (~300MB); Alpine won't work (needs glibc)
- **MUI Select in Playwright** — `data-testid` is on hidden `<input>`; click parent div to open, then `li[role="option"]:has-text("...")` to select

## Rust / Axum

- **Axum 0.7 uses `:id` path params, NOT `{id}`** — `{id}` syntax is axum 0.8+. Wrong syntax compiles but routes return 404
- **VEN poll retry logic** handles auth failures gracefully — safe to start before fixtures are loaded

## OpenADR & VTN

- **programType**: NOT an enum. It's a free-text `Option<String>` in the spec. Example shows "PRICING_TARIFF" but any string is valid. No dropdown needed.
- **programDescriptions**: Array of URL entries. Each entry has one field: `url: String`. VTN UI maps single "Description URL" field to first array entry for simplicity.
- **openleadr-rs targets: one VEN per entry** — `extract_vens()` reads `values[0]` only. Must use `[{type:"VEN_NAME",values:["ven-1"]},{type:"VEN_NAME",values:["ven-2"]}]` NOT `[{type:"VEN_NAME",values:["ven-1","ven-2"]}]`
- **VEN-1 fixture venName is `ven-1-name`** (not `ven-1`) — ven ID is `ven-1`, venName is `ven-1-name`. Test VEN-2 provisioned with venName `ven-2`.
- **Token endpoint** is `/auth/token` (NOT `/oauth/token`), uses `application/x-www-form-urlencoded` (NOT JSON)
- **VTN auto-migrates** on first boot — no need for manual `cargo sqlx migrate run`
- **Role-based access is enforced**: wrong role = 403 Forbidden. `any-business` sees programs/events, `ven-manager` sees VENs — a BFF needing both must use multiple credentials
- **VTN POST /reports requires VEN role** — a BFF with business credentials cannot create reports on behalf of VENs
- **VTN returns 409 Conflict** when deleting events that have associated reports (FK constraint, no `ON DELETE CASCADE`). Must delete reports first, then events
- Credentials are argon2-hashed server-side; use API, not raw SQL INSERT
- To discover API shapes: create test data via curl, inspect JSON responses
- When API docs are unavailable, read Rust source (grep for route handlers, serde tags)
- **Events are permanent records** — deletion fails when reports exist. The correct pattern is to edit the event to add timing, marking it as completed
- **`ven_program` JOIN causes duplicate rows** — used for permission filtering but multiplies rows when a program has multiple enrollments. Fix with `DISTINCT`
- **Program enrollment (`ven_program`)** is appropriate for controlling program/event visibility (shared resources). Reports are VEN-private data and require direct ownership tracking (`ven_id`), not enrollment-based access
- **Timestamps must be RFC 3339** — VTN rejects naive timestamps (`2026-02-15T19:00:00` → 400 Bad Request). Use local time with offset: `2026-02-15T19:00:00+01:00` (CET). VTN normalizes to UTC internally; VEN UI displays in local time. CLI: `date -d '+0 min' +%Y-%m-%dT%H:%M:%S%:z`

- **Reactor FSM must track instruction changes, not just event presence** — a boolean `event_active` is insufficient for multi-interval events. When price changes between intervals (e.g., mid→high→mid), the FSM must detect the *effective* target changed and reset. Mid-range prices (between `price_low`/`price_high`) should be treated as inactive since target setpoints equal defaults.

## React / UI

- **MUI components provide native ARIA roles** — don't duplicate them (e.g. `<Button>` already has `role="button"`)
- Use `role="status"` and `role="alert"` on `<Typography>` for screen reader announcements
- **React Query `refetchInterval`** is a cleaner replacement for manual `setInterval` polling
- **`React.FC` is discouraged** — use plain `function` with typed props for cleaner component signatures
- **nginx reverse proxy (`proxy_pass`)** eliminates CORS issues — browser sees same-origin `/api/` calls
- Avoid DTO normalization across layers — pass through upstream field names as-is. One vocabulary reduces code and debugging friction

## Git & GitHub

- **Never push PRs from `main`** — always use feature branches. Pushing from `main` causes the fork to diverge from upstream
- **Signed-off-by (DCO)** — many open-source projects require `git commit --signoff`. Use `--author="Name <email>"` to control what appears publicly
- **GitHub noreply email** — use `username@users.noreply.github.com` to keep private email out of public commit history. Check `git config user.email` before committing — if it's not an email address, DCO will fail
- **DCO fix workflow** — amend the commit with `git commit --amend --signoff --reset-author --no-edit` (sets author+email from current git config), then force-push the branch. DCO re-checks on push
- **`git config user.email` persists per-repo** — set it once in the submodule: `git config user.email "username@users.noreply.github.com"`. It does not inherit from the parent repo's config
- **GitHub can't change PR head branch** — if a PR is on the wrong branch, must close and recreate
- **Cherry-pick conflicts** — commits built on top of each other can't be cleanly cherry-picked individually. Better to apply the combined diff manually as a single clean commit
- **Fork workflow**: keep `main` as upstream mirror, use `feature/*` branches for PRs, use `dev` branch for integration/deployment
- **rustfmt in upstream PRs** — always run `cargo fmt` before pushing. The CI Format check runs `cargo fmt --check` and fails fast. Long attribute lines like `#[sqlx::test(fixtures(...))]` get split to multiple lines by rustfmt
- **`#[sqlx::test(fixtures(...))]` fixture dependencies** — the `vens.sql` fixture inserts into `user_ven` which has a FK to `users`. Always include `"users"` before `"vens"` in the fixture list, exactly as all existing ven tests do
- **CI log access** — GitHub Actions logs for failed jobs are not accessible via `gh api .../logs` (returns redirect/403). Use `gh pr checks <n>` to see job names and URLs, then fetch log URLs via `gh api .../jobs --jq` to identify which specific job failed
- **`git reset --soft <base>` is the simplest squash** — unstages all commits back to index, then one `git commit -s` creates a single clean commit. Simpler than interactive rebase for squashing — no editor interaction needed
- **DCO email must match exactly** — `Signed-off-by` email must be identical to the commit author email. Using `git commit -s` with `git config user.email` set correctly handles this automatically. Always verify with `git log --format="%ae%n%(trailers:key=Signed-off-by,valueonly)"` before pushing
- **nohup over SSH returns exit code 1 but process runs** — when nohup writes to stderr ("nohup: ignoring input"), SSH reports exit code 1, but the background process started. Always verify with `docker ps` before concluding a background launch failed. Run `docker compose down` before any `docker compose run` to prevent accidental duplicate containers

## Infrastructure

- **Windows SSH PATH issue** — Git Bash SSH (`C:\Program Files\Git\usr\bin\ssh.exe`) takes PATH precedence over Windows OpenSSH and cannot find `C:\Users\<user>\.ssh\config`. Use full path `"C:/Windows/System32/OpenSSH/ssh.exe"` in Claude Code Bash commands when SSH connections fail silently.
- SSH to Pi has no interactive terminal — git credentials must be written directly to `~/.git-credentials`
- **BFF token refresh after VTN restart** — VTN regenerates JWT keys on restart; BFF's cached OAuth token becomes stale. BFF restart needed (or wait for token refresh)
- **Docker named volumes survive Pi power cycles** — volumes are stored in Docker's storage area on the filesystem, not in container layers. A mid-compilation crash does not corrupt them; the next build resumes incrementally with full cache hit rate
- **Two concurrent `cargo test --workspace` on Pi4 = crash** — each Rust compile job can use 200-400 MB RAM. Two containers with default parallelism saturate 4 cores + 4 GB RAM → SSH unreachable → power cycle required. Always run `docker compose down` before `docker compose run`. Use `CARGO_BUILD_JOBS` and compose `deploy.resources.limits` as safety nets
- **`CARGO_BUILD_JOBS=N` limits parallelism per container** — controls how many crates compile in parallel within one cargo invocation. Does not prevent multiple containers from running, but caps the damage if they do
