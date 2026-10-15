# Tasks

Branch: `refactor/pv-base-load-per-asset` (part 1), then a second branch for part 2.

## Part 1: per-asset loops (no profile change)

- [x] 1.1 Test first per row: timeline per forecast asset, forecast publishing per asset,
      base-load learning under another id, residual baseline from SoC, heater anchor per
      thermostat, usage view by id, comms-loss PV ceiling summed.
- [x] 1.2 `PlanTimeSlot::forecast_kw_by_asset`; the rows of the inventory above part 1.
- [x] 1.3 `/sim/config/{asset_id}/battery`, inject targets by asset; UI client; BDD.
- [ ] 1.4 Checks, docs, suites on Node2; merge part 1.

## Part 2: per-array weather and measurement

- [ ] 2.1 Test first: east/west arrays forecast differently; old sections refused; measured
      array only; generation limit split; two base loads smoothed separately.
- [ ] 2.2 Profile schema, validation, migrate every profile; `PvParams::weather`, `measured`.
- [ ] 2.3 Per-asset tick feeds, previews, base-load smoothing, generation-limit split.
- [ ] 2.4 `/weather` per array, UI weather page; BDD.
- [ ] 2.5 Checks, docs, journal, wiki, suites on Node2; delete this change; merge.
