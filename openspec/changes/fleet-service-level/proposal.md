## Why

The Fleet plan card compares planned import, export and peak between two objectives, and says nothing about
whether the owners got what they needed. The first fleet switch to the opportunistic objective (2026-10-21)
read as 36 % less planned import while the 10 EVs charged 0 kWh in 16 hours (66-93 kWh in the three days
before): the saving was demand left unserved. The heater has the same blind spot (a tank left below comfort is
noticed late), and so does stored energy: an objective that ends a window with an emptier battery, tank or EV
looks cheaper without being so. Planned energy beside a cost is only a comparison once the service level and the
change in stored energy stand next to it (GB-58).

## What Changes

- Every asset that serves a need declares **unserved energy** [kWh, electric-equivalent]: the energy its owner is
  missing against what the asset's own rules say is due. The EV declares it from the moment it leaves (target not
  reached), the heater while its tank is below the comfort floor or a requested target past its time, the
  shiftable load once its deadline has passed unfinished. The battery serves no owner need and declares none.
- Every asset that stores energy declares **stored energy** [kWh, electric-equivalent]: battery, EV (also while
  away: the energy is in the car), heater tank (above its comfort floor).
- Both declarations ride on the asset snapshot, so they appear in the VEN UI's Devices page and, unchanged, in the
  fleet telemetry the BFF already stores.
- The BFF answers `GET /api/fleet/service?from=&to=` from that stored telemetry: per VEN and per asset, the
  unserved energy that accrued in the window and the change in stored energy (carry-over), summed for the fleet,
  with how many VENs contribute. A window older than the raw telemetry is reported unavailable, never as zero.
- The Fleet page gets a "Service delivered" card beside the plan comparison: unserved kWh and carry-over kWh before
  and after the latest objective switch, over equal-length windows, with a per-VEN, per-asset drill-down.
- A new asset kind (heat pump, air conditioner: R-151) joins by declaring the same two quantities; the report and
  the card do not change.

## Capabilities

### New Capabilities
- `fleet-service-level`: what an asset declares about the energy its owner is missing and the energy it stores,
  how the fleet report accumulates those from stored telemetry, and how the Fleet page shows them next to the
  plan comparison.

### Modified Capabilities
<!-- None: `openspec/specs/` holds no capability for the fleet comparison; `fleet-objective-compare` is an
     open change, not a spec, and its card is extended by this one rather than changed. -->

## Impact

- VEN: `assets/asset_trait.rs`, `asset_handle.rs` (two declarations), `assets/ev.rs` (energy to target shared with
  time to target; a departure latch in `EvState`), `assets/heater.rs`, `assets/shiftable_load.rs`,
  `assets/battery.rs`; `controller/sim_snapshot.rs` (two `AssetSnapshot` fields) and the snapshot builders that
  fill them; generated ts-rs types.
- VEN UI: the asset cards on the Devices page show both figures.
- BFF: `fleet_store.rs` (a query beside `power_series`), a new `fleet_service.rs` (the accumulation), `routes/fleet.rs`
  and `main.rs` (the route).
- VTN UI: `pages/Fleet.tsx`, a new service card component and util, `api/client.ts` and `api/types.ts`.
- Tests: Rust unit tests per asset and for the accumulation; BFF test over synthetic telemetry rows; UI component
  tests; BDD scenario on the VEN's snapshot in `tests/features/`.
- Docs: `docs/architecture/VEN_ARCHITECTURE.md` (the asset declarations), the fleet architecture page under
  `docs/architecture/`, `docs/use-cases/SYSTEM-USE-CASES.md` section 14 and the HEMS observation manual; the GB-58
  row is deleted from `docs/BACKLOG.md`.
- Out of scope, filed when this change is implemented: the departure state of charge per EV and the minutes below
  comfort per heater in the drill-down (they need per-kind episodes, which the kind-free accumulation avoids).
  The VEN's own SQLite history does not store the two figures (see design, Open Questions). The compressor-lockout
  reason a heat pump would add is R-151.
