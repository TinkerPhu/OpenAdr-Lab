# Data Model: Flatten Assets Module (008)

No new data structures are introduced. This document records the existing type ownership
and how it changes after the move.

---

## Type ownership: before → after

| Type | Before (module path) | After (module path) | Changed? |
|------|---------------------|---------------------|----------|
| `AssetState` (enum) | `simulator::assets` | `assets` | Path only |
| `TickEnvironment` | `simulator::assets` | `assets` | Path only |
| `AssetCapabilities` | `simulator::assets` | `assets` | Path only |
| `EnergyState` | `simulator::assets` | `assets` | Path only |
| `TimeWindow` | `simulator::assets` | `assets` | Path only |
| `ControlKind` | `simulator::assets` | `assets` | Path only |
| `ControlDescriptor` | `simulator::assets` | `assets` | Path only |
| `history_from_buffer()` | `simulator::assets` | `assets` | Path only |
| `PvInverter` | `simulator::assets::pv` | `assets::pv` | Path only |
| `Battery` | `simulator::assets::battery` | `assets::battery` | Path only |
| `EvCharger` | `simulator::assets::ev` | `assets::ev` | Path only |
| `Heater` | `simulator::assets::heater` | `assets::heater` | Path only |
| `BaseLoad` | `simulator::assets::base_load` | `assets::base_load` | Path only |
| `AssetEntry` | `simulator` | `simulator` (unchanged) | No change |
| `SimState` | `simulator` | `simulator` (unchanged) | No change |
| `GridMeter` | `simulator` | `simulator` (unchanged) | No change |
| `AssetInterface` | `common` | `common` (unchanged) | No change |

---

## Backward-compatibility bridge

`simulator/mod.rs` will re-export from `crate::assets` so that any call site using
`crate::simulator::AssetState` continues to compile without modification:

```rust
// simulator/mod.rs — after the move
pub use crate::assets::{
    AssetState, AssetCapabilities, TickEnvironment,
    EnergyState, TimeWindow, ControlKind, ControlDescriptor,
    history_from_buffer,
};
```

This re-export can be removed in a later cleanup pass once all call sites have been
updated to reference `crate::assets` directly.

---

## File manifest: before → after

```
VEN/src/simulator/assets/mod.rs      →  VEN/src/assets/mod.rs
VEN/src/simulator/assets/pv.rs       →  VEN/src/assets/pv.rs
VEN/src/simulator/assets/battery.rs  →  VEN/src/assets/battery.rs
VEN/src/simulator/assets/ev.rs       →  VEN/src/assets/ev.rs
VEN/src/simulator/assets/heater.rs   →  VEN/src/assets/heater.rs
VEN/src/simulator/assets/base_load.rs→  VEN/src/assets/base_load.rs
```

All six files are moved verbatim — no content changes.
