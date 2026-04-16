# Plan: Fix Battery Allocation + Remove PlanStep

## Context

The "black grid line" in the Controller accumulated power chart shows large negative values (e.g. -4.5 kW) that don't match the sum of displayed asset bars. Root cause: `translate_to_plan()` in `milp_planner.rs` creates `AssetAllocation` entries for EV, heater, and shiftable loads, but **not for battery**. Battery gets only a `PlanStep` entry. The timeline reads `slot.allocations` for future asset power, so battery always renders as 0 kW even when the MILP solver has it discharging at 5 kW.

Fix: add battery `AssetAllocation` to match the grid's `net_export_kw`.

**Secondary cleanup**: `PlanStep` / `Plan.steps` is entirely redundant with `AssetAllocation`:
- The dispatcher only reads `slot.allocations` (never `plan.steps`)  
- `plan.steps` is write-only internally — never consumed by backend logic  
- The frontend `PlanDecisionMatrix` uses `plan.steps` only to derive an asset list and a power-per-slot lookup — both are already available from `slot.allocations`  
- The TypeScript `PacketAllocation` type (with stale `packet_id` field) and the `PlanWarning.packet_id` field are both stale  
- The Rust unit tests in `timeline.rs` import non-existent `PacketAllocation` with `packet_id` — they don't compile under `cargo test`

---

## Changes

### 1. `VEN/src/entities/plan.rs`
- Remove `PlanStep` struct (lines 229–237)
- Remove `Plan.steps: Vec<PlanStep>` field (line 202) and its doc comment (line 201)
- Remove `packet_id` from `PlanWarning` — it has no such field in Rust, but verify the TS type matches

### 2. `VEN/src/controller/milp_planner.rs`
**Bug fix — add battery `AssetAllocation`** (insert after shiftable-loads block, before `// ── Track violations`):
```rust
// ── Battery allocation ────────────────────────────────────────────
if let Some(ref bid) = bat_id {
    let bat_net_kw = sol.p_bat_ch_kw[t] - sol.p_bat_dis_kw[t];
    if bat_net_kw.abs() > 0.01 {
        let (surplus_power_kw, grid_power_kw, cost_eur, co2_g) = if bat_net_kw > 0.0 {
            // Charging: consume PV surplus first, then grid
            let sp = surplus_remaining_kw.min(bat_net_kw);
            let gp = bat_net_kw - sp;
            surplus_remaining_kw -= sp;
            (sp, gp,
             gp * inputs.c_imp_eur_kwh[t] * dt_h - sp * inputs.c_exp_eur_kwh[t] * dt_h,
             gp * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h)
        } else {
            // Discharging: negative power_kw = net injection; revenue = negative cost
            let dis_kw = sol.p_bat_dis_kw[t];
            (0.0, bat_net_kw,
             -(dis_kw * inputs.c_exp_eur_kwh[t] * dt_h),
             -(dis_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h))
        };
        allocations.push(AssetAllocation {
            asset_id: bid.clone(),
            power_kw: bat_net_kw,
            surplus_power_kw,
            grid_power_kw,
            marginal_value: inputs.c_exp_eur_kwh[t],
            cost_eur,
            co2_g,
        });
    }
}
```

**Remove PlanStep creation** — delete the entire `// ── PlanStep entries ──` section (the `if let Some(ref eid)` / heater / `if let Some(ref bid)` + shiftable `steps.push(...)` blocks, lines ~1190–1222).

**Remove `let mut steps = Vec::new()`** (line 1110).

**Update `translate_to_plan` return type** `-> (Plan, Vec<PlanStep>)` → `-> Plan`.

**Update Plan literal**: remove `steps: steps.clone()` from the struct (line 1331), change `(plan, steps)` return to just `plan`.

**Update `fallback_plan` return type** `-> (Plan, Vec<PlanStep>)` → `-> Plan`, change `(plan, vec![])` to just `plan`.

**Update `run_planner` return type** `-> (Plan, Vec<PlanStep>)` → `-> Plan`. Both match arms now return `Plan` directly (no tuple).

**Remove `PlanStep` from imports** (line 20).

