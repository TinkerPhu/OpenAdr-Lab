# Postponed Features: VEN Controller Dashboard V2

Items explicitly deferred from this feature. Each entry references where it is documented and why it was postponed.

---

## 1. API Rename: `/rates` → `/tariffs` and `RateSnapshot` → `TariffSnapshot`

**Documented in**: `spec.md` (Clarifications), `data-model.md` (header note), `contracts/ui-components.md` (header note), `research.md` (section 7)

**What**: The VEN API endpoint `GET /rates` and its backing struct `RateSnapshot` (plus `PlannedRates`, `PastRates`) use "rate" to mean a per-kWh tariff value. This conflicts with the project nomenclature where "tariff" = X/kWh and "rate" = X/h. The endpoint should be renamed to `GET /tariffs` and the struct to `TariffSnapshot`.

**Why postponed**: Purely a naming correction; no behavioral change. Touches `VEN/src/entities/rate_snapshot.rs`, `VEN/src/main.rs`, the UI hooks (`useRates()`), and all callers. Deferred to avoid scope creep; current code is functional with the naming mismatch annotated.

---

## 2. Replace V1 Controller Page with V2

**Documented in**: `research.md` (section 10), `spec.md` (input description: "final goal to replace the current")

**What**: Remove the existing `/controller` route and `Controller.tsx` page once V2 has passed all acceptance tests and is approved as the primary controller view.

**Why postponed**: V2 launches alongside V1 as `/controller`. Replacement happens only after V2 is stable, tested, and approved. Removing V1 prematurely risks losing working functionality.

---

## 3. Proper Simulation Setting Endpoints (replacing stubs)

**Documented in**: `spec.md` (FR-027), `research.md` (section 4), `data-model.md` (Backend Stub section)

**What**: Three stub fields added to `UserOverrides` in `VEN/src/state.rs` must be replaced with proper implementation:

| Stub Field | Behavior | Proper Replacement |
|---|---|---|
| `ev_initial_soc: Option<f64>` | One-shot jump of EV SoC | Dedicated setter or first-class sim state write |
| `battery_initial_soc: Option<f64>` | One-shot jump of battery SoC | Same |
| `battery_capacity_kwh: Option<f64>` | Override battery capacity | Proper profile/device configuration endpoint |

**Why postponed**: Stubs are sufficient for the dashboard UI to be complete and testable. Proper endpoints require more backend design (profile mutation, persistence). Deferred to the next change as per spec clarification.

---

## 4. Full Baseline / No-Asset Support

**Documented in**: `spec.md` (Edge Cases), `research.md` (section 8)

**What**: Display a proper Baseline cell with a historical/forecast graph, equivalent to an asset cell.

**API gap**: `TraceEntry.setpoints` (`VEN/src/reactor/trace.rs`) only captures reactor-controlled setpoints — `ev_charge_kw`, `heater_kw`, `pv_export_limit_kw`. `base_load_w` is a passive background value the reactor never controls, so it is not recorded per tick. As a result:

| Data | Available? | Source |
|---|---|---|
| Current base load | ✅ | `GET /sim` → `base_load_w` |
| Future base load (forecast) | ✅ | `GET /plan` → `firm_slots[].baseline_kw` |
| Historical base load (past graph) | ❌ | Not in `TraceEntry` — no per-tick record |

**Required backend change**: Add `base_load_kw: f32` to `TraceSetpoints` in `VEN/src/reactor/trace.rs`. The simulator already knows the value at each tick; it just needs to be written into the trace ring buffer.

**Why postponed**: Only the current snapshot value is needed for V2 (the left-section metric). The historical graph for base load requires the trace change above. Deferred to the next phase alongside the simulation stub replacements.

---

## 5. Graph History Window — Full 1-Hour Coverage

**Documented in**: `spec.md` (Assumption A-002), `research.md` (section 2 note), `quickstart.md`

**What**: The spec assumes 1 hour of history in the left half of each asset timeline graph. The current `GET /trace` endpoint buffers ~500 entries at ~1 s/tick = ~8 minutes of actual history. To fill a full 1-hour window, either:
- Increase the trace buffer (configurable limit in `GET /trace?limit=N`), or
- Add a persistent time-series store for asset power history

**Why postponed**: Sparse data is accepted for V2 (graphs render correctly with gaps). Full 1-hour history requires a storage decision that is out of scope here.

---

## 6. Graph Time Window Configurability

**Documented in**: `spec.md` (Assumption A-002: "This default may be revisited during planning")

**What**: The 2-hour total visible window (1h past + 1h future) is a hardcoded default. Future: allow the operator to adjust the time window (zoom in/out, or preset options like 30 min / 2 h / 24 h).

**Why postponed**: A fixed window is sufficient for V2. Configurability adds UI complexity that is not required for the initial release.
