# Quickstart: Verify the RF-02 Refactor

This guide tells a developer how to verify the refactor is complete and correct.

## Pre-conditions

- You are on branch `008-flatten-assets-module`.
- `cargo build` passes on the baseline branch (`007-asset-forecast-past`).

## Step 1 — Check the new directory exists

```
ls VEN/src/assets/
```

Expected: `mod.rs`, `pv.rs`, `battery.rs`, `ev.rs`, `heater.rs`, `base_load.rs`

## Step 2 — Confirm the old directory is gone

```
ls VEN/src/simulator/assets/
```

Expected: directory not found (no such file or directory).

## Step 3 — Build

```
cd VEN && cargo build
```

Expected: zero errors, zero new warnings.

## Step 4 — Unit tests

```
cd VEN && cargo test --workspace
```

Expected: all tests pass, same count as before the move.

## Step 5 — Integration tests (on Pi4-Server)

```
ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
```

Expected: 895 steps, 0 failures.

## Quick sanity check — no stale imports

```
grep -r "simulator::assets" VEN/src/
```

Expected: zero matches (or only re-export lines in `simulator/mod.rs`).