**Remove doc comment mention of "PlanStep setpoints"** in `run_planner` doc (line 1342).

**Remove `let _ = surplus_remaining_kw;`** suppressor (line 1231) — it's now actually used in battery charging.

### 3. `VEN/src/loops.rs`
- Line 565: Change `let (mut plan, plan_steps) =` → `let plan =`
- Line 578: Remove `plan.steps = plan_steps;`
- Drop `mut` from `plan` binding if nothing else mutates it in that scope

### 4. `VEN/src/routes/hems.rs`
- Remove `PlanQuery` struct and its `Query(q)` parameter from `get_plan`
- Remove `if q.summary.is_some() { plan.steps = vec![]; }` block
- Change `Some(mut plan)` → `Some(plan)`
- Remove `Query` from `axum::extract` imports (if now unused)

### 5. `VEN/src/controller/timeline.rs` (test module only)
Fix the stale `PacketAllocation` references that prevent `cargo test` from compiling:
- Line 395: Change import `CostBreakdown, PacketAllocation, Plan, ...` → `AssetAllocation, CostBreakdown, Plan, ...`
- Lines 508–510 (`empty_plan` Plan literal): Remove `packets: vec![]` and `steps: vec![]` fields
- Lines 541–549 (`make_slot`): Replace `PacketAllocation { packet_id: Uuid::new_v4(), ... }` with `AssetAllocation { ... }` (no `packet_id` field)
- Lines 645–654 (surplus test): Same replacement
- Lines 1110–1113 and 1159–1163 (two PV test Plan literals): Remove `packets: vec![]` and `steps: vec![]`

### 6. `VEN/ui/src/api/types.ts`
- Rename `PacketAllocation` → `AssetAllocation`; remove `packet_id`; add missing fields (`surplus_power_kw`, `grid_power_kw`, `marginal_value`):
  ```typescript
  export type AssetAllocation = {
    asset_id: string;
    power_kw: number;
    surplus_power_kw: number;
    grid_power_kw: number;
    marginal_value: number;
    cost_eur: number;
    co2_g: number;
  };
  ```
- Update `PlanTimeSlot.allocations: PacketAllocation[]` → `AssetAllocation[]`
- Remove `PlanStep` type and `Plan.steps: PlanStep[]` field
- Remove `packet_id: string | null` from `Plan.warnings` element type (not in Rust struct)

### 7. `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx`
Replace `plan.steps`-based logic with allocations-based logic:

**Imports** (line 9): Remove `PlanStep`; add `AssetAllocation`:
```typescript
import type { AssetAllocation, Plan, PlanTimeSlot } from "../../api/types";
```

**State** (line 54): Change to store allocation + slot start for drawer:
```typescript
const [selectedAlloc, setSelectedAlloc] = useState<{ alloc: AssetAllocation; slotStart: string } | null>(null);
```

**`assetIds` useMemo** (lines 59–67): Replace `plan.steps.map(...)` with allocations-derived set:
```typescript
const assetIds = useMemo(() => {
  if (!plan) return [];
  const UNCONTROLLABLE = new Set(["pv", "base_load"]);
  const ids = new Set(
    plan.slots.flatMap((s) => s.allocations).map((a) => a.asset_id).filter((id) => !UNCONTROLLABLE.has(id))
  );
  ids.add("battery");
  return [...ids].sort();
}, [plan]);
```

**Replace `stepMap` useMemo** (lines 82–92) with `allocLookup`:
```typescript
const allocLookup = useMemo(() => {
  const map = new Map<string, { alloc: AssetAllocation; slotStart: string }>();
  if (!plan) return map;
  for (let i = 0; i < plan.slots.length; i++) {
    const slot = plan.slots[i];
    for (const alloc of slot.allocations) {
      map.set(`${alloc.asset_id}:${i}`, { alloc, slotStart: slot.start });
    }
  }
  return map;
}, [plan]);
```

**Cell onClick** (line 252): Change `const step = stepMap.get(...)` → `const entry = allocLookup.get(...)` and `onClick={() => setSelectedStep(step ?? null)}` → `onClick={() => setSelectedAlloc(entry ?? null)}`.

