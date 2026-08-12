# Key Learnings

## SQLx Offline Cache

- Hash = SHA-256 of the exact query string between `r#"` and `"#` (whitespace matters)
- File naming: `.sqlx/query-{hash}.json`, `hash` field inside must match
- When modifying SQL in Rust source, must update/rename `.sqlx` cache files with new hash
- **Preferred workflow** — generate cache on Node1 (safest, reads the exact bytes Rust will compile):
  1. Push Rust code changes to the fork
  2. Pull on Node1
  3. Run hash script on Node1 reading the actual `.rs` file:
     ```python
     import hashlib, re, json
     with open('openleadr-vtn/src/data_source/postgres/event.rs') as f: content = f.read()
     queries = re.findall(r'r#"(.*?)"#', content, re.DOTALL)
     for i, q in enumerate(queries): print(i, hashlib.sha256(q.encode()).hexdigest())
     ```
  4. Copy an existing `.sqlx/query-*.json`, update `hash` + `query` fields, save as new filename
  5. Commit from Node1 and push
- **Alternative (Windows-safe)** — define SQL inline in a Python `.py` script file, run on Windows with `python gen_sqlx_cache.py`. This avoids reading the `.rs` file (avoiding CRLF/LF ambiguity) because Python string literals use `\n` (LF), matching what Rust/SQLx sees on Node1. Verified working in Phase 17.
- **Risk**: if the Edit tool alters whitespace inside the SQL `r#"..."#` block (e.g. strips trailing spaces), the hash changes silently and the build fails 25 min later — always verify the generated hash against what Node1 computes
- **Symlink note**: `openleadr-vtn/.sqlx` → `../.sqlx`. Use `.sqlx/` path for `git add`, not `openleadr-vtn/.sqlx/`
- **When replacing a cache file**: (1) DELETE the old file (`git rm`), (2) create new file with correct hash as filename AND in `hash` field, (3) **update the `query` field** inside the JSON to match the actual current SQL text from the source. Copying an old file and only changing `hash` field leaves stale query text → SQLx "hash collision" error
- **The `query` field inside the cache JSON must match the SQL in the `.rs` file exactly** — if it doesn't, SQLx detects the mismatch and fails
- **Cross-platform hash verification for PRs** — when creating `.sqlx` cache files on Windows for GitHub CI (Linux), verify hashes account for CRLF→LF conversion
- A wrong hash wastes ~25 min per rebuild cycle on Node1

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

- **Playwright on Node1 ARM64** — works with Debian-slim + `playwright install chromium --with-deps` (~300MB); Alpine won't work (needs glibc)
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
- **`Option<T>` cannot represent tri-state PATCH bodies (absent / null / value)** — `serde_json`'s blanket `Option<T>` impl collapses a top-level JSON `null` straight to Rust `None` for *any* `T`, including `Option<serde_json::Value>`, before `T::deserialize` ever runs. A field typed `Option<serde_json::Value>` therefore cannot distinguish "key absent from the JSON body" from "key present with value `null`" — both deserialize to `None`. This silently breaks any partial-merge endpoint whose contract is "absent = no change, null = clear the field, value = set it" (e.g. `POST /sim/inject`): the null-clear branch becomes structurally unreachable via real HTTP requests, even though it looks correct in code and in tests that construct the body struct directly in Rust. Fix: use the "double option" pattern — `Option<Option<T>>` with `#[serde(default, deserialize_with = "double_option")]`, where `double_option` wraps the field in an extra `Option::deserialize` call to preserve the third state. **Regression tests for this class of bug must deserialize an actual JSON string via `serde_json::from_str`, not construct the body struct by hand** — hand construction bypasses the exact serde behavior being tested and produces a false-positive pass. (VEN `routes/sim.rs`, fixed 2026-07-31 after the bug was found live on Node1 for `grid_export_limit_kw`/`pv_generation_limit_kw`.)

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
- **Two concurrent `cargo test --workspace` on Node1 = crash** — each Rust compile job can use 200-400 MB RAM. Two containers with default parallelism saturate 4 cores + 4 GB RAM → SSH unreachable → power cycle required. Always run `docker compose down` before `docker compose run`. Use `CARGO_BUILD_JOBS` and compose `deploy.resources.limits` as safety nets
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

- **Server-side `max_points` downsampling is essential for timeline APIs** — a 3600-row ring buffer (1 sample/sec × 1 hour) sent raw to a browser chart on Node1 ARM freezes the JS thread. Add a `max_points` parameter (default 120) and stride through the buffer with `step_by(ceil(n / max_points))`, always preserving the last point. A fresh VEN returns ~62 points; a 1-hour-old VEN returns exactly 120.
- **Playwright "locator resolved to visible" with timeout = JS frozen, not missing element** — when `wait_for_selector` times out but the call log shows `locator resolved to visible`, the DOM has the element but the JS thread is blocked (CPU overload). Confirms a performance/data-size issue, not a missing testid.
- **Rust `serde(rename_all = "snake_case")` vs TypeScript string unions** — serde produces `"switch"`, `"slider"`, `"number_input"`. If TypeScript defines `ControlKind = "Switch" | "Slider" | "NumberInput"`, all comparisons fail silently (no TS error on string union mismatch). Always verify serde output format against TS type values.
- **Schema-driven Switch must reflect sim state, not assume false** — when a boolean override is absent from `UserOverrides`, the control should display the sim's current hardware state as its initial value. Defaulting to `Boolean(null) = false` causes a click to toggle in the wrong direction (sends `true` instead of `false`). Add a per-key sim-snapshot fallback in `getValue` for any boolean control whose absent-override semantic is "use hardware default".
- **Stale test-ven-ui image silently runs old code** — `docker compose run --build test-runner` does NOT rebuild `test-ven-ui`. Must explicitly `docker compose build test-ven-ui` before the run whenever React source changes. The image bakes source at build time via `COPY`.
- **Uncommitted files cause Node1 build failure, not local failure** — TypeScript files modified locally but not staged pass local `npm test` because the dev server uses the filesystem directly. The Node1 Docker build fails because `COPY . .` copies only committed files. Always stage and commit all changed source files before pushing and deploying.

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

