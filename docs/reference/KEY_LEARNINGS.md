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
- **Subst drive C:\DriveD (formerly D:) warning** — Vite/vitest resolves paths through the real filesystem, causing old `C:/DriveD/Tinker/...` subst aliases to break `setupFiles`, `resolve(__dirname)`, etc. Always run from the real path `C:\DriveD\...`.
  - **Fix for vite.config.ts**: do NOT use `resolve(__dirname)` for `root`. Omit `root` entirely.
  - **Fix for tests**: always run `npm test` from the real path: `cd C:\DriveD\Tinker\OpenAdr-Lab\...\ui && npm test`
  - **Docker builds are unaffected** (they run inside the container)

## Docker & Compose

- **nginx caches upstream hostnames at startup** — `proxy_pass http://hostname/` resolves the hostname once when nginx starts, not per-request. If upstream containers are rebuilt and get new IPs, nginx still routes to the old IP (now stale or pointing to a different container). Fix: restart the nginx container after rebuilding any upstream service. In this project: always restart `ven-ui-1` after rebuilding `ven-1`, `ven-2`, or `ven-3`. Symptom: wrong data served (e.g. ven-1 proxy returning heater data from ven-2).
- Docker Compose project name = directory name; don't duplicate in service names
- Docker Compose `.env` files silently override `${VAR:-default}` in YAML — always check for stale `.env` values after changing defaults
- `--abort-on-container-exit` kills everything when ANY container exits — don't use one-shot containers alongside it
- When multiple containers on a shared host need ports, pick a dedicated range (e.g. 82xx) to avoid conflicts with existing services
- Stale test DB can cause mass test failures — `docker compose down -v` removes ephemeral DB volumes
- **`docker compose` working directory matters** — `docker compose -f path/to/compose.yml run ...` resolves `context: .` relative to the compose file, but the entrypoint's `WORKDIR` and behave's `paths` setting depend on the build context being correct. Running `docker compose` from the wrong directory can cause `ConfigError: No steps directory` or similar path resolution failures. Always run from the project root: `cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run ...`
- **`docker compose build <service>` is the reliable rebuild** — `docker compose run --build <service>` may rebuild only dependency images, not the named service itself. When source code changes, explicitly run `docker compose build <service>` first, then run normally
- **Named cargo volume + stale image = silent false-negative** — after source changes, if the Docker image hasn't been rebuilt, Cargo sees matching fingerprints in the named target volume and skips recompilation. New test functions silently don't appear in output (no error, no compilation). Always rebuild the image explicitly before relying on test results

## Testing

