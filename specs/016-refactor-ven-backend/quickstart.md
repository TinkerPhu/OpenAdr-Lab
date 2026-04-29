# Quickstart: 016 — Refactor VEN Backend

Developer guide for implementing and verifying this refactoring. Work on branch `016-refactor-ven-backend`.

## Prerequisites

- Rust stable toolchain (`rustup show`)
- `cargo` in PATH
- Docker Compose v2 (for BDD regression)

## Build Check (local, no Docker)

```bash
cd VEN
cargo check 2>&1 | head -40   # fast type-check without codegen
```

After each change group below, run `cargo check` to catch type errors early. Run `cargo test` for the full unit-test suite.

## Implementation Order

Work in this order to keep the codebase in a compilable state after each step.

### Step 1 — Delete dead file (R-01)

```bash
git rm VEN/src/controller/profile.rs
cargo check   # should produce zero errors
```

No code references this file. If `cargo check` errors, a `mod profile;` statement has been added somewhere — it should not exist.

### Step 2 — Create `ids.rs` (R-03 / R-08)

Create `VEN/src/ids.rs` with the 6 constants (see data-model.md §1).

Add `mod ids;` to `VEN/src/main.rs` (after the existing `mod` block).

```bash
cargo check
```

### Step 3 — Replace inline asset-id literals (R-03 / R-08)

Find all literal hits:

```bash
grep -rn '"heater"\|"boiler"\|"ev"\|"battery"\|"pv"\|"base_load"' VEN/src/ \
  --include='*.rs' \
  | grep -v '\.yaml\|test\|//.*"'
```

Replace each non-YAML, non-comment asset-id literal with the corresponding `crate::ids::*` constant. Add the `// TODO(boiler-physics):` comment at `hems.rs:275`.

```bash
cargo check
cargo test -p ven 2>/dev/null || cargo test --manifest-path VEN/Cargo.toml
```

### Step 4 — Remove dead code from `assets/mod.rs` (R-04)

Delete `AssetCapabilities`, `EnergyState`, `TimeWindow`, and the 5 `capabilities()` method impls (see data-model.md §3).

```bash
cargo check   # confirm no remaining callers
```

### Step 5 — Remove `DeviceConfig` from `profile.rs` (R-02)

1. Remove the `#[serde(default)] pub devices: DeviceConfig,` field from `Profile`.
2. Delete the `DeviceConfig` struct and `default_base_load()` fn (the per-asset default fns stay).
3. Simplify the 5 accessor methods (see data-model.md §2 accessor table).
4. Add the startup guard to `try_load()`.
5. Update `main.rs` to call `Profile::try_load(path).await?` directly.

```bash
cargo check
cargo test --manifest-path VEN/Cargo.toml
```

### Step 6 — Split `InnerState` into 3 sub-structs (R-06)

1. Add `PollingState`, `ControllerSimState`, `HemsState` structs to `state.rs` (see data-model.md §4).
2. Replace `InnerState` body with the 3 `#[serde(flatten)]`/`#[serde(skip)]` fields.
3. Replace the manual `Clone` impl with `#[derive(Clone)]`.
4. Update all accessor methods: `inner.field` → `inner.polling.field` / `inner.ctrl_sim.field` / `inner.hems.field` (see accessor table in data-model.md §4).

```bash
cargo check
```

Verify `state.json` round-trip still works:

```bash
cargo test --manifest-path VEN/Cargo.toml -- state
```

### Step 7 — Remove legacy `cancel_request` `None =>` branch (R-07)

Locate the `match session_type` block in `AppState::cancel_request`. Remove the `None =>` arm (see data-model.md §4 R-07 section).

```bash
cargo check
cargo test --manifest-path VEN/Cargo.toml
```

## Full Unit Test Run

```bash
cd VEN
cargo test 2>&1 | tail -20
```

Expected: all tests pass; zero compilation warnings for the files touched.

## BDD Regression (Pi4-Server)

Push the branch and run the full BDD suite on the Pi4:

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab
git fetch && git checkout 016-refactor-ven-backend && git pull

docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
```

All existing scenarios must pass. No new scenarios are required for this refactoring (behaviour-preserving).

## Definition of Done

- [ ] `cargo check` passes with zero errors
- [ ] `cargo test --manifest-path VEN/Cargo.toml` — all tests pass
- [ ] `controller/profile.rs` deleted from git history
- [ ] `DeviceConfig` struct and its `default_base_load()` fn absent from `profile.rs`
- [ ] `AssetCapabilities`, `EnergyState`, `TimeWindow`, 5 `capabilities()` impls absent from `assets/mod.rs`
- [ ] No inline `"heater"`, `"boiler"`, `"ev"`, `"battery"`, `"pv"`, `"base_load"` asset-id literals in non-test `VEN/src/*.rs` files
- [ ] `InnerState` contains exactly 3 fields (`polling`, `ctrl_sim`, `hems`)
- [ ] `state.json` serde round-trip unit test passes (flat JSON key structure preserved)
- [ ] Legacy `None =>` branch absent from `cancel_request`
- [ ] BDD suite passes on Pi4 (or locally with `--local` flag if Pi4 unavailable)