- **VEN unit tests were never run in CI**: The first `cargo test` run on Node1 revealed multiple stale tests referencing removed types (`DeviationState`, `apply_deviation_correction`) and non-existent fields. New features should ensure unit tests run in the BDD pipeline (or a parallel cargo-test job). Test infrastructure gaps accumulate silently.

- **Residual vs. raw deviation for Tier 2 triggers**: Accumulating the raw grid deviation (post-net) into a Tier 2 counter causes spurious MILP replans for transient deviations the absorber handles in real-time. Accumulate `residual_kw` (what the absorber could NOT cover) instead. The trigger becomes "absorber exhausted for N consecutive ticks" — a semantically meaningful and less noisy escalation signal.

- **SSE deduplication by magnitude delta**: Emitting a `CorrectionActive` event every tick floods SSE subscribers with near-identical messages. A threshold (0.2 kW change since last emission) suppresses noise during steady-state correction. State-transition events (`CorrectionCleared`) should always be emitted regardless of magnitude — they signal a discrete change in control state.

- **Docker build context includes `target/` by default**: On a Node1 with 2.1 GB in VEN/target/, every `docker compose run --build` spent 3 minutes just sending the build context before compilation started. Fix: add `VEN/.dockerignore` with `target/`. Named volumes (`ven-cargo-target`) then keep the compiled artifacts across runs without re-sending them through the Docker socket.

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
  image build on Node1, killing the E2E run before any test executed. The tsc
  gate only exists in the image build unless you run it locally.