- **Gherkin `Background`** runs before EACH scenario, not once per feature — use unique test data names
- **Behave step ambiguity** — `{param}` captures greedily; use distinct wording (e.g. "targeting both") to avoid collisions between single/dual-param steps. Fix: use `use_step_matcher("re")` with `[^"]+` capture groups
- **Behave feature-level tags** — `scenario.tags` only has scenario-level tags; use `scenario.feature.tags` too for inherited tags
- **`poll_until()` with short intervals** is the right pattern for testing eventual consistency across services
- **Behave test-runner entrypoint already calls `python -m behave`** — the `entrypoint.sh` does `exec python -m behave "$@"`. Passing `python -m behave features/...` as the docker compose run command override causes double-invocation (`python -m behave python -m behave ...`), which fails. Correct invocation: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/<feature>.feature` — pass only the feature path as the argument. Never prefix with `python -m behave` when using the test-runner container.
- **`userEvent.type` treats `{` as a special key descriptor** — In `@testing-library/user-event`, curly braces are reserved for keyboard shortcuts. To type literal JSON with braces, use `fireEvent.change()` instead
- **Program/Event update mutations wrap payload as `{ id, input }`** — test assertions must match this shape, not just the inner input
- **Mock clearing in beforeEach** — without `mockClear()`, assertions on mocks accumulate across tests and match stale calls
- **Test race conditions** — if tests run alphabetically and a prior test leaves stale data, add a short wait or explicit cleanup before asserting

## Playwright (E2E UI Tests)

- **Playwright on Pi4 ARM64** — works with Debian-slim + `playwright install chromium --with-deps` (~300MB); Alpine won't work (needs glibc)
- **MUI Select in Playwright** — `data-testid` is on hidden `<input>`; click parent div to open, then `li[role="option"]:has-text("...")` to select
- **MUI Slider `data-testid` via `slotProps.input`** — forwards the prop to the internal `<input type="range">` in JSDOM (unit tests pass), but does NOT reliably reach the DOM `<input>` in a real Chromium browser. Use a `<Box data-testid={...}>` wrapper around the `<Slider>` instead, then scope selectors to `[data-testid="..."] input[type="range"]`
- **MUI Slider disabled state in Playwright** — use `wait_for_selector` with CSS `:disabled` / `:not([disabled])` pseudo-classes scoped to the Box wrapper (`state="attached"` works on hidden inputs). `wait_for_function` JS polling is less reliable and harder to debug
- **`docker compose run --build` only rebuilds the target service** — `depends_on` images (e.g. `test-ven-ui`) are NOT rebuilt. After changing UI source, explicitly `docker compose build test-ven-ui` before running tests, or use `--no-cache` if Docker's layer cache is stale despite source changes
- **React 18 unhandled render errors cause empty root div** — if a React component throws during render without an Error Boundary, React 18 unmounts the entire tree, leaving an empty `<div id="root"></div>`. Playwright tests see only a timeout waiting for `data-testid` selectors with no obvious cause. Diagnose by adding `page.on("pageerror", lambda exc: print(f"[PAGE ERROR] {exc}"))` and `page.on("console", lambda msg: print(...) if msg.type in ("error","warning") else None)` in `before_scenario` — these capture the actual JS exception before the tree unmounts. Add these listeners in `environment.py` for all browser-based scenarios.
- **API contract mismatches are silent** — TypeScript types can diverge from actual backend responses (wrong field names, object vs array shape) without compiler warnings if the hook returns `unknown` or `any`. When a page crashes with `e.map is not a function` or `Cannot read properties of undefined (reading 'toFixed')`, verify the actual API response with `docker exec <container> curl -s <endpoint>` before editing TypeScript types. Never trust declared types without confirming against live data.
- **Long-lived test containers bleed state between scenarios** — any server-side state set in scenario N survives to scenario N+1 if the container keeps running. Reset it explicitly in a behave Background step (e.g. `POST /sim/override {}` to clear VEN force overrides)

## Rust / Axum

- **`Option<f64>` in Setpoints/TraceSetpoints serializes as JSON `null`** — when a control channel has no active command (e.g. `pv_export_limit_kw: None`), serde produces `"pv_export_limit_kw": null`. TypeScript types must use `number | null`, not just `number`. Access with `!= null` (loose equality) to catch both `null` and `undefined`
- **Binary constraints should not be interpolated** — a hard limit either applies or doesn't. Using `if factor > 0.0 { to.value } else { from.value }` in an `interpolate()` function is correct for `Option` fields; trying to blend `None` and `Some` is meaningless
- **Axum 0.7 uses `:id` path params, NOT `{id}`** — `{id}` syntax is axum 0.8+. Wrong syntax compiles but routes return 404
- **VEN poll retry logic** handles auth failures gracefully — safe to start before fixtures are loaded
- **Don't add `ORDER BY` when application code groups results** — if rows are collected into a `HashMap` keyed by ID, DB-side ordering is redundant overhead. Remove it; the grouping logic is unaffected by row order.
- **`Ok(sqlx::query_as!(...))` wrapper pattern** — `retrieve()` wraps the entire async chain in a single `Ok(...)`. The `)` before `?` closes `Ok(`, not just the inner expression. When inserting `.map(|e| transform(e))` or strip helpers, they go inside this chain before `?`: `Ok(query!(...).fetch_one(&db).await?.try_into().map(|e| strip(e, flag))?)`. Dropping the `Ok(` leaves a dangling `)` that causes a compile error ("unexpected closing delimiter") far from the actual deletion site

## OpenADR & VTN

- **programType**: NOT an enum. It's a free-text `Option<String>` in the spec. Example shows "PRICING_TARIFF" but any string is valid. No dropdown needed.
- **programDescriptions**: Array of URL entries. Each entry has one field: `url: String`. VTN UI maps single "Description URL" field to first array entry for simplicity.
- **openleadr-rs targets: one VEN per entry** — `extract_vens()` reads `values[0]` only. Must use `[{type:"VEN_NAME",values:["ven-1"]},{type:"VEN_NAME",values:["ven-2"]}]` NOT `[{type:"VEN_NAME",values:["ven-1","ven-2"]}]`
- **GB-02/GB-03 (Phase 0):** ven-1's legacy fixture-seeded row (id `ven-1`, venName `ven-1-name`) is superseded — ven-1 is now re-provisioned via the VTN API in `tests/entrypoint.sh` / `scripts/seed_vtn.py`, same as ven-2/ven-3, giving it a real UUID id and uniform venName `ven-1`. The SQL fixture itself (shared with openleadr-rs's own CI) was left untouched; only our E2E/demo bootstrap deletes and re-provisions those rows.
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
- **Rust `Option<T>` serializes as JSON `null`, not omitted** — `null !== undefined` is `true` in JS. Always use loose equality `!= null` (catches both) when checking whether an optional value from a Rust API is absent. `forceValue !== undefined` will treat a reset `null` as "set"
- **`tsc` is the only full type-check** — `npm test` (vitest) only type-checks files imported by tests. Pages with no dedicated test file (e.g. `Dashboard.tsx`, `Trace.tsx`) can have broken types that all unit tests miss. Run `npx tsc --noEmit` locally before pushing, or accept that the Docker build is the last line of defence
- **Chart guard condition must match data source** — after adding synthetic future points, `chartData.length === 0` is never true. Guard on `traceEntries.length === 0` (historical data only) to correctly show the "no data yet" placeholder and avoid rendering `ResponsiveContainer` in tests before any real data arrives

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
- **Never assume upstream CI failures are pre-existing** — what looks like a pre-existing failure on main may be caused by your own commits (format differences, syntax bugs introduced during rebase, etc). Always investigate every CI failure properly rather than writing it off. So far, every "pre-existing" assumption turned out to be wrong
- **Codecov `}` line = implicit else-branch** — a closing brace reported as uncovered means the condition above it (e.g. `if let Some(...)`, `if condition`) was never false during tests. Identify what makes it false and add a test for that case (e.g. `if let Some(ref mut t) = targets` → add a test where `targets` is `None`)
- **`docker compose run --build` + `cargo clean` must happen in sequence** — `--build` rebuilds the image with new source, but the named cargo-target volume still has the old binary. Cargo compares source timestamps (from `COPY . .`, set at image build time) against binary timestamps (in the volume) and may consider the old binary fresh. Always clean after `--build`, not before

## Infrastructure

- **Windows SSH PATH issue** — Git Bash SSH (`C:\Program Files\Git\usr\bin\ssh.exe`) takes PATH precedence over Windows OpenSSH and cannot find `C:\Users\<user>\.ssh\config`. Use full path `"C:/Windows/System32/OpenSSH/ssh.exe"` in Claude Code Bash commands when SSH connections fail silently.
- SSH to Pi has no interactive terminal — git credentials must be written directly to `~/.git-credentials`
- **BFF token refresh after VTN restart** — VTN regenerates JWT keys on restart; BFF's cached OAuth token becomes stale. BFF restart needed (or wait for token refresh)
- **Docker named volumes survive Pi power cycles** — volumes are stored in Docker's storage area on the filesystem, not in container layers. A mid-compilation crash does not corrupt them; the next build resumes incrementally with full cache hit rate
- **Two concurrent `cargo test --workspace` on Pi4 = crash** — each Rust compile job can use 200-400 MB RAM. Two containers with default parallelism saturate 4 cores + 4 GB RAM → SSH unreachable → power cycle required. Always run `docker compose down` before `docker compose run`. Use `CARGO_BUILD_JOBS` and compose `deploy.resources.limits` as safety nets
- **`CARGO_BUILD_JOBS=N` limits parallelism per container** — controls how many crates compile in parallel within one cargo invocation. Does not prevent multiple containers from running, but caps the damage if they do

## React / Vitest / recharts (Phase 26)

- **MUI Switch click in Playwright** — `data-testid` on `<Switch>` goes on the root `<span>`. Clicking the span doesn't reliably fire `onChange`. Always target the inner `<input type="checkbox">`: `page.wait_for_selector('[data-testid="ctrl-..."] input[type="checkbox"]')`.
- **`globalThis` not `global` for jsdom mocks** — TypeScript projects targeting `browser` lib don't know `global`. Use `(globalThis as typeof globalThis & { ResizeObserver: unknown }).ResizeObserver = class ...` in `setup.ts`.
- **`dict.get(key, default)` returns `None` for explicit null** — Python's `.get()` only uses the default when the key is **absent**. If a JSON API returns `{"ev_plugged": null}`, `.get("ev_plugged", True)` returns `None`. Guard with `True if v is None else v`.
- **recharts `ResponsiveContainer` requires `ResizeObserver`** — jsdom doesn't include it. Mock in `setup.ts` with `globalThis`.
- **Bidirectional recharts stacking** — positive contributions use `stackId="positive"`, negative use `stackId="negative"`. Both are separate `Area` series derived from the same raw value.
- **MUI `Collapse` and Playwright `is_visible()`** — place `data-testid` on an element INSIDE `Collapse`, not outside. Otherwise `is_visible()` returns `true` even when collapsed.

## Rust (Phase 26)

- **One-shot overrides must be cleared outside `tick()`** — `tick()` receives `&UserOverrides` (immutable). Clear one-shot fields in `main.rs` after the tick block using a clone+patch posted back to shared state.

## Rust Simulator Reform (speckit 002)

- **`serde(flatten)` on HashMap merges keys into parent object** — `#[serde(flatten)]` on a `HashMap<String, f64>` field emits all its key-value pairs at the same JSON level as other named fields. Use this to flatten generic `state_values()` output alongside `power_kw` in the asset snapshot.
- **Backward-compat typed fields in API response** — when refactoring an API from named fields to a generic map, adding the old named fields back as derived/compat fields (reconstructed from the new generic data) allows zero UI changes. The old fields are cheap to derive and can be removed in a future speckit when the UI is updated.
- **`_resolve_nested` fallback for API shape migration** — when Python BDD step definitions use dotted paths like `"battery.soc"` against an API that moved `battery` under `assets.battery`, add a fallback in the resolver: try `data["assets"][first_part]` when `data[first_part]` is None. No feature file changes needed.
- **user_request.rs uses SimSnapshot not SimState** — code that receives `Option<&SimSnapshot>` must access per-asset state through `sim.assets.get("ev").and_then(|a| a.values.get("soc_pct"))`, not through typed helper methods like `.ev()` (which only exist on `SimState`).
- **Serde internally-tagged enum for YAML** — `#[serde(tag = "type", rename_all = "snake_case")]` on an enum allows `type: ev` in YAML to deserialize to `AssetConfig::Ev(EvConfig {...})`. The inner struct fields are sibling keys of `type` in the YAML map. The `id` field must also be in the inner config struct.
- **Profile dual-field transition** — keep `devices: DeviceConfig` for backward compat AND add `assets: Vec<AssetConfig>`. Add accessor methods (`ev_config()`, `battery_config()`, etc.) that check `assets` first then fall back to `devices`. Enables incremental migration without breaking existing callers.

