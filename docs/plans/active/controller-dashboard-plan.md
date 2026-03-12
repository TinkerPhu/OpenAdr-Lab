# Controller Dashboard Page — Implementation Plan

## Context

The VEN HEMS controller (Stages 1–6) is fully implemented with rich data available from multiple endpoints (`/plan`, `/rates`, `/capacity`, `/packets`, `/ledger`, `/requests`). The current UI has no page that shows this data visually. The goal is a single new **Controller** page that transparently shows what the controller is doing:

- Past actual behavior (from `GET /trace`)
- Present state (from `GET /sim`, `GET /capacity`)
- Future plan (from `GET /plan` slots + `GET /rates` rate forecast)
- Active HEMS tasks + ledger summary

---

## Page Layout

```
┌──────────────────────────────────────────────────────┐
│ Controller                                            │
├──────────────────────────────────────────────────────┤
│ Status Bar: [Capacity Card] [Plan Card] [Packets Card]│
├──────────────────────────────────────────────────────┤
│ POWER CHART (syncId="ctrl")                          │
│  Y: kW (import +, export -)                         │
│  Past lines (solid): trace ev/heater/pv/net         │
│  Future lines (dashed): plan allocations per asset  │
│  Step lines: import_cap, export_cap                 │
│  Red dashed ReferenceLine at NOW                    │
├──────────────────────────────────────────────────────┤
│ RATE + CAPACITY CHART (syncId="ctrl")               │
│  Y-left: €/kWh (import_price, export_price)        │
│  Y-right: g/kWh (co2)                              │
│  Step areas from /rates snapshots                   │
│  Capacity limit reference lines from /capacity      │
│  Red dashed ReferenceLine at NOW                    │
├──────────────────────────────────────────────────────┤
│ ACTIVE PACKETS / REQUESTS TABLE                     │
│  Asset | Status | Fill% | Deadline | Est. Cost €   │
├──────────────────────────────────────────────────────┤
│ ENERGY LEDGER                                       │
│  Asset | Import kWh | Export kWh | Cost € | CO2 g  │
└──────────────────────────────────────────────────────┘
```

---

## Data Strategy

### Time axis
Two chart panels share `syncId="ctrl"` — recharts syncs cursor/tooltip across both automatically.
Both use numeric `ts` (Unix ms) as `dataKey` on XAxis, formatted via `tickFormatter={v => new Date(v).toLocaleTimeString()}`.

### Power chart data
1. **Past points** from `GET /trace?limit=500` (reversed to chronological):
   - `trace_ev` = `entry.setpoints.ev_charge_kw`
   - `trace_heater` = `entry.setpoints.heater_kw`
   - `trace_pv` = `entry.setpoints.pv_export_limit_kw` (null → undefined)
   - `trace_net` = ev + heater − pv (computed)

2. **Future points** from `GET /plan` — one point per `firm_slot` + `flexible_slot`:
   - `plan_ev` = sum of `allocation.power_kw` where `asset_id` contains `"ev"`
   - `plan_heater` = sum where `asset_id` contains `"heater"`
   - `plan_pv` = sum where `asset_id` contains `"pv"`
   - `plan_net` = `slot.net_import_kw`
   - `import_cap` = `slot.import_cap_kw`
   - `export_cap` = `slot.export_cap_kw`

### Rate chart data
From `GET /rates` → `PlannedRates.snapshots`, one point per `interval_start`:
- `import_price` = `import_price_eur_kwh`
- `export_price` = `export_price_eur_kwh`
- `co2` = `co2_g_kwh`

Capacity limits from `GET /capacity` rendered as `ReferenceLine` at `y=import_limit_kw` and `y=export_limit_kw`.

### Status bar
| Card | Data source | Shows |
|------|------------|-------|
| Capacity | `GET /capacity` | import_limit_kw, export_limit_kw, last_updated |
| Plan | `GET /plan` | trigger, created_at, warning count, firm_summary.total_cost_eur |
| Packets | `GET /packets` | counts by status: ACTIVE, PENDING, SCHEDULED |

---

## Files to Create / Modify

### 1. `VEN/ui/src/api/types.ts` — add HEMS types

```typescript
RateSnapshot, PlannedRates
OadrCapacityState
EnergyPacket, PacketStatus, EnergySnapshot, ValueCurve, ComfortRate, DeadlineTier
UserRequestMode, CompletionPolicy
Plan, PlanTimeSlot, PlanningHorizon, PacketAllocation
FlexibilityEnvelope, PlanWarning, PlanTrigger
AssetLedger
UserRequest, RequestDeadline, UserRequestStatus
```