- **Never pipe docker build output through `tail -1`** (Phase 4): a failed
  `docker compose build` was invisible because only the last line survived;
  `compose up -d` then silently kept the old image running ("Container …
  Running" instead of "Recreated"). Check for ERROR lines explicitly, or let
  the full output through.

- **A MILP with cost-equal integer choices is nondeterministic across builds —
  break ties in the objective** (Phase 3/4 review): shiftable-load start slots
  were only pinned by cost; the x86 HiGHS build happened to pick the earliest
  slot while the Node1 ARM build picked a later one, producing an E2E flake that
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
  After any bundler/toolchain major upgrade, the Node1 browser E2E is the
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

## Heater Safety Envelope (see docs/architecture/VEN_ARCHITECTURE.md's Heater section)

- **A comfort/service band and a true physical safety limit are two
  different kinds of constraint, even when the code only has one field for
  each edge.** `temp_min_c`/`temp_max_c` looked like "the" physical bounds;
  they're actually a user-configured comfort target. The real physical
  ceiling sits *above* `temp_max_c` (scalding/relief-valve risk), and there
  is no physical floor at all — the tank can drift to ambient with zero
  harm. Conflating the two meant `emergency_active` was treating a comfort
  edge as if defending it were a safety requirement.
- **Removing a doc's backlog entry is a "done" signal — don't emit it
  before the work is actually done.** Caught mid-session: the entry was
  deleted from the analysis doc's backlog right when a renumbering pass
  touched that section, before any code existed yet. Restored it and only
  removed it again once Node1 validation actually passed. Doc bookkeeping
  and completion are separate steps.
- Every new struct field needs a literal-construction site audit, not just
  the `Default` impl. `HeaterParams`/`HeaterConfig` are constructed via
  full struct literals (no `..Default::default()`) in several test files;
  `grep -rn "HeaterParams {" / "HeaterConfig {"` found all of them before
  `cargo check` did, which was faster than iterating on compiler errors
  one file at a time.

## PV Weather-Override Decay Bug

- **A one-shot field's "is this override active" check must track the
  field's whole lifecycle, including decay, not just its instantaneous
  value on the current tick.** `pv_irradiance` auto-clears from
  `SimInjectState` one tick after being posted, then its offset EMA-decays
  back toward the natural sin model — deliberately slowly (tuned for
  slider-drag smoothness over a 300 s window). The precedence rule ("manual
  override beats weather") was correctly *stated*; it broke because
  "override active" was implemented as "is the field `Some` this exact
  tick" instead of "is there still a perturbation in flight." Those two
  only coincide for fields that don't decay.
- **Reproduce live before trusting a code review of precedence logic.**
  The suppression check read correctly in isolation; only `curl`-ing a
  running instance (`POST /sim/inject`, then polling `/capability/pv`) and
  watching `irradiance` drift while `power_kw` stayed pinned to the weather
  value revealed the override was clearing far sooner than its own doc
  comments assumed.
- **`run_all_tests.sh --e2e` pulls the Node1 checkout from `origin` before
  building — it does not test local uncommitted changes.** The first E2E
  run in this session validated `origin/main`, not the working tree with
  the heater feature in it; the PV bug found there was real but unrelated
  to what was supposedly being tested. Confirmed via `git status --branch`
  before drawing conclusions. Use `scp` (per `.claude/skills/deploy-node1`)
  to test uncommitted local work against the Node1 E2E stack instead.
- A twin/preview function (`peek_pv_kw` mirroring `SimState::tick`) needs
  the same fix as its counterpart, found by grep, not by assumption — the
  equivalence test between them only catches *output* divergence for
  scenarios it actually exercises, not a shared logic bug present in both.

## PV-Export Decision Variable (openspec/changes/pv-export-curtailment/)

- **A field that "looks wired" (has a doc comment describing its intended
  path) can still be completely dead in production.** Scoping "give the
  planner a PV-export decision variable" surfaced that
  `PvInverter.export_limit_kw` — the field `step_inner` actually clamps
  against — was never written by any live tick-pipeline code path, only by
  unit tests. `dispatcher.rs` computed a clamped `setpoints["pv"]` value
  intending to enforce it, but `PvInverter::step()` ignores its
  `setpoint_kw` argument entirely (already flagged dead by its `_` prefix
  and a doc comment) — so VTN `EXPORT_CAPACITY_LIMIT` events had no
  physical effect on simulated PV output until this change. A doc comment
  asserting a mechanism works is a claim, not evidence; trace the actual
  call graph before building on top of it.
- **Adding a free decision variable to a MIP can surface a pre-existing
  MIP-gap-tolerance artifact immediately, even with zero real incentive to
  exploit it.** Every real cost term already favored *not* curtailing PV
  (export revenue exceeds the small `w_grid` friction cost), yet a test
  with **no export cap at all** still came back with PV curtailed by a
  small amount — `solve_phase1`'s pre-existing `with_mip_gap(0.02)` let
  HiGHS accept a "good enough" incumbent on the `u_grid` binary rather than
  the true optimum. Separately, Phase 2's objective is friction-only and
  had *zero* opinion on the new variable, so it could drift arbitrarily
  within the epsilon cost budget. Fixed with a tiny tie-break
  (`PV_USE_TIEBREAK_EUR_PER_KWH`), the same pattern already established by
  `SHIFT_TIEBREAK_EUR_PER_SLOT` for shiftable-load start slots — this
  codebase's second confirmed case of "a real cost difference should
  dominate, but a tiny nudge is still needed to make HiGHS actually find
  it."
- **A per-tick "effective" value computed inside one function isn't
  automatically available to a second function called alongside it.**
  `build_tick_setpoints` composes `effective_capacity` (VTN capacity state
  merged with sim-injected overrides) as a local variable and never
  exposed it; the new PV export-limit resolver, called separately in
  `tick.rs`, initially read the raw un-merged `capacity_snap` instead —
  compiling and unit-testing fine, but silently ignoring the
  `grid_export_limit_kw` sim-inject path in production. Caught only by a
  live Node1 `curl` test (`POST /sim/inject` with `grid_export_limit_kw`,
  then `GET /capability/pv`), not by any unit test, since the unit tests
  each exercised the resolver and the capacity-composition logic
  separately, never together through the real tick path. Fixed by
  extracting the composition into a shared `effective_capacity()` helper
  both call. Lesson: when two code paths both need "the same effective
  value," share the function that computes it — don't let each caller
  reconstruct it from raw inputs.

## PV Curtailment History & Inverter Capability (openspec/changes/pv-curtailment-history/)

- **Don't persist a modeled quantity as if it were a measurement.** A first
  draft proposed storing `curtailed_kw` (potential vs. actual output). Real
  inverters under a curtailment command report actual output and the
  commanded limit — never "what would have been produced," which is a
  model, not something measurable. Persisting it as ground truth wouldn't
  have generalized past the simulator. Store only the facts the system
  actually knows (the limit, its source); derive anything else (e.g.
  whether a limit is currently binding) at render/query time from those
  facts.
- **"A limit is active" and "a limit is actually reducing output" are
  different questions — conflating them breaks on real hardware.**
  Checking only whether `export_limit_kw` was set (ignoring the inverter's
  own AC capability) would have misclassified ordinary DC/AC-oversizing
  clipping as imposed curtailment. `rated_kw` (DC panel peak) and
  `inverter_max_kw` (AC output capability) are genuinely different values
  in real installations; a profile field for one doesn't stand in for the
  other.
- **Self-review an openspec spec before implementing it, even a spec you
  just wrote.** A dedicated review pass on `spec.md` found six real gaps —
  a missing tie-break scenario, a "binding" concept leaking into a
  requirement that shouldn't have known about it, no aggregation rule for
  a categorical field sampled across a time window, no scenario for the
  actual motivating case, a requirement with no way to satisfy it
  (`inverter_max_kw` needed live-visibility but nothing required it), and a
  proposal/spec disagreement over a validation rule. None were wording —
  each would have let two implementers build different, incompatible
  behavior from the same document.
- **`run_in_background: true` on a `wsl bash -lc "cargo test ..."` call
  can get silently killed mid-compile even with ample free memory and
  `-j 1`, while the identical command run synchronously (foreground, large
  timeout) succeeds immediately.** Four consecutive manually-backgrounded
  attempts died at the same "Compiling ven-app" line regardless of host
  memory (checked immediately before each: 1.0–2.2 GB free) or WSL's own
  internal memory (always healthy, 4.8+ GiB free). Running the same command
  in the foreground with a large timeout — even when it later exceeded the
  timeout and got auto-backgrounded by the harness — completed cleanly. The
  failure was specific to *manually requesting* background execution for
  this class of command, not to system resources. Default to synchronous
  invocation for WSL cargo commands; only rely on auto-backgrounding when a
  command genuinely runs long.
- Adding a new field to a persisted struct (`PvState`, written to
  `sim_state.json`) needs `#[serde(default)]` on the new fields for
  backward compatibility with already-persisted state on disk — the same
  pattern already used for config structs, just not yet needed on a *state*
  struct before this change.
- The file-size audit can fail on a file nobody touched in the current
  change — `tasks/sim_tick/helpers.rs` was already over its 200-line cap
  from a previous, unrelated feature merge, and `history_store/mod.rs`
  crossed 500 lines from this change's own two new columns. Per the "fix
  what the audit reports, don't triage by blame" rule: extracted
  `dispatch_override.rs` and `ticks.rs` respectively, following the
  existing `pv_smoothing.rs`/`notifications.rs` split pattern (free
  functions taking `&Connection`/`&mut Connection`, delegated to from the
  trait impl).
- **`docker compose run <service>` without `--build` silently reuses the
  last-built image**, including stale test fixtures/step files `COPY`'d in
  at build time — it is not a bind mount. Two verification cycles wasted
  scp'ing a fix to the Node1 host filesystem and rerunning, without noticing
  the container's behavior hadn't changed at all. Always pass `--build`
  when verifying a fix to Dockerfile-`COPY`'d files.
- **An E2E assertion that "happens to pass" isn't the same as an
  invariant.** `ven_heater_tank.feature`'s "no full-tier heater allocation
  anywhere in the first 12 slots near T_max" looked like it tested the
  trajectory model's overheating guard, but `E[t] ≤ E_max` is enforced for
  *every* slot unconditionally — the tank legitimately cools during
  idle/mid-tier slots and reopens headroom for a later full-power burst,
  which is correct behavior, not a bug. Only slot 0 is provably
  headroom-constrained; which later slot (if any) gets a burst depends on
  MILP solver tie-breaking, and this session's own always-on
  `PV_USE_TIEBREAK_EUR_PER_KWH` objective term nudged that tie-breaking
  even for a scenario with no PV curtailment involved at all. Confirmed via
  direct manual repro (`/sim/inject` + `/plan` fetch, bypassing behave)
  before narrowing the assertion — verify a suspected over-broad assertion
  against real solver output, don't just guess and weaken it.
- Node1 is shared with other active sessions; host load spikes to 7-8 are
  real and cause genuine timeouts on scenarios that are otherwise correct.
  Before concluding a timeout is a regression, check `uptime`/`ps aux
  --sort=-%cpu` for concurrent load, and rerun the specific failing
  scenario(s) in isolation once load drops — don't rerun the entire ~45min
  suite repeatedly hoping for a quiet window.
- **A "wait for fresh plan" step that captures its cutoff timestamp *after*
  the triggering action can race the trigger it's waiting on.**
  `ven_heater_tank.feature`'s near-T_max scenario intermittently timed out
  at 300s (previously passed in 81s) even though nothing in the planner or
  dispatcher changed for it. Root cause: `POST /sim/inject` with
  `heater_temp_c` synchronously fires `PlanTrigger::AssetStateChange` — but
  the "Given I inject" step blocks for up to 15s *after* that POST, waiting
  for the sim tick to reflect the injected temperature (a real, separate,
  already-fixed race — see that step's own docstring). By the time the next
  step captured `cutoff = datetime.now()`, the AssetStateChange-triggered
  plan (built almost immediately after the POST) was already older than
  cutoff — so the test was never actually waiting on its own injection's
  plan, only on some unrelated later trigger that might or might not arrive
  within the timeout. Fixed by capturing the cutoff *before* sending the
  triggering POST (`context.plan_freshness_cutoff`, `phase_a_physics_steps.py`)
  and having the wait step consume it when present. General lesson: when a
  test waits for "a plan/state created after X," X must be captured before
  the action that can cause that creation — not after any step that itself
  blocks, however briefly.
- **HiGHS (via `good_lp`) returns all-zero row/column duals for any model
  containing an integer-flagged column — even one pinned to a single value
  via an added equality constraint.** Building the `solver-marginal-cost`
  dual LP by keeping mode variables `variable().binary()` and adding
  `constraint!(x == fixed_value)` on top compiled, solved, and silently
  returned zeros for every dual, in scenarios where the hand-derived
  answer was clearly non-zero. Disabling presolve didn't fix it either
  (which was itself the signal the bug was at the declaration/model-type
  level, not a presolve-eliminated-row artifact). The fix: declare the
  variable directly as continuous with `min == max == fixed_value` instead
  — "integer column pinned to a constant" and "continuous column fixed at
  a constant" are not equivalent for dual availability, even though they
  describe the same feasible region. Any future code reading LP duals out
  of a MILP by fixing its integers must declare those decisions as
  continuous `[v, v]` outright.
- When validating a solver-derived economic quantity (a shadow
  price/dual), don't just assert "differs from X" against a scenario
  picked by intuition — derive the expected value by hand via the LP's
  KKT stationarity condition first. A "battery pinned at its own power
  bound" scenario looked like it should move the balance row's dual (per
  the design doc's worked example), but it doesn't: KKT only pulls another
  constraint's dual into a variable's own equation when that variable
  participates in the constraint, and `p_imp` never touches the battery's
  power-bound row directly. Redesigned the test around a scenario where
  `p_imp` itself is party to a binding constraint (an import-violation
  penalty), which matched the hand-derived value exactly on the first try.
- **A storage asset's "available capacity" has two independent dimensions —
  power rating and energy headroom — and a capacity check that only tests
  one will silently offer a lever that's actually unusable.** The
  deviation arbiter's battery lever (`controller::arbiter`) initially
  checked only `cap_max_import/export_kw`, which stays nonzero regardless
  of SoC — so a battery at 100% SoC would still have been offered as a
  charge lever, violating the arbiter's own "zero-capacity levers must be
  excluded outright, not deprioritized" rule (see
  `docs/architecture/VEN_ARCHITECTURE.md`'s Deviation Arbiter section). Caught by a test for the
  "everything else is exhausted, only the backstop lever remains" case,
  which only works if the battery is genuinely excluded — not by reasoning
  about the battery lever in isolation. Fix: also gate on
  `available_charge/discharge_kwh <= 0.0`. General lesson: when modelling
  "how much more can this asset do right now," enumerate every resource
  dimension that can independently reach zero (rate limit, energy/SoC
  limit, thermal limit, etc.) rather than checking the one that's most
  obviously relevant to the lever's direction.

## Comfort-Curve MILP Wiring (BL-34, openspec/changes/comfort-curve-milp-constraints/)

- **A MILP reward term can be syntactically correct and semantically inert at the same
  time — verify the variable it rewards is actually load-bearing in the constraint set
  before writing a test around it.** The plan was to test the EV's comfort curve by
  asserting `e_ev_extra_kwh` differs between two curves. It never did, at any reward
  value: `e_ev_extra` is bounded only *above* by `e_extra_max_kwh × z_ev_core`
  (`ev_milp.rs::constraints`) — nothing lower-bounds it by real charged power, so the
  solver "banks" the reward without moving `p_ev`. This was an already-filed debt item
  (R-18, `docs/reference/TECHNICAL_DEBTS.md`), independently rediscovered here via a
  binary-search probe on the reward coefficient before the pattern made sense. The fix
  was to trace which variable the constraint set actually couples to real allocation
  (`z_ev_core`, via `ev_energy ≥ e_core_kwh × z_ev_core`) and test that instead. Before
  wiring a new reward into an existing MILP objective, read the constraints, not just the
  objective — the two can disagree about what a variable actually controls.
- **A "reward gated behind the same binary as another degenerate reward" confounds any
  test of the first reward.** Once `z_ev_core=1`, the free-banked `e_ev_extra` reward
  (above) subsidizes committing to core regardless of the core-side reward's own value —
  so a naive core-price threshold test kept committing even when the core price alone
  was clearly unprofitable, until the confounding extra-price was pinned to `0.0`
  independently. When two reward terms share a gating variable, isolate one by neutralizing
  the other, not just varying the one under test.
- **An "obviously correct" objective-coefficient estimate (tariff × energy) can be off by
  a wide margin against the real solved threshold** — Phase 2 friction terms and other
  objective contributions shift the true breakeven point. A short empirical binary search
  on the actual solved output (a handful of `cargo test -- --nocapture` runs at different
  coefficient values) found the real threshold faster and more reliably than deriving it
  by hand from the objective's listed terms.

## Node1 Operational Gotchas (BL-41 final verification, 2026-08-01)

- **Never wrap `docker_host_lock.sh` in an outer `ssh Node1 "..."` call.** The script does its own
  internal `ssh "$LOCK_TARGET_HOST"` round-trip so the lock check/mutate runs atomically
  server-side. Nesting it (running it from inside a shell already SSH'd into Node1) makes
  Node1 try to SSH to itself via the alias `Node1` — an alias that only exists in the
  *local* machine's `~/.ssh/config`, not on Node1 — producing intermittent, confusing
  "Host key verification failed" errors that look like a real host-key problem but
  aren't. Always run `docker_host_lock.sh` directly from the local worktree.
- **A long-running detached Node1 test suite can be silently killed by an unattended
  reboot**, not just by the local session's background-task lifecycle. Node1 rebooted
  twice unprompted during one session (likely `unattended-upgrades`), each time wiping
  `/tmp` (killing the `nohup`-launched suite's log *and* `docker_host_lock`'s lock file — the
  next acquire has to be a fresh `acquire`, not `refresh`, since the lease is gone).
  Detect via `uptime` or a kernel-version change in restarted containers' startup logs,
  not any explicit error. Always wait for an explicit `ALL_DONE` marker before trusting a
  background run finished, and check `ps aux`/`docker ps` for orphaned duplicate
  `docker compose` process trees after any resume — both a reboot and a locally-killed
  wrapper leave the remote side able to keep running (or half-running) undetected.
- **`run_all_tests.sh --e2e` only `git pull`s Node1's checkout — it never checks out a
  feature branch.** Running it while working on an unmerged branch silently E2E-tests
  whatever Node1's checkout is currently on (typically `main`), not the branch under
  test, with no error — the run just "passes" against stale code. To E2E-test a feature
  branch before merging: `git push origin <branch>`, then on Node1
  `git fetch origin <branch> && git checkout <branch>` (verify with `git log --oneline -1`)
  before invoking the script; switch back to `main` (`git checkout main && git pull`)
  afterward. Also watch for a stale scp'd working tree from an earlier `deploy-node1`-skill
  session blocking the pull/checkout (`git status`; `git checkout -- <files>` to discard —
  safe once those files' content is already merged into `main`).
- **Node1's mDNS name is `node1.local`** (lowercase, explicit `host-name=node1` in
  `/etc/avahi/avahi-daemon.conf` — Avahi otherwise falls back to the static hostname
  `Node1` → `Node1.local`, capitalized). Older docs/journal entries said `node1server.local`,
  a fossil from a hostname the box had before an earlier rename to `Node1`; `/etc/hosts`
  still carried two matching stale `127.0.1.1` lines (`old-tinker`, `node1server`) that
  never got cleaned up at rename time. `nslookup name.local` will report "non-existent
  domain" even when the name is fine — `nslookup` only queries regular DNS, not mDNS;
  verify `.local` names with `curl`/a browser instead.
- **A background shell task's local tracker can die early** (observed on both a multi-minute
  `ssh`-wrapped sampling loop and a `docker compose build`) **without the remote work actually
  stopping.** `ssh Node1/Node2 "docker compose build ..."` runs server-side in the Docker
  daemon and survives the local SSH client disappearing — check `ps aux` on the remote host
  before assuming a "killed" notification means the build failed. A bare shell loop
  (`for ...; do ...; sleep 30; done`) run directly over `ssh`, by contrast, has no life
  independent of that SSH session and dies with it. For anything that must survive, launch it
  with `nohup ... </dev/null >logfile 2>&1 & disown` *on the remote host itself*, then poll the
  logfile — don't rely on the local background-task tracker for long-running remote loops.
- **Nested shell quoting through `ssh "..."` → `bash -c "..."` → escaped `awk`/heredocs is
  fragile enough to silently produce empty output** (e.g. a field that should hold a number
  comes back blank) **rather than fail loudly.** Burned two full sampling attempts before
  switching to writing the script as a plain file with the `Write` tool and `scp`-ing it over —
  sidesteps the quoting entirely and should be the default for any multi-line remote script.

## Fleet Memory Diagnosis — malloc_trim after MILP Solves (BL-fleet-memory, 2026-08-02)

- **A 10x memory spread across identically-configured VEN containers correlated almost
  exactly with `solver_ms`** (the MILP solve duration logged per plan cycle), not with
  uptime, database/WAL file size, or profile/state.json size. Harder solves (bigger asset
  mix → bigger HiGHS problem) leave proportionally more resident memory behind. Check the
  per-instance workload metric first, before reaching for heap-profiling tools, whenever
  "identical" containers show wildly different RSS.
- **`pmap -x`'s resident/dirty-vs-virtual split distinguishes "bigger working set" from
  "bigger reservation."** Virtual size reserved was similar (~525-700 MB) across VENs with
  30 MB vs 330 MB RSS — only the *touched* (dirtied) portion differed, confirming the
  variance was in how much of the reserved space a solve actually used, not in what was
  requested upfront.
- **glibc's malloc does not return a thread's freed-but-dirtied heap pages to the OS on its
  own** — RSS ratchets up to a solve's high-water mark and plateaus there indefinitely. This
  looks exactly like a leak from outside (steady climb, never drops) but isn't one: a
  10-minute observation window wasn't long enough to see the plateau-then-step pattern and
  wrongly looked like unbounded growth; a 30-minute trace across multiple full solve cycles
  was needed to confirm it. Fix: call `libc::malloc_trim(0)` on the same thread right after
  the allocation-heavy work completes (here: the `tokio::spawn_blocking` closure running the
  HiGHS solve, in `VEN/src/services/planning.rs`) — verified this returns RSS to a flat
  baseline within 15-45s of each solve instead of leaving it at the peak.

## Real Measurements Feed the Planner Forecast Indirectly (real-measurement-mqtt × BL-14, 2026-08-04)

- **Two independently-built features compose into one nobody designed on purpose.** The
  real-measurement-mqtt feature (live-tick-only, by design) and the pre-existing
  `learn_asset_heuristics` daily job (WP5.2/BL-14, trains the planner's base-load forecast
  from `tick_samples` history) were never meant to interact — but `tick_samples` records
  whatever `entry.last_power_kw` was that tick, without caring whether it came from a real
  measurement or the synthetic heuristic. So once a measured baseline-load reading starts
  winning the live tick's substitution, it starts silently flowing into the planner's
  *forecast* too, not just the live "now" value — no code change needed for that to happen.
  Before shipping a "make X real" feature, check whether an existing history/learning
  pipeline already downstream-consumes X's output; the two may already compose without
  anyone asking them to.
- **No measured/synthetic provenance tag on stored samples is a real gap, not a hypothetical
  one.** `tick_samples` has no column distinguishing "this row came from a live MQTT reading"
  vs. "this row is the synthetic fallback because the feed was stale." A feed outage
  silently re-mixes synthetic-era behavior back into the EWMA-weighted learned heuristic for
  up to the full `rolling_window_days` (42) afterward, with no way to audit which samples
  caused it. Not fixed here (deliberately out of scope — see
  `docs/architecture/real_measurement_mqtt.md`'s "Indirect path into the forecast" section)
  but worth remembering before treating a "converged" learned heuristic as trustworthy after
  any known outage window.

## Container Restarts Masqueraded as Mystery PV Injections (ven-1, 2026-08-05 & 2026-08-07)

- **A months-long "unexplained `/sim/inject` call" mystery on production ven-1 was never an
  HTTP call at all.** Root cause: `main.rs`'s graceful-shutdown handler only listened for
  `tokio::signal::ctrl_c()` (SIGINT). `docker stop` / `docker compose up -d` (container
  recreate) send SIGTERM, which that handler never caught — so a routine redeploy never ran
  the final `simulator::persist::save()`, leaving `state.json` up to `persist_every_s` (15s
  for ven-1) stale relative to the continuously-decaying `PvSmoothingState.irradiance_offset`.
  Reloading that stale, larger-magnitude offset on the next start produced an instant step in
  `pv.power_kw` indistinguishable from a fresh external inject — same signature (single-tick
  step, ~40min decay per `pv_alpha=0.1`) as a real `/sim/inject` call.
  Confirmed by directly correlating a `/history/ticks` jump timestamp with the operator's own
  `docker compose up -d` timestamp for an unrelated deploy — the two matched to the second.
  Fix: also listen for `tokio::signal::unix::signal(SignalKind::terminate())` via
  `tokio::select!` alongside `ctrl_c()`.
- **A targeted "who called this endpoint" logging fix (source IP + payload on `/sim/inject`,
  `ee6013b`) correctly showed *zero* calls during the incident window — and that absence was
  the actual diagnostic signal, not a monitoring gap.** When an endpoint-level trace shows
  nothing, the next place to look isn't "maybe the logging missed it" — it's every other code
  path that can produce the same observable state without going through that endpoint
  (here: process restart + stale persisted state). Don't discard a clean negative signal from
  a tracing fix just because the anomaly still occurred.
- **`docker stop`/`compose up -d` sending SIGTERM (not SIGINT) is an easy blind spot for any
  Rust service using `tokio::signal::ctrl_c()` alone for graceful shutdown** — it works
  perfectly under `Ctrl+C` in a dev terminal, which is exactly the scenario least likely to
  ever run in production, masking the gap until a docker-orchestrated restart exposes it.
- **Round 3 (2026-08-07, same day): a genuine `/sim/inject`-shaped PV step recurred on a
  long-uptime container (up since 12:27Z, no restart), from the operator's own dev-laptop
  IP but explicitly not a manual action — root cause still unresolved.** Mitigated with
  defense-in-depth rather than a fix: a `simulator.sim_inject_enabled` profile flag
  hard-disables the endpoint on production ven-1, and every caller (VEN UI, E2E `ven_post()`)
  now self-tags a `source` field logged with the request, so the next occurrence (if the
  endpoint is ever re-enabled) is traceable to a call site immediately.
- **Redeploying a container to ship a fix mid-incident destroys that container's own log
  evidence, with no way to recover it.** A second PV step happened during this same
  investigation, before the fix was deployed — but redeploying (`docker compose up -d`
  recreates the container) deleted the old container along with its `json-file` driver log,
  and Node1 runs no log aggregator (only `telegraf` for metrics). The persisted
  `/history/ticks` data survived and proved the step happened, but the HTTP-level
  `peer`/`source` trace for that specific event is gone forever. **Always capture
  `docker compose logs <service> > file` before any redeploy while actively investigating a
  live incident**, even if the redeploy itself is the fix.
- **Round 4 — actual root cause: a "skip if unreachable" integration test
  (`pv_irradiance_one_shot.test.ts`) had a fallback default that pointed straight at
  production hardware.** The intent ("skip cleanly in CI, run for real when a dev points
  it at a live VEN") was sound; the mistake was making the *unset* case default to
  Node1's real `ven-1` instead of failing safe (skip). On a LAN where every dev/AI
  session's laptop can reach Node1, "opt-in via env var" quietly became "opt-out via env
  var" — every plain `npm test` from every parallel branch (across three-plus rounds of
  this mystery, from the original 2026-08-05 reports onward) silently exercised
  production. **Any test with a live-server fallback must fail toward "skip," never
  toward "target whatever's reachable."** Found by correlating logged attempt timestamps
  against Node1's git history (`scripts/correlate_ven1_inject.sh`) — every attempt landed
  2-25 minutes before a commit, the "test then commit" cadence of ordinary dev work, not
  a mystery process. The disable-gate and source-tagging from round 3 didn't fix this,
  but they're why nothing broke while it was being found: the gate blocked every write,
  and the archived/live logs are what made the correlation possible at all.
- **A shared checkout can have its branch switched by a concurrent session mid-task.**
  While fixing this, a commit landed on `fix/plan-power-stack-grid-export` instead of
  `main` because another session working in the *same* directory (not a separate
  worktree) had checked out that branch — discovered only by noticing `git push origin
  main` reported "Everything up-to-date" after a real commit. Recovered via a temporary
  `git worktree add` + `cherry-pick` + push, without touching the shared checkout's
  branch or working tree at all, so as not to disturb the other session's in-progress
  work. Always confirm the current branch before assuming a commit went where intended
  in a shared directory.

## Chart Cursor/Tooltip Correctness (openspec/changes/unified-chart-primitives/, 2026-08-08)

- **Recharts resolves a hovered tooltip's value by array index, not by re-matching
  timestamps across series.** Any chart plotting two series from two separately-indexed
  `data` arrays (e.g. a 1-minute actual line and its own 5-minute forecast overlay) risks
  showing one series' real value next to another series' value from an unrelated timestamp.
  This bug class was fixed twice, independently, in two different files
  (`AssetTimelineChart` in `117b44f`, `StackedAreaChart` earlier in `f7b911e`) before being
  recognized as one root cause and fixed structurally: fold every series into ONE
  timestamp-keyed row array before rendering, and give every `<Line>`/`<Area>` a `dataKey`
  accessor into that single array — never its own independent `data` prop. The fix is a
  contract enforced by the composition component's own shape (no prop path exists for a
  per-series array), not a convention documented and hoped for.
- **A regression test that only checks a derived property (e.g. "is the axis span narrow
  enough?") can pass even when the underlying computation is still wrong.** The first fix
  for `TariffChart`'s squeezed axis (splitting tariff onto its own `<YAxis>`) was tested with
  `expect(tMax - tMin).toBeLessThan(1)` — which passed even though the axis still used
  `minSpanDomain` (0-anchored), silently reintroducing a milder version of the exact squeeze
  the split was meant to fix (an always-positive ~0.04-wide series got a `[0, 0.32]` domain
  instead of `[0.28, 0.32]`). A code-review pass caught it days later. Prefer asserting the
  actual bounds a fix is supposed to produce over a bound that merely happens to be satisfied
  by both the correct and incorrect implementation.
- **A domain-flooring helper's "anchor at 0" behavior is only correct for quantities with a
  genuine zero baseline (a rate that can swing through "no cost"), not for a strictly-positive
  price.** `minSpanDomain` (seeds `dataMin`/`dataMax` at 0, widens outward) is right for
  cost-rate/CO2-rate axes; a second function, `tightSpanDomain` (fits tightly to real data,
  widens symmetrically around the data's own center only when necessary, never touches 0),
  was needed for tariff and CO2-intensity axes. Two helpers with different anchor semantics,
  not one helper with a flag — the doc comment on each explains which quantities it's for and
  why, so a future caller doesn't have to guess.

## OpenSpec `validate --strict`: hard-wrapped requirement text hides SHALL/MUST (2026-08-12)

- **The `openspec` CLI's requirement parser only reads the first physical line after `###
  Requirement:` as the requirement's text** — it does not join a hard-wrapped paragraph before
  checking for SHALL/MUST. A requirement written as prose spanning several lines (each ending in
  a real `\n`, common when a proposal is authored with manual line wrapping around ~100 cols)
  fails `validate --strict` with `must contain SHALL or MUST` whenever the SHALL/MUST verb happens
  to land on the second or later physical line, even though the full paragraph clearly states the
  requirement.
- **Fix**: reflow the requirement's opening sentence so it leads with the SHALL/MUST clause
  (e.g. "The system SHALL emit ... when X transitions...") and keep that clause on the first
  physical line — a pure rewording, no semantic change. Don't rely on the paragraph "reading
  fine" as prose; the validator is line-1-only regardless of how the rendered markdown looks.
- **Practical habit**: run `openspec validate <change> --strict` (and `--json --deltas-only` to
  see exactly what text the parser extracted) right after authoring a spec delta, before treating
  the proposal as ready — this class of failure is invisible on a plain read-through.
- **A third recurrence of the same root cause, one layer up the stack**
  (`openspec/changes/unify-plan-power-stack-grid/`, 2026-08-09): where the above bullets are
  about a chart component plotting two independently-indexed *arrays*, this was two
  independently-*computed* single values — `PlanPowerStack.tsx` re-derived grid power from
  `usePlan()`'s raw `Plan` object (`slot.net_import_kw` alone, dropping `net_export_kw`)
  instead of reusing the already-correct `net_import_kw - net_export_kw` the backend computes
  once (`controller/timeline.rs`) and that `GridAccumulatedCell.tsx` already consumed
  correctly via `useAllTimelines()`. Same underlying failure mode as the tooltip-index bug:
  two call sites independently reimplementing "raw data → the value actually shown," one of
  them silently wrong, with nothing structural preventing a third. The fix wasn't patching the
  one-line bug in place — it was deleting the second implementation and pointing both
  consumers at the one that was already correct. When two components need the same derived
  value from the same upstream source, that is a signal to share the derivation, not a
  coincidence to leave alone.
- **Building "one universal chart component" was explicitly rejected in favor of a shared
  primitives kit plus three named compositions** (`TimeSeriesChart`, `StackedTimeSeriesChart`,
  `CurveChart`) — forcing genuinely different shapes (stacked areas with net-value tooltip
  re-aggregation; a non-temporal X-axis) through one component's prop API would have relocated
  duplication into branchy config instead of removing it. Migrating the raw-diagnostics charts
  surfaced a 4th, real shape mismatch (`SimProfileChart`'s categorical X-axis) — left as its
  own small component rather than forced into either composition. Not every chart belongs in
  the same abstraction just because it's a chart.
- **A recharts component mock can render its own `content`/`children` prop instead of
  returning `null`, making a genuinely interactive test possible in jsdom without a full
  recharts render.** `chart-legend-toggle`'s tests mock `recharts`' `<Legend>` as
  `(props) => props.content ?? null` — since `content` is already a real React element
  (`<ChartLegend .../>`) built by the composition, rendering it directly puts real,
  clickable checkboxes in the DOM. This let tests click a checkbox and assert the resulting
  `hide` prop on the mocked `<Line>`/`<Area>`, verifying the actual interaction rather than
  just inspecting static props — same class of technique as the existing `ReferenceArea`/
  `XAxis` prop-capturing mocks, extended to be interactive where the component under test
  needs it to be.

## Reactive-Correction Notifications & RingBuffer<T> (BL-37 + R-46, 2026-08-11)

- **A `len() >= capacity` eviction guard is silently wrong at `capacity == 0`, and only a
  dedicated zero-capacity test catches it.** `RingBuffer<T>::push`'s first draft evicted when
  `self.items.len() >= self.capacity` before pushing — correct for capacity ≥ 1, but at
  capacity 0 that's `0 >= 0` (true), so `pop_front()` on an already-empty deque is a no-op and
  the following `push_back` still lands, leaving the buffer holding 1 item instead of 0.
  Nothing about the three real call sites (all capacity ≥ 100) would ever have exercised this,
  since a generic reusable type invites capacities its current callers don't use. The
  capacity-0 test (part of the task list's required test set, written before the
  implementation per test-first) failed immediately and unambiguously
  (`left: 1, right: 0`) — general-purpose infrastructure code needs its edge cases tested
  even when no current caller hits them, because the whole point of extracting it is that a
  *future* caller will.
- **Edge-triggered notification producers should key on the boolean shape of a transition
  (`is_some()`), not on full value equality, when the underlying condition can hand off
  between values without ever going false.** The deviation arbiter can hand a sustained
  correction from one lever to another (e.g. battery hits its SoC bound, `heater_pause` takes
  over) without the correction itself ever clearing — `Some("battery") -> Some("heater_pause")`
  is a lever handoff, not an edge, and a notification producer keyed on `Option<String>`
  equality would wrongly treat it as clear-then-reactivate (two notifications, one spurious).
  `notify_correction_edge` compares `prev.is_some()` vs `current.is_some()` instead. The
  corollary: once a producer is keyed this coarsely, its message text must not embed the
  specific value that can change without an edge (no `active_lever` in the notification body) —
  otherwise a handoff leaves an already-emitted notification's text stale. Detail that *does*
  vary per-tick stays on a separate, already-existing richer surface (`GET
  /arbiter-diagnostics`) rather than being force-fit into the edge-triggered message.
- **A step definition can sit unused in a steps file for a long time and still be exactly the
  right one to reuse later.** `I inject base_load_kw {kw} with alpha {alpha} via sim inject`
  and `within {N} seconds the VEN sim battery power_kw is less than {threshold}`
  (`dispatcher_steps.py`, "Layer 1 — reactive battery correction" section) were written for a
  since-twice-removed deviation-absorber feature and referenced by zero `.feature` files for
  weeks — but the HTTP mechanics they wrap (`/sim/inject`, `/sim`) are feature-agnostic, so the
  first step was still exactly right for exercising the current arbiter. Grep the steps
  directory for existing step text before writing a new one with the same intent, even (or
  especially) if no `.feature` file currently references it — an orphaned step can mean "not
  yet reused," not "dead and safe to duplicate."