## Controller Reform (speckit 004)

- **recharts drops reference lines outside XAxis domain** — `<ReferenceLine x={value}>` is silently omitted when `value` falls outside the computed domain. Always specify an explicit `domain` that includes the reference value: `domain={[Math.min(refValue - margin, ...data.ts), Math.max(refValue + margin, ...data.ts)]}`.
- **ResponsiveContainer ResizeObserver timing is test-load-bearing** — `ResponsiveContainer` uses `ResizeObserver` which fires asynchronously. Replacing it with a fixed-width `ComposedChart` eliminates this async delay, which can break tests that rely on MUI `Collapse` animation timing. Never swap `ResponsiveContainer` for a fixed-width wrapper without checking animation-dependent test behavior.
- **`docker compose down` before `run` after rebuilding services** — `docker compose run --rm test-runner` reuses already-running dependency containers (e.g., `test-ven-ui`). After rebuilding a dependency, `down` first to force fresh containers with the new image.
- **Deleted Rust modules don't fail builds if not in `mod` declarations** — orphaned `.rs` files (not listed via `mod foo;`) compile silently even if they have broken imports. Always search for `use crate::<deleted_module>::` in all source files when deleting a module.
- **`serde(tag = "type")` enum makes events directly JSON-serializable** — use a tagged enum for controller events instead of a string parameter. The tag field (`"type": "PlanCycle"`) is added automatically by serde and makes the event log self-describing in the API response.
- **Clone+modify+writeback for synchronous functions that need shared state** — when a synchronous function needs to mutate state held behind an async `RwLock`, clone the state out, pass `&mut` to the sync function, then write back. Avoids async-in-sync complexity and makes the function purely testable.
- **Active interval detection must handle missing `intervalPeriod`** — OpenADR events created without explicit `intervalPeriod.start` are always-active. A reporter that only fires for intervals with a matching time window will never fire for these events. Default: treat missing `intervalPeriod` or missing `start` as always-active.