### 2. `VEN/ui/src/api/client.ts` — add 7 methods

| Method | Endpoint | Notes |
|--------|----------|-------|
| `packets()` | `GET /packets` | `→ EnergyPacket[]` |
| `plan()` | `GET /plan` | `→ Plan \| null` (null on 204/404) |
| `rates()` | `GET /rates` | `→ PlannedRates` |
| `capacity()` | `GET /capacity` | `→ OadrCapacityState` |
| `ledger()` | `GET /ledger` | `→ AssetLedger[]` |
| `requests()` | `GET /requests` | `→ UserRequest[]` |
| `flexibility()` | `GET /flexibility` | `→ FlexibilityEnvelope[]` |

### 3. `VEN/ui/src/api/hooks.ts` — add 6 hooks

| Hook | refetchInterval |
|------|----------------|
| `usePackets()` | 10 000 ms |
| `usePlan()` | 10 000 ms |
| `useRates()` | 30 000 ms |
| `useCapacity()` | 10 000 ms |
| `useLedger()` | 30 000 ms |
| `useRequests()` | 10 000 ms |

### 4. `VEN/ui/src/pages/Controller.tsx` — new file (~400 lines)

| Component | Purpose |
|-----------|---------|
| `StatusBar` | 3 MUI Paper summary cards in a Grid |
| `buildPowerChartData(trace, plan)` | Pure fn → `ControllerPowerPoint[]` |
| `buildRateChartData(rates)` | Pure fn → `RateChartPoint[]` |
| `PowerChart` | `ComposedChart syncId="ctrl"` — solid past, dashed future, cap step-lines, NOW reference |
| `RateChart` | `ComposedChart syncId="ctrl"` — step areas for prices/CO2, capacity reference lines, NOW reference |
| `PacketsTable` | MUI Table with inline fill-% progress bar |
| `LedgerTable` | MUI Table of per-asset totals |
| `ControllerPage` | Top-level, assembles all sections via `Stack spacing={3}` |

### 5. `VEN/ui/src/App.tsx` — minor additions

- Import `ControllerPage`
- Add nav button: `<Button component={Link} to="/controller">Controller</Button>`
- Add route: `<Route path="/controller" element={<ControllerPage />} />`

---

## Chart Color Scheme

| Series | Color | Style |
|--------|-------|-------|
| EV (actual) | `#1976d2` blue | solid line |
| Heater (actual) | `#ed6c02` orange | solid line |
| PV (actual) | `#f5c518` yellow | solid line |
| Net import (actual) | `#616161` grey | solid, strokeWidth 2 |
| EV (planned) | `#1976d2` blue | dashed line |
| Heater (planned) | `#ed6c02` orange | dashed line |
| PV (planned) | `#f5c518` yellow | dashed line |
| Import cap | `#7b1fa2` purple | step, thin |
| Export cap | `#388e3c` green | step, thin |
| Import price | `#0288d1` cyan | step area, low opacity fill |
| Export price | `#00897b` teal | step area, low opacity fill |
| CO2 | `#9e9e9e` grey | step line, right Y-axis |
| NOW reference | `#f44336` red | dashed vertical ReferenceLine |

---

## Packet Fill Bar

Each row in the packets table shows an inline progress bar:
- Value: `estimated_completion * 100`%
- Color: green ≥ 80%, orange ≥ 40%, red < 40%

---

## Scope Notes

- No new BDD/integration tests (UI-only addition; existing 123-scenario suite unaffected)
- No existing pages modified except `App.tsx` (nav + route)
- `Simulation` page stays as-is — override sliders remain there
- `Controller` page is **read-only** — no write/mutation actions

---

## Verification

1. `cd /c/DriveD/Tinker/OpenAdr-Lab/VEN/ui && npm run build` — must compile clean
2. Deploy: `git push && ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose -f VEN/docker-compose.yml build ven-ui && docker compose -f VEN/docker-compose.yml up -d ven-ui"`
3. Open `http://pi4server.local:8214/controller`
4. Verify:
   - Power chart shows solid past lines + dashed future plan lines with red NOW divider
   - Rate chart shows step price curves from `/rates`
   - Both charts sync tooltip on hover
   - Status bar reflects live capacity, plan, and packet state
   - Packets table shows fill bars and deadlines
   - Ledger table shows per-asset cumulative totals
5. Switch VEN1 → VEN2 → VEN3 in dropdown and confirm all panels update
