---
title: Asset Layer
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_asset_interface_spec.md, VEN/src/assets/, VEN/src/entities/asset.rs]
tags: [assets, abstraction, ven]
---

# Asset Layer

The uniform device abstraction between the HEMS controller and whatever produces the
numbers — simulated physics today, real hardware later
(docs/architecture/VEN_ARCHITECTURE.md §3.0; contract details in
docs/architecture/ven_asset_interface_spec.md).

```
trait AssetInterface {
    fn current(&self) -> f64;                                          // kW now
    fn forecast(&self, horizon: Duration) -> Vec<(DateTime<Utc>, f64)>; // predicted kW
    fn past(&self, window: Duration) -> Vec<(DateTime<Utc>, f64)>;      // recorded kW
}
```

Two implementations: `SimulatedAsset` (PV · Battery · EV · Heater · BaseLoad, backed by
the [[simulator]]) and `MeasuredAsset` (future: real hardware / external APIs). The
controller never calls physics functions or reads simulation parameters directly.

## Planning-side counterpart

For the [[milp-planner]], each asset additionally provides an `AssetMilpContext` —
its constraints and variables in solver terms (`VEN/src/assets/`). The solver only sees
the trait objects, keeping concrete asset types out of the optimisation core
([[ven-hexagonal-architecture]], port obligations).

Sign convention for all power values crossing this interface: positive = import,
negative = export/generation — see [[sign-convention]].