## Timeline UI (speckit 005)

- **Server-side `max_points` downsampling is essential for timeline APIs** — a 3600-row ring buffer (1 sample/sec × 1 hour) sent raw to a browser chart on Pi4 ARM freezes the JS thread. Add a `max_points` parameter (default 120) and stride through the buffer with `step_by(ceil(n / max_points))`, always preserving the last point. A fresh VEN returns ~62 points; a 1-hour-old VEN returns exactly 120.
- **Playwright "locator resolved to visible" with timeout = JS frozen, not missing element** — when `wait_for_selector` times out but the call log shows `locator resolved to visible`, the DOM has the element but the JS thread is blocked (CPU overload). Confirms a performance/data-size issue, not a missing testid.
- **Rust `serde(rename_all = "snake_case")` vs TypeScript string unions** — serde produces `"switch"`, `"slider"`, `"number_input"`. If TypeScript defines `ControlKind = "Switch" | "Slider" | "NumberInput"`, all comparisons fail silently (no TS error on string union mismatch). Always verify serde output format against TS type values.
- **Schema-driven Switch must reflect sim state, not assume false** — when a boolean override is absent from `UserOverrides`, the control should display the sim's current hardware state as its initial value. Defaulting to `Boolean(null) = false` causes a click to toggle in the wrong direction (sends `true` instead of `false`). Add a per-key sim-snapshot fallback in `getValue` for any boolean control whose absent-override semantic is "use hardware default".
- **Stale test-ven-ui image silently runs old code** — `docker compose run --build test-runner` does NOT rebuild `test-ven-ui`. Must explicitly `docker compose build test-ven-ui` before the run whenever React source changes. The image bakes source at build time via `COPY`.
- **Uncommitted files cause Pi4 build failure, not local failure** — TypeScript files modified locally but not staged pass local `npm test` because the dev server uses the filesystem directly. The Pi4 Docker build fails because `COPY . .` copies only committed files. Always stage and commit all changed source files before pushing and deploying.

## OpenADR reportDescriptor Fields

- **VTN (openleadr-rs) does not store arbitrary reportDescriptor fields** — only the OpenADR 3.0 schema fields are persisted: `payloadType`, `readingType`, `aggregate`, `startInterval`, `numIntervals`, `historical`, `frequency`, `repeat`. Custom fields like `duration` are silently dropped.
- **Use `frequency` (integer seconds) for report interval duration** — `frequency` is the correct OpenADR 3.0 field for specifying how often a VEN should report. It's an integer, not an ISO 8601 duration string. Default to 3600 if not specified.

## Docker Test Infrastructure

