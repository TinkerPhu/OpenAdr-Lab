# OpenADR Lab — Copilot Instructions

## Architecture

Full OpenADR 3.0 lab running on Raspberry Pi 4 (ARM64) via Docker Compose. Two independent stacks:

**VTN stack** (`VTN/docker-compose.yml`):
- `vtn` (:8200) — `openleadr-rs` git submodule (TinkerPhu fork of OpenLEADR/openleadr-rs)
- `db` (:8201) — PostgreSQL 16, auto-migrated via SQLx on first boot
- `bff` (:8220) — Rust (axum) Backend-for-Frontend; holds dual OAuth credentials (`any-business` + `ven-manager`) and proxies to the VTN
- `ui` (:8221) — React + MUI operator dashboard

**VEN stack** (`VEN/docker-compose.yml`):
- `ven-1/2/3` (:8211-8213) — Rust (axum + tokio) polling VEN; each instance configured via environment variables and a per-VEN YAML physics profile
- `ui` (:8214) — React + MUI device dashboard

Both stacks share the `vtn_openadr-net` Docker network. `openleadr-rs` is a git submodule — always clone with `--recursive`.

### VEN internals

The VEN is a set of concurrent tokio background loops (spawned in `VEN/src/loops.rs`):
- `spawn_event_poll` / `spawn_program_poll` / `spawn_report_poll` — HTTP polling against the VTN
- `spawn_sim_tick` — physics simulation tick (uses YAML profile)
- `spawn_planning` — LP-based HEMS planner (`good_lp` / HiGHS solver); triggered reactively via a `tokio::sync::watch` channel
- `spawn_obligation_check` — fires report obligations
- `spawn_state_persist` — periodic JSON serialisation to `/data/state.json`

All loops share `AppCtx` (contains `AppState: Arc<RwLock<...>>`, `VtnClient`, `SimState: Arc<Mutex<...>>`, `Profile: Arc<Profile>`).

History is kept as in-memory `VecDeque` ring buffers (3600 rows per asset), not in the database.

## Build & Deploy

```bash
# VTN stack (first build ~25 min on Pi4, ~5 min x86)
cd VTN && docker compose up -d --build

# VEN stack (first build ~11 min on Pi4, ~2 min x86)
cd VEN && docker compose up -d --build

# Seed demo programs/events
python3 scripts/seed_vtn.py --vtn-url http://localhost:8200
```

## Testing

### UI unit tests (local, no Docker)

```bash
cd VEN/ui && npm test          # run once
cd VEN/ui && npm run test:watch # watch mode

cd VTN/ui && npm test
```

Test files live in `src/__tests__/<ComponentName>.test.tsx`.

### Run a single BDD feature or scenario (Pi4-Server)

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab

# Single feature file
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/enrollment.feature

# By tag
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@ui
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@resilience

# Tear down ephemeral stack afterward
docker compose -f tests/docker-compose.test.yml down -v
```

Always pass `--build` when any Python test file has changed (Docker caches the image otherwise).

### Full test suite

```bash
bash run_all_tests.sh              # all suites
bash run_all_tests.sh --local      # VEN + VTN UI unit tests only
bash run_all_tests.sh --e2e        # behave E2E only
bash run_all_tests.sh --resilience # resilience/failure-recovery tests
bash run_all_tests.sh --rust       # openleadr-rs cargo tests
```

Configure `DOCKER_HOST` and `DOCKER_DIR` at the top of `run_all_tests.sh` to point at your Docker machine (empty = local).

### openleadr-rs cargo tests

```bash
cd openleadr-rs
docker compose up db --wait -d
cargo sqlx migrate run --source openleadr-vtn/migrations
psql -U openadr -h localhost openadr < fixtures/test_user_credentials.sql
cargo test --workspace

# Offline (compile-check only, no DB)
SQLX_OFFLINE=true cargo test --workspace
```

## Key Conventions

### BDD test infrastructure

- **Tags**: `@ui` (Playwright VTN UI), `@ven-ui` (Playwright VEN UI), `@resilience` (requires Docker socket), `@upstream_pending` (excluded by `behave.ini` — awaiting upstream fix)
- `before_feature` wipes all VTN programs before each feature; each feature must be self-contained
- `after_scenario` resets VEN sim overrides, device sessions, and restarts any services stopped by resilience scenarios
- Safety guard in `environment.py` prevents tests from running against live (non-`test-*`) hosts unless `ALLOW_LIVE_TESTS=true`
- Test helper modules: `features/helpers/api_client.py` (VTN/VEN/BFF HTTP), `ui.py` (Playwright page objects: `VtnUi`, `VenUi`), `docker_ctl.py` (compose stop/start), `wait.py` (polling retry)

### Playwright timeouts

All `wait_for_selector` / navigation calls must use **≥ 20000 ms** timeout (Pi4 ARM64 is slow under full-suite load).

### React component style (from `docs/guidelines/REACT_GUIDELINES.md`)

```tsx
// Named function, not const FC
export function MyComponent(props: MyComponentProps) {
  const { value, onChange } = props;   // destructure on first line
  ...
}
```

- Define a `Props` type (e.g. `MyComponentProps`), pass as generic on `props`.
- Add `data-testid` to every interactive element and every element that displays data.
- Only add explicit `role` when the rendered element lacks a semantic role (MUI `Button`, `Dialog`, etc. already supply roles).

### React tests

Mock API hooks at the module level with `vi.mock()`. Wrap renders in required providers (`QueryClientProvider`, `BrowserRouter`). Test data rendering, visibility, loading, empty state, and interactions.

### VEN configuration

Each VEN instance is configured entirely via environment variables (see `VEN/docker-compose.yml`). Physics behaviour is driven by a per-VEN YAML profile (`VEN/profiles/ven-N.yaml`). There is no VEN-side database; state is persisted as JSON to `/data/state.json`.

### BFF credential model

The BFF holds two OAuth credentials: `any-business` (read/write to programs/events/VENs) and `ven-manager` (VEN enrollment). Clients call the BFF — they never hit the VTN directly. The BFF selects the appropriate credential per route.

### Windows / subst drive note

If working on Windows with a subst drive (e.g. `D:` → `C:\DriveD`), vitest may fail to resolve modules. Run UI tests from the real path (`C:\DriveD\...`), not the subst path.
