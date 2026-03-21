# Contract: Planner Interface

**Feature**: 009-backend-timeseries-adoption

## run_planner() — Before

```
Input:
  rates: list of TariffSnapshot (interval_start, interval_end, import?, export?, co2?)
  packets: list of EnergyPacket
  capacity: OadrCapacityState
  profile: Profile
  now: timestamp
  trigger: PlanTrigger
  asset_forecasts: map of asset_id → TimeSeries

Output: Plan
```

## run_planner() — After

```
Input:
  tariffs: TariffTimeSeries (import_eur_kwh, export_eur_kwh, co2_g_kwh — each a TimeSeries with Step interpolation)
  packets: list of EnergyPacket
  capacity: OadrCapacityState
  profile: Profile
  now: timestamp
  trigger: PlanTrigger
  asset_forecasts: map of asset_id → TimeSeries

Output: Plan (unchanged)
```

## TariffTimeSeries — New Type

```
Fields:
  import_eur_kwh: TimeSeries (Step)  — import tariff in EUR/kWh
  export_eur_kwh: TimeSeries (Step)  — export tariff in EUR/kWh
  co2_g_kwh:      TimeSeries (Step)  — CO2 intensity in g/kWh

Constructor: from list of TariffSnapshot
  For each snapshot:
    if import_tariff_eur_kwh is Some → add (interval_start, value) to import_eur_kwh
    if export_tariff_eur_kwh is Some → add (interval_start, value) to export_eur_kwh
    if co2_g_kwh is Some → add (interval_start, value) to co2_g_kwh
  Sort each series by timestamp.
  Duplicates: last-write-wins.
```

## Backward Compatibility

- Plan output structure: unchanged
- PlanTimeSlot fields: unchanged
- Callers of run_planner() must update to pass TariffTimeSeries instead of &[TariffSnapshot]
- Only one call site (main.rs planning loop)