- **`docker compose run --build` only rebuilds the run target** — dependent services (test-ven-1, test-ven-2, etc.) are NOT rebuilt. After changing VEN Rust source, must explicitly `docker compose -f tests/docker-compose.test.yml build --no-cache test-ven-1` to ensure the new binary is baked into the image. Without `--no-cache`, Docker's layer cache may reuse stale `COPY src ./src` layers if the build context hash hasn't changed (e.g., due to intermediate cached layers).
- **Timer-driven and obligation-driven reports must use distinct reportNames** — if both paths use the same `reportName`, `upsert_report()` causes one to overwrite the other. Use `ob-{ven}-{event}-{type}` for obligation reports vs `auto-{ven}-{event}` for timer reports. Events with `reportDescriptors` should be skipped by the timer path entirely.

## Planner Tariff Lookup

- **`resample_uniform` + epoch-aligned HashMap lookup never works for real-time slots** — `resample_uniform` aligns output to epoch-based grid boundaries (multiples of 5 min since Unix epoch). Planner slots start at the current second, so `import_map.get(&epoch)` always returns `None`. Use `TimeSeries::interpolate_at(slot_start)` for per-slot tariff lookup instead.
## BDD Test Polling

- **Don't poll for "any steps exist" when you need a specific plan state** — A step like `When I wait for plan to have steps for X` satisfies on the very first (stale) plan. When the scenario depends on a VTN event being reflected in the plan, poll for the specific assertion condition rather than mere existence of steps.
- **Post-scenario cleanup doesn't instantly update VEN plan** — After `after_scenario` deletes a VTN event, the VEN needs 2s (poll interval) to detect the deletion, then up to 20s for the planner to re-run. The next scenario may see the old plan if it polls immediately. Use a poll step that waits for the expected post-cleanup state.

## VEN State Architecture (016-refactor-ven-backend)

- **InnerState → 3-lock split requires `PersistedVenState` helper for JSON compat**: When splitting a single `Arc<RwLock<InnerState>>` into `polling`, `ctrl_sim`, and `hems` sub-locks, the existing `state.json` format is preserved by introducing a private `PersistedVenState` struct that contains only the fields that were actually serialised (non-`#[serde(skip)]`) in the old `InnerState`. `to_json` assembles this struct from the two relevant locks; `load_from_json` distributes back. No migration needed for existing state files.

- **`ControllerSimState` naming avoids collision with `simulator::SimState`**: When adding a controller-side state struct in a crate that already has `crate::simulator::SimState`, use a distinct name. `ControllerSimState` is unambiguous. Note: it requires explicit `impl Default` (not `#[derive(Default)]`) because `SensorSnapshot::empty_now()` is not the unit constructor.

- **Startup guard belongs in `try_load()` not `load()`**: The `Profile::try_load()` path is called by `main.rs` for production use; `Profile::load()` is used in tests that build synthetic profiles. Adding `if profile.assets.is_empty() { bail!(...) }` in `try_load()` keeps the test entry point clean.

- **Dead `TimeWindow` in `assets/mod.rs` coexists with live `TimeWindow` in `controller/timeline.rs`**: SC-002 verification grep for `TimeWindow` produces hits in the timeline module — these are a completely different struct used by the `/timeline` route feature. Only the dead `TimeWindow` in `assets/mod.rs` (used solely by `AssetCapabilities`) is removed. The grep pattern is correct but results require human triage.

- **INVARIANT: no RwLock guard held across a second lock acquisition** — even read guards. While two simultaneous `read()` calls can't deadlock, holding a guard across an `.await` of a second lock violates the stated INVARIANT and makes the code harder to audit. Always use the acquire-clone-drop pattern: `let val = { lock.read().await.field.clone() };` before acquiring the next lock.

## Deviation Absorber (017-add-deviation-absorber)