**Drawer** (lines 340–380): Replace all `selectedStep` / `setSelectedStep` with `selectedAlloc` / `setSelectedAlloc`. Update content:
- `open={selectedAlloc !== null}` / `onClose={() => setSelectedAlloc(null)}`
- Replace `selectedStep.ts`, `.asset_id`, `.setpoint_kw`, `.actual_power_kw` with `selectedAlloc.slotStart`, `.alloc.asset_id`, `.alloc.power_kw`, `.alloc.cost_eur`, `.alloc.co2_g`, `.alloc.surplus_power_kw`, `.alloc.grid_power_kw`
- Guard: `{selectedAlloc ? (...)` instead of `{selectedStep ? (...)`
- Rename drawer title to "Allocation Detail"

### 8. `VEN/ui/src/__tests__/PlanDecisionMatrix.test.tsx`
- Remove `PlanStep` import; add `AssetAllocation`
- Remove `makeStep()` helper; replace test data with `AssetAllocation` objects in `slot.allocations`
- Tests checking "rows from steps" → rewrite to check "rows from allocations"
- Tests for drawer content: update field names (`power_kw` / `cost_eur` instead of `setpoint_kw` / `actual_power_kw`)

### 9. `VEN/ui/src/__tests__/PlannerPage.test.tsx`
- Remove `PlanStep` import
- Remove `steps: []` from `makeMockPlan()` and any test Plan objects

### 10. `VEN/ui/src/__tests__/PlanHeaderBar.test.tsx`
- Remove `steps: []` from `makePlan()` (line 44)
- Remove `packet_id: null` / `packet_id: "pkt-001"` from warning objects (lines 123, 124, 136, 150, 164) — not in Rust struct, now not in TS type

---

## Order of Execution

1. Rust entities first (`plan.rs`) — all other Rust files depend on it  
2. Rust logic (`milp_planner.rs`, `loops.rs`, `hems.rs`, `timeline.rs` test section)  
3. Build/check: `cargo check` in `VEN/` — must be zero errors  
4. TypeScript types (`types.ts`) — all TS files depend on it  
5. Component (`PlanDecisionMatrix.tsx`)  
6. Tests (`PlanDecisionMatrix.test.tsx`, `PlannerPage.test.tsx`, `PlanHeaderBar.test.tsx`)  
7. TS type check: `tsc --noEmit` in `VEN/ui/`  

---

## Edge Cases

- `bat_id` variable (line 1107 in milp_planner.rs) must be **kept** — it's now used for allocation, not just the removed PlanStep
- `surplus_remaining_kw` was previously suppressed with `let _ = ...` — remove that suppressor; it's genuinely used in battery charging path now
- Battery charging case must happen **after** EV/heater/shiftable loads have claimed surplus, so the insertion point (after shiftable loop) is correct
- The dispatcher already has a comment "Battery allocations have no associated packet" in the battery branch — it is already wired to handle battery allocations; no dispatcher changes needed
- `?summary` param in `hems.rs`: frontend never uses it; removing the handler is safe (axum silently ignores unknown query params if no `Query` extractor is present)
- BDD tests (`planner_steps.py`) check allocation **presence** for EV/heater — they will still pass since those allocations are unchanged; battery allocations are additive

---

## Verification

1. **`cargo check`** in `VEN/` — zero errors  
2. **`cargo test`** in `VEN/` — `timeline.rs` unit tests now compile and pass (PacketAllocation references fixed)  
3. **`tsc --noEmit`** in `VEN/ui/` — zero errors  
4. **`npm test`** in `VEN/ui/` — all frontend unit tests pass  
5. **Deploy to Pi4** and query `GET http://localhost:8211/timeline/all`:  
   - Future battery entries must have `power_kw != 0` when MILP plans discharge  
   - Future battery `power_kw` + future base_load + future EV + future PV must sum to the grid's `power_kw` within rounding  
6. **`GET /plan`** — response has no `steps` field  
7. **UI: Controller page** — black grid line in accumulated chart must match the sum of visible asset bars in the forecast section
