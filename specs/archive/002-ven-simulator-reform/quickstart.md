# Quickstart: VEN Simulator Reform

**Branch**: `002-ven-simulator-reform`
**Date**: 2026-03-15

## What This Refactor Does

Replaces all hardcoded per-device fields in the VEN simulator with a generic `Vec<AssetEntry>` model. Profile YAML files change from named device blocks to a typed asset list. The HTTP API for `GET /sim` changes to return an `assets` map. Three new endpoints are added for asset reset/config. All existing BDD scenarios must pass unchanged.

## Running the Test Suite

**Prerequisite**: Deploy to Pi4 before running — the test runner bakes source files at build time.

```bash
# From local machine
git push

# On Pi4-Server
ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"

# Run full BDD suite (always use --build when source changed)
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

Run a specific feature to iterate faster:
```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/simulator.feature"
```

## Key File Locations After Refactor

```
VEN/src/simulator/
├── mod.rs              # SimState, SimSnapshot, AssetEntry, AssetSnapshot
├── assets/
│   ├── mod.rs          # AssetState enum, AssetCapabilities, ControlDescriptor, TickEnvironment
│   ├── ev.rs           # EvCharger, EvConfig
│   ├── heater.rs       # Heater, HeaterConfig
│   ├── pv.rs           # PvInverter, PvConfig
│   ├── battery.rs      # Battery, BatteryConfig
│   └── base_load.rs    # BaseLoad, BaseLoadConfig
├── energy.rs           # EnergyCounter (unchanged)
├── persist.rs          # save/load SimState (unchanged logic)
└── power_model.rs      # net power = sum of asset power_kw (simplified)

VEN/src/profile.rs      # Profile.assets: Vec<AssetConfig> (was DeviceConfig)
VEN/src/state.rs        # UserOverrides (3 fields removed)
VEN/src/controller/trace.rs  # AssetHistoryBuffer added (data structure only)
VEN/src/main.rs         # Setpoints bridge: Setpoints → HashMap<String, f64>

VEN/profiles/
├── ven-1.yaml          # Migrated to typed asset list
├── ven-2.yaml          # Migrated to typed asset list
├── ven-3.yaml          # Migrated to typed asset list
└── test.yaml           # Migrated to typed asset list
```

## Verifying the Refactor

After the refactor, verify these conditions manually:

**1. Generic assets map in GET /sim**
```bash
curl http://Pi4-Server:8211/sim | jq '.assets | keys'
# Expected: ["base_load", "battery", "ev", "pv"]  (for ven-1)
# No "ev", "heater", "pv", "battery" at top level
```

**2. Schema endpoint**
```bash
curl http://Pi4-Server:8211/sim/schema | jq 'keys'
# Expected: asset ids with non-empty control descriptor lists
```

**3. Reset endpoints**
```bash
curl -X POST http://Pi4-Server:8211/sim/reset/ev \
  -H "Content-Type: application/json" -d '{"soc": 0.9}'
curl http://Pi4-Server:8211/sim | jq '.assets.ev.values.soc_pct'
# Expected: ~90.0
```

**4. Old stub fields ignored**
```bash
curl -X POST http://Pi4-Server:8211/sim/override \
  -H "Content-Type: application/json" \
  -d '{"ev_initial_soc": 0.5}'
# Expected: 200 OK, field silently ignored (or 400 if deny_unknown_fields)
```

## Adding a New Asset Type (Future Reference)

After this refactor, adding e.g. a `WindTurbine` type requires touching exactly two files:

1. **New file** `VEN/src/simulator/assets/wind_turbine.rs` — define `WindTurbine`, `WindTurbineConfig`, implement `update()`, `predict()`, `state_values()`, `default_setpoint()`, `capabilities()`, `control_schema()`, `reset()`, `update_config()`
2. **Add variants** to `AssetState::WindTurbine(WindTurbine)` and `AssetConfig::WindTurbine(WindTurbineConfig)` in `simulator/assets/mod.rs`
3. Rust compiler enforces exhaustiveness — any missing match arm is a compile error

No changes needed to `SimState`, `tick()`, `to_sim_snapshot()`, `profile.rs`, `persist.rs`, `main.rs`, or any existing asset files.

## Setpoints Bridge (Temporary)

During this speckit, the reactor still outputs a `Setpoints` struct with named fields. A conversion in `main.rs` translates this to `HashMap<String, f64>` before passing to `sim.tick()`. This bridge is removed in speckit 2 when the reactor is refactored.

The conversion maps: `ev_charge_kw → "ev"`, `heater_kw → "heater"`, `pv_export_limit_kw → "pv"`, `battery_kw → "battery"`. `base_load` is non-flexible and never in the setpoints map.