- **`impl Default` vs. `pub fn default()` is a Rust trait bound distinction**: A struct with `pub fn default() -> Self` as an associated function does NOT satisfy `T: Default`. Struct spread `..Default::default()` requires the `Default` trait to be implemented. When adding a new field to such a struct in test literals, you must write the field explicitly (e.g., `absorber: Default::default()` using the nested struct's real trait impl) rather than relying on spread syntax.

- **Private re-export at module boundary**: `crate::simulator::EnergyCounter` is not available because `simulator/mod.rs` uses `use energy::EnergyCounter` (private), not `pub use`. To use it from outside `simulator`, import through the public sub-module: `use crate::simulator::energy::EnergyCounter`. Before assuming a re-export exists, check whether the mod.rs line is `pub use` or just `use`.

- **VEN unit tests were never run in CI**: The first `cargo test` run on Pi4 revealed multiple stale tests referencing removed types (`DeviationState`, `apply_deviation_correction`) and non-existent fields. New features should ensure unit tests run in the BDD pipeline (or a parallel cargo-test job). Test infrastructure gaps accumulate silently.

- **Residual vs. raw deviation for Tier 2 triggers**: Accumulating the raw grid deviation (post-net) into a Tier 2 counter causes spurious MILP replans for transient deviations the absorber handles in real-time. Accumulate `residual_kw` (what the absorber could NOT cover) instead. The trigger becomes "absorber exhausted for N consecutive ticks" — a semantically meaningful and less noisy escalation signal.

- **SSE deduplication by magnitude delta**: Emitting a `CorrectionActive` event every tick floods SSE subscribers with near-identical messages. A threshold (0.2 kW change since last emission) suppresses noise during steady-state correction. State-transition events (`CorrectionCleared`) should always be emitted regardless of magnitude — they signal a discrete change in control state.

- **Docker build context includes `target/` by default**: On a Pi4 with 2.1 GB in VEN/target/, every `docker compose run --build` spent 3 minutes just sending the build context before compilation started. Fix: add `VEN/.dockerignore` with `target/`. Named volumes (`ven-cargo-target`) then keep the compiled artifacts across runs without re-sending them through the Docker socket.

- **EV departure guard: skipping charge curtailment, not charge addition**: The guard blocks the absorber from reducing EV charge when departure is imminent and SoC < target. It does NOT block increasing EV charge to absorb surplus. When no session exists (unknown departure), the guard is disabled — conservative assumption is that absorption takes priority. Guard only triggers for positive deviation (import excess → curtail load).

- **Absorber BDD deviation injection via PV irradiance is time-of-day dependent**: The MILP plan computes `plan_signed_net_kw` from battery/EV/base_load allocation without forecasting PV. Actual net is therefore `plan_net - pv_actual`. When PV generates, `deviation_kw = actual - plan = -pv < 0` (always surplus). PV irradiance injection creates surplus-magnitude change, not positive-shortage deviation. BDD tests using "PV drop = positive deviation" produce inverted or near-zero absorber response. Fix: drive BDD deviation via `/plan` endpoint baseline comparison, or add a physics-independent inject field (e.g., `deviation_override_kw`). Unit tests remain the reliable validation layer for absorber logic.

- **`AssetSnapshot` exposes `power_kw` (actual delivered), not `setpoint_kw` (commanded)**: The `/sim` response under `assets.<id>` contains `power_kw` (from `AssetEntry.last_power_kw`) plus flattened state values (`soc`, `plugged`, etc.). The commanded setpoint is internal to the dispatcher loop and not exposed in the API. BDD assertions on absorber behavior must use `power_kw` with relative-change semantics (delta from baseline), not absolute setpoint comparisons.


## Deterministic MILP BDD Tests (022-deterministic-test-env)

- **pv_irradiance vs pv_plan_kw are two separate overrides**: pv_irradiance
  is a physics-tick inject — it affects what PV produces NOW and lets the EMA decay
  model forward-extrapolate into the horizon.  pv_plan_kw is a MILP-forecast
  inject — it pins every slot in the 24h horizon to a constant kW, completely
  replacing the sin-model forecast.  Tests that need deterministic planner output
  (e.g., stable battery headroom) must use pv_plan_kw, not pv_irradiance.

- **MILP planning-only overrides must NOT trigger a replan**: Including pv_plan_kw
  in the should_replan guard in 
outes/sim.rs causes a T1+T2 double-solve race:
  the Background step fires T1 (replan), the subsequent absorber step fires T2, and
  the second plan is adopted during the 8 s assertion window.  Overrides that only
  affect future planning (not current device state) must be excluded from
  should_replan — same rationale as ase_load_kw.

- **Read inject snapshot before spawn_blocking**: pv_plan_kw (like all inject
  fields) must be captured from inject_snap BEFORE the spawn_blocking call in
  planning.rs.  The one-shot fields (pv_irradiance, ase_load_kw) are cleared
  by the sim tick; reading them inside the closure risks a stale zero value.

- **Architecture ring naming at domain boundary**: The infra ring calls the field
  pv_plan_kw; the domain ring calls the parameter pv_forecast_override.  This
  rename at the boundary is intentional: it preserves the domain ring's independence
  from infrastructure field names and makes the distinction from pv_irradiance
  self-documenting in the function signature.

- **Clamp planning overrides at the point of use**: pv_forecast_override.max(0.0)
  in uild_milp_inputs prevents a BDD test injecting a negative value (e.g., by
  mistake) from producing unphysical negative PV generation in the MILP model.
  Validate at the boundary, not in the route handler.

- **No-replan BDD assertion pattern**: To verify an inject does NOT trigger a solve,
  capture plan["created_at"] BEFORE the inject (via Given the system is idle),
  then poll GET /plan for N seconds after the inject and assert created_at does
  not change.  This is more reliable than log-string matching and works across both
  the replan_interval-based periodic solve and the watch-channel-based reactive solve.

- **Reward variables need a lower coupling to act** (Phase 4, WP4.1-b): a reward
  on a slack variable that only appears in upper-bound constraints
  (`ev_energy <= core + e_ev_extra`) is free money — the solver maxes the slack
  without moving the physical variable. To make a reward drive behaviour, put it
  on the physical quantity itself (per-slot `p_ev`) or couple the slack from
  below. Audit any `-reward * aux_var` objective term for this shape.

- **Phase 2 friction smoothing competes with soft incentives** (Phase 4, WP4.1-c):
  any objective preference weaker than `phase2_epsilon_eur` over the affected
  slots can be traded away by the friction phase (it may spend exactly that
  budget on ramp smoothing). Either make the incentive dominate the epsilon
  (ASAP's 10 EUR/kWh·h lateness) or specify and test the weaker invariant
  ("front-loaded up to the friction budget"), never assert the strong one.

- **Gate timing-sensitive test phases on actual host load, not ordering**
  (Phase 4): running the @isolated E2E tail "after" the main suite still means
  running at load 5+. Containers see the host /proc/loadavg — poll it and start
  the sensitive phase only below a threshold (entrypoint.sh waits for 1-min
  load < 2.0, capped). Two flaky runs became deterministic.

- **When a backlog entry's premise is wrong, say so in its resolution**
  (Phase 4, BL-19): the entry assumed a live comfort-curve consumption path;
  implementation found the resolved curve was dropped. The resolution records
  the gap (curve→MILP tiers still open) instead of silently absorbing or
  silently expanding scope.

- **vitest and eslint do not typecheck — run `npm run build` before shipping UI
  changes** (Phase 4, WP4.6): a type-predicate error passed the full UI test
  suite and lint locally, then failed `tsc && vite build` inside the Docker
  image build on Pi4, killing the E2E run before any test executed. The tsc
  gate only exists in the image build unless you run it locally.

- **Never pipe docker build output through `tail -1`** (Phase 4): a failed
  `docker compose build` was invisible because only the last line survived;
  `compose up -d` then silently kept the old image running ("Container …
  Running" instead of "Recreated"). Check for ERROR lines explicitly, or let
  the full output through.

- **A MILP with cost-equal integer choices is nondeterministic across builds —
  break ties in the objective** (Phase 3/4 review): shiftable-load start slots
  were only pinned by cost; the x86 HiGHS build happened to pick the earliest
  slot while the Pi4 ARM build picked a later one, producing an E2E flake that
  no local run could reproduce. Any binary choice the system's observable
  behaviour depends on needs an explicit (tiny) objective bias — in BOTH
  phases of a two-phase solve, or the phase-2 epsilon budget undoes it.

- **Attach a one-line state diagnostic to E2E poll timeouts** (Phase 3/4
  review): wrapping `poll_until` failures with a `/plan` summary (trigger,
  allocated assets, warnings) turned four unreproducible "flakes" into a
  single attributable planner defect on the first failing run afterwards.
  A timeout that only says "timed out" blames the infrastructure by default.

- **A pre-physics snapshot is last tick's state, not this tick's — a control
  loop that reads it for an uncontrolled/physics-driven asset (no real
  setpoint, e.g. PV) is always one tick behind** (Phase 3/4 review): the
  EV-surplus overlay's tiny, persistent grid-residual toggle traced back to
  exactly this. Fix pattern: preview what physics is about to compute for
  `now` (a pure, read-only function mirroring the mutating formula) and pass
  that into the control loop instead of the pre-tick snapshot. Guard the
  preview against drifting from the real formula with an equivalence test
  that calls both with identical arguments and asserts they agree.

- **A "derived" simulator quantity can't be used to test for the deviation it
  was derived from** (Phase 5 WP5.1): SITE_RESIDUAL is defined as
  `grid_meter_kw − Σ modelled_asset_kw`, but the simulator's own
  `grid.net_power_w` is computed *as* the sum of its modelled assets each
  tick — so in simulation the two terms can never disagree, and residual is
  mathematically guaranteed to read 0. Caught only by tracing the physics
  engine's "derive grid meter" step, not by the unit tests (which correctly
  pass against hand-built snapshots that assert nothing about what the real
  simulator can produce). Before writing an integration/adapter-contract
  test for a formula involving two "independent" signals, verify they really
  are independent in the system under test — one may be defined in terms of
  the other, silently making the interesting case untestable end-to-end.

- **Threading a new "distinct but structurally identical" term through a
  solver (residual_kw parallel to base_kw) touches every site that already
  special-cases the original term, not just its declaration** (Phase 5
  WP5.1): `p_base_kw` alone appeared in the shared power-balance constraint,
  two independent PV-surplus heuristics in a separate interactions module,
  and two result-reporting call sites — all needed the same treatment.
  `grep -n "p_base_kw"` across the whole subsystem before starting is faster
  than discovering each site via compile errors one at a time.


## Total Project Review (docs/plans/total_review_plan.md, 2026-07)

- **Unit tests + tsc cannot see production-bundle breakage — only a real
  browser can.** vite 8's rolldown bundler mis-resolved a MUI default-import
  interop in the VTN UI so the built bundle threw React #130 at runtime,
  while vitest (jsdom, unbundled modules) and `tsc` stayed fully green.
  After any bundler/toolchain major upgrade, the Pi4 browser E2E is the
  gate that matters; alternatively `vite preview` + one manual page load
  before merging. Conservative pin (vite ^7) chosen over debugging a
  brand-new bundler.

- **Review findings expire — re-verify each one against current main
  immediately before fixing.** A review conducted on a baseline commit that
  intervening merges outran produced an obsolete finding (an "unused"
  StaleRatePolicy that WP4.4 had since fully wired) which survived into an
  owner decision before being caught. The cost of one grep per finding at
  fix time is trivially cheaper than reverting a wrong fix.

- **cargo audit reports the lockfile, not the compiled graph.** Cargo.lock
  pins dependencies of *optional, disabled* features (e.g. sqlx-mysql's
  `rsa` behind `default-features = false`); `cargo tree -i <crate>` is the
  arbiter of whether an advisory is actually in the build.

- **vitest 4: mocks called with `new` must be implemented with
  `function`/`class`.** Arrow-function `mockImplementation(() => ({...}))`
  for class mocks (VenApi/BffApi pattern) is not constructable and fails
  at render, not at mock definition.

- **On this 8 GB host, WSL cargo builds must be throttled** (`-j 2`, one
  build at a time, check free RAM first) — two host crashes via pagefile
  exhaustion during this review. Rule lives in `.claude/CLAUDE.md`
  (memory-budget).

- **A dead endpoint can hide behind a plausible empty state.** The Planner
  tab's packet board polled a route deleted months earlier; react-query's
  error state left `data` undefined, `?? []` rendered the same UI as "no
  work scheduled", and every unit test stayed green because tests mock the
  hook. When removing a backend abstraction, grep the UI/consumer side for
  its whole chain (types → client method → hook → component → tests), and
  prefer empty states that distinguish "nothing to show" from "fetch
  failed".

- **DTO pass-through types drift unless audited against the owning
  struct.** The UI's `FlexibilityEnvelope` carried a `packet_id` field the
  Rust wire struct never had and lacked four real fields — harmless only
  because nothing consumed the type. When a wire struct changes, its UI
  mirror must change in the same commit.

- **When a wide, multi-call-site refactor can't be compile-verified in the
  same pass (host memory constraint, no build available), ship the
  fully-tested pure building blocks and defer the wiring as documented
  debt rather than risk an unverifiable edit across several existing test
  suites.** Applied during the weather-forecast-plugin build: the domain
  physics/port/adapter layers landed fully tested, but threading a new
  field through `SolveRequest`/`build_milp_inputs` (6+ call sites) was
  deferred to a follow-up, recorded as R-50 in `docs/reference/TECHNICAL_DEBTS.md`
  instead of silently left undone.

- **Read the vendor's own API documentation before reverse-engineering
  codes from a small observed sample.** SRF Meteo's icon-code legend
  (fetched from the PDF linked in `SrfWeatherToInfluxDb.py`'s own source
  comment) revealed sign=day/night, magnitude=condition — not guessable
  from the handful of codes seen in one day's data.

- **This project already has two reusable patterns for "staged but not
  yet wired" work**: a module-level `#![allow(dead_code)]` with a doc
  comment (`entities/design_vocabulary.rs`'s "type-level sketches" header)
  for Rust code, and a `@wip` BDD tag (`behave.ini`'s `tags = ~@wip`, per
  `ven_reports.feature`) for committed-but-not-yet-passing scenarios. Reuse
  these before inventing a new "not implemented yet" convention.

- **Re-verify a prior session's "too risky, deferred" call before
  accepting it as settled — the risk may have been overestimated, and any
  blocking prerequisite may have since landed.** R-50 (weather → planner
  wiring) was deferred as "6+ risky call sites"; tracing the actual call
  graph on a follow-up pass found only 9, most of the apparent call sites
  being local test-wrapper functions rather than the production functions
  themselves. Don't let an earlier deferral decision calcify into an
  assumption.

- **A file already flagged near its size cap (an existing watch-list
  debt note) will tip over on the very next real change — plan the split
  the note already called for, don't spend cycles manually shaving
  comment lines to survive one more feature.** `tasks/planning.rs` was at
  ~198/200 per a pre-existing debt note recommending a split "when next
  touched." That next touch arrived; the fix was the directory-module
  split the note had already specified, not line-by-line compaction.

- **Verify document structure immediately after any edit that inserts a
  new section into an existing file with prior content — not just that
  the diff looks right in isolation.** An edit meant to append a new
  journal section instead landed mid-list inside an earlier entry,
  orphaning an unrelated bullet. Caught by grepping the file's heading
  structure right after, before reporting the edit done.

- **`#[allow(dead_code)]` markers need revisiting once code actually gets
  wired in** — they're not "set and forget." When `GET /weather` started
  calling the weather-forecast-plugin's physics functions, clippy stopped
  flagging them as dead on its own; the stale `#[allow(dead_code)]`
  attributes and their "not yet wired" doc comments had to be found and
  removed by hand (clippy doesn't warn about an unnecessary `allow`).
  Treat every such marker as a follow-up-change checklist item, not a
  permanent annotation.

- **When adding a new config type to an existing profile file, check
  `scripts/audit_file_sizes.py` before, not after, deciding where it goes.**
  ~55 lines pushed `profile/schema.rs` over its 500-line cap; every other
  asset in this codebase already keeps its config-struct-to-domain-struct
  mapping (`BatteryConfig`→`BatteryParams` etc.) in one place, but nothing
  stops a new one from being the file that finally tips the cap. Run the
  audit script immediately after a green build, before calling any phase
  "done."
