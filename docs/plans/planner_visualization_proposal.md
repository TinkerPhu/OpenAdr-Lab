# Planner Visualization Proposal

## Context

The VEN HEMS planner is an 8-phase greedy scheduler that runs every 20s (or on trigger) and produces
a `Plan` with per-slot, per-asset decisions each annotated with a `PlanReason` (12 variants:
`CheapTariff`, `ExpensiveTariff`, `FirmObligation`, `SocCeiling`, `SocFloor`, `ComfortBound`,
`UserOverride`, `GridImportLimit`, `GridExportLimit`, `PolicyReserve`, `OpportunityMissed`, `Idle`).

This data is fully available via existing endpoints — no backend changes required:
- `GET /plan` — full plan including `firm_slots[]`, `flexible_slots[]`, `steps[]` (per-slot audit trail), `warnings[]`, `packets[]`, `envelopes[]`
- `GET /packets` — energy packet lifecycle (status, fill %, deadlines, value curve, budget)
- `GET /tariffs` — tariff timeline driving decisions
- `GET /trace/events` — controller event log (PlanCycle triggers, RateChange, OpenAdrArrived)

**Problem**: These endpoints exist but the planner's reasoning is opaque in the UI. The Controller
page shows summary numbers (cost, kWh) but not *why* each slot was scheduled the way it was.
The user cannot trace: "why is the battery charging at 14:00?", "why was this packet deferred?",
"what triggered this replan?"

---

## Visualization Options

### Option A — Decision Heatmap (Time × Asset matrix)

A 2D grid: columns = time slots across the planning horizon, rows = assets (EV, battery, heater…).
Each cell is colored by `PlanReason`. A tariff gradient strip sits above as a header row. FIRM/FLEX
boundary shown as a vertical divider.

| What it emphasizes | What it misses |
|---|---|
| Full audit trail at a glance — every slot, every asset, every rule | Power magnitude (cells are equal size, no sense of kW scale) |
| Pattern recognition — cheap-tariff blocks, SoC-ceiling streaks, idle gaps | Packet identity — can't see which packet is driving a slot |
| Tariff-driven arbitrage is immediately visible (battery color matches tariff gradient) | Budget & deadline urgency — no fill % or remaining budget |
| Compact — 288 slots × N assets fits in a scrollable panel | Dense → needs legend + tooltips; not mobile-friendly |

**Best for**: engineers who want to audit rule-firing correctness.

---

### Option B — Packet Progress Board

Kanban-style cards, one per `EnergyPacket`, organized in status columns
(PENDING → SCHEDULED → ACTIVE → COMPLETED/ABANDONED). Each card shows:
fill gauge (%), active deadline tier, budget remaining bar, next scheduled slot.

| What it emphasizes | What it misses |
|---|---|
| Packet lifecycle and user intent fulfillment | No time axis — can't see *when* in the horizon energy is allocated |
| Deadline pressure and budget burn visible at a glance | No connection to tariff prices driving the schedule |
| Clear status communication (why ABANDONED? which tier expired?) | PlanReason context — doesn't explain *why* a slot was chosen |
| Good for non-technical users asking "is my EV charging?" | Battery arbitrage invisible — battery has no user-facing packets |

**Best for**: end-users and operators who care about request outcomes, not planning mechanics.

---

### Option C — Tariff + Power Timeline (enriched ControllerV2 extension)

Extend the existing ControllerV2 asset timeline chart to overlay planned power (dashed line) with
PlanReason annotations as icon/badge markers at inflection points. Tariff coloring in the chart
background. Clicking a badge opens a reason tooltip.

| What it emphasizes | What it misses |
|---|---|
| Physical power scale (kW magnitude visible) | Full audit trail — only shows inflection points, not every slot |
| Continuity with existing ControllerV2 familiar to users | Cross-asset comparison — one chart per asset, no combined view |
| Past vs. planned overlay already built — minimal incremental work | Doesn't expose packet-level reasoning (why SCHEDULED not ACTIVE?) |
| Tariff background color already in TariffChart.tsx | FIRM/FLEX horizon boundary not clearly communicated |

**Best for**: engineers already using ControllerV2 who want incremental insight.

---

### Option D — Plan Run Detail (single-plan deep-dive panel)

A dedicated panel for the *latest plan run* showing:
- Plan metadata header (trigger, created_at, firm summary)
- Per-slot table: time, tariff, baseline_kw, net_import_kw, allocations, reason (expandable)
- Packet section: packets considered in this plan with fill % and estimated cost
- Warnings section

| What it emphasizes | What it misses |
|---|---|
| Complete transparency for a single plan — every number explainable | Historical comparison — can't see how the plan evolved over time |
| Warnings surface directly (CRITICAL/WARNING severity with messages) | Real-time feel — table of 288 rows is heavy to scan |
| Packet × slot allocation linkage visible (which packet gets which slot) | No visual pattern — tabular only |
| Envelope view: flexibility offers to VTN clearly scoped | Doesn't show *why this plan was triggered* |

**Best for**: debugging a specific plan decision ("why did the planner skip slot 47?").

---

### Option E — Plan Trigger History (trace timeline)

A timeline of `ControllerEvent` entries from `/trace/events`: plan cycles, rate changes,
OpenADR arrivals, packet transitions. Events plotted on a horizontal time axis. Clicking a
`PlanCycle` event loads Option D for that plan.

| What it emphasizes | What it misses |
|---|---|
| Shows causation — "rate changed → plan triggered" | Doesn't show what the planner decided inside each run |
| Reactive-vs-periodic replanning ratio visible | Only the last 500 events in ring buffer (not long-term) |
| Packet status transitions traceable over time | |

**Best for**: understanding *what caused* the planner to act, not *what it decided*.

---

## Recommended Approach — "Planner" Page (combines A + B + D + E)

A new `/planner` tab that layers all four views into a coherent, scannable page. The key insight is
that the four views answer four distinct questions — and a good planner UI should answer all four
without forcing navigation between tabs.

```
┌─────────────────────────────────────────────────────────────────────┐
│  PLAN HEADER                                                         │
│  Last run: 14:03:22  Trigger: RateChange  ⚠ 1 warning               │
│  FIRM: 12h  Cost: €1.84  Import: 8.2 kWh  CO₂: 2.1 kg              │
├─────────────────────────────────────────────────────────────────────┤
│  TRIGGER TIMELINE  (Option E — last 20 events, horizontal scroll)   │
│  ●PlanCycle ○RateChange ●PlanCycle ○OpenAdrArrived ●PlanCycle …     │
├─────────────────────────────────────────────────────────────────────┤
│  DECISION MATRIX  (Option A — scrollable, collapsible)              │
│  Tariff:  [pale][pale][amber][red][red][amber][pale][pale]…          │
│  battery: [Cheap][Cheap][SoC↑][Exp][Exp][Exp][Idle][Idle]…         │
│  ev:      [FirmObl][FirmObl][Idle][Idle][Idle][Usr][Usr][Done]…     │
│  heater:  [Cmft][Cmft][Cmft][Idle][Idle][Cmft][Cmft][Cmft]…        │
│               │← FIRM ───────────────────│─── FLEXIBLE ──│          │
├─────────────────────────────────────────────────────────────────────┤
│  PACKET BOARD  (Option B — horizontal scroll by status)             │
│  [ACTIVE: ev — 62% — €0.44 left — T−2h30m]  [SCHEDULED: battery…]  │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Implementation Detail

### New page: `VEN/ui/src/pages/Planner.tsx`

One page, four stacked sections. All data from existing hooks — no new backend work.

#### Section 1: Plan Header (`PlanHeaderBar`)

- **Source**: `usePlan()` → `plan.created_at`, `plan.trigger`, `plan.firm_summary`, `plan.warnings`
- One-line chip row: trigger badge (color-coded by type), relative timestamp ("3s ago"), firm cost €, import kWh, CO₂ kg, warning count badge
- Warning list collapses below: each entry shows `severity` chip + `message` + `suggested_action`
- Trigger badge colors: `Periodic` → gray, `RateChange` → blue, `CapacityChange` → orange, `UserRequest` → purple, `Event` → teal

#### Section 2: Trigger Timeline (`PlanTriggerTimeline`)

- **Source**: `useTrace()` → `GET /trace/events?limit=50`
- Horizontal scrollable strip, newest on the right, auto-scrolled to right end
- Event chip shapes:
  - `PlanCycle` → filled circle, color = trigger_reason (same scheme as above), label = "Plan"
  - `RateChange` → diamond, blue, label = new import tariff value
  - `CapacityChange` → diamond, orange, label = new import limit kW
  - `OpenAdrArrived` → star, teal, label = event_name truncated
  - `OpenAdrExpired` → star outline, gray
  - `PacketTransition` → right-arrow, color = to_status
- Click chip → popover shows full event JSON
- Visual causation: chips close in time are visually grouped (gap < 2s → nudged together)

#### Section 3: Decision Matrix (`PlanDecisionMatrix`)

- **Source**: `usePlan()` full (not `?summary`) → `plan.steps[]` grouped by `asset_id` + `ts`; `plan.firm_slots[]` for tariff header
- Layout:
  - Fixed left column: asset labels
  - Scrollable right section: time slots as columns
  - Row 0 (header): tariff color gradient (green = cheap, yellow = medium, red = expensive), showing `import_tariff_eur_kwh` per slot
  - Rows 1–N: one row per asset, cells colored by `PlanReason`
  - Vertical divider line at `plan.firm_boundary`: left = FIRM (full opacity), right = FLEXIBLE (50% opacity, dashed border)
- Click any cell → right side-drawer opens showing full `PlanStep`:
  - `ts`, `asset_id`, `setpoint_kw`, `actual_power_kw`
  - `reason` rendered as structured detail (e.g. `CheapTariff { tariff: 0.12 €/kWh, threshold: 0.17 €/kWh }`)
  - `capability`: max_import_kw, max_export_kw, current SoC if applicable
  - `state_before` enum value
- Collapse button hides entire matrix section (288 × N cells is heavy at 5-min slots)
- Zoom toggle: "FIRM only" collapses flexible columns (shows ~36 columns instead of 288)

**PlanReason color + icon legend** (pinned below matrix):

| Reason | Color | Icon | Meaning |
|---|---|---|---|
| `Idle` | gray | `—` | Asset has nothing to do |
| `CheapTariff` | green | `↓€` | Tariff below threshold → charge/consume |
| `ExpensiveTariff` | orange | `↑€` | Tariff above threshold → discharge/shed |
| `FirmObligation` | blue | `⚡` | VTN event or policy mandates this setpoint |
| `UserOverride` | purple | `U` | User request drives this slot |
| `SocCeiling` | amber | `⬆` | Battery/EV SoC hit upper limit → stop charging |
| `SocFloor` | red-dark | `⬇` | Battery SoC hit lower limit → stop discharging |
| `ComfortBound` | teal | `C` | Thermal or SoC comfort bound → forced on/off |
| `GridImportLimit` | pink | `←` | Site import capacity constraint |
| `GridExportLimit` | pink | `→` | Site export capacity constraint |
| `PolicyReserve` | slate | `P` | Flexibility policy default reserve |
| `OpportunityMissed` | red striped | `✗` | Wanted to act but could not (constraint) |

#### Section 4: Packet Board (`PacketProgressBoard`)

- **Source**: `usePackets()` → `EnergyPacket[]`
- Cards grouped into collapsible rows by status group:
  - **Active** (ACTIVE): shown expanded by default
  - **Queued** (PENDING, SCHEDULED): shown expanded
  - **Done** (COMPLETED, PARTIAL_COMPLETED, ABANDONED, FAILED): collapsed by default
- Each card layout:
  ```
  ┌──────────────────────────────────────┐
  │ [asset label]          [status chip] │
  │ Fill ████████░░ 62%    T−2h30m       │
  │ Budget ██████░░░ €0.44/€1.20         │
  │ Target: 5.0 kWh  Desired: 3.5 kW    │
  └──────────────────────────────────────┘
  ```
  - Fill bar: `estimated_completion × 100%`, color = green (>80%) / amber (40–80%) / red (<40%)
  - Deadline countdown: `deadline_tiers[active_tier_index].deadline` → relative time "T−2h30m" or red "OVERDUE"
  - Budget bar: `accumulated_cost_eur` / `deadline_tiers[active_tier_index].max_total_cost_eur` (omit if null)
  - Expand button → shows all `deadline_tiers` as a mini table: tier index, deadline, min_completion %, max_cost
  - ABANDONED/FAILED cards show which tier expired and the final fill %

---

## What This Visualization Answers

| User question | Answered by |
|---|---|
| Why is the battery charging right now? | Decision Matrix cell → `CheapTariff` + threshold value in drawer |
| Why was the EV deferred past 15:00? | Decision Matrix `Idle` cells + Packet Board deadline tier |
| What triggered this replan? | Trigger Timeline → `RateChange` chip before latest `PlanCycle` chip |
| Will my EV finish charging before 07:00? | Packet Board fill gauge + deadline countdown |
| Is the planner respecting the VTN capacity limit? | Decision Matrix `GridImportLimit` cells visible in future slots |
| Are there any warnings about infeasible packets? | Plan Header warning list (collapsed but counted) |
| What does the planner commit to vs. keep flexible? | Decision Matrix FIRM/FLEX boundary divider |
| What is the cost of the current plan? | Plan Header firm_summary cost € |
| Did a rate change cause a replan? | Trigger Timeline shows `RateChange` → `PlanCycle` sequence |
| Is the battery arbitrage working correctly? | Decision Matrix battery row: `CheapTariff` in green columns, `ExpensiveTariff` in red columns |

---

## Files to Create / Modify

| File | Action |
|---|---|
| `VEN/ui/src/pages/Planner.tsx` | Create — main page, four sections |
| `VEN/ui/src/components/planner/PlanHeaderBar.tsx` | Create |
| `VEN/ui/src/components/planner/PlanTriggerTimeline.tsx` | Create |
| `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx` | Create — heaviest component |
| `VEN/ui/src/components/planner/PacketProgressBoard.tsx` | Create |
| `VEN/ui/src/api/hooks.ts` | Modify — ensure `usePlan()` fetches full plan (with `steps[]`, not `?summary`) or add `usePlanFull()` |
| `VEN/ui/src/App.tsx` | Modify — add `/planner` route and tab between Controller and User Requests |

No backend changes required. All data is already exposed via existing endpoints.

---

## Polling / Performance Notes

- `usePlan()` full (with steps) at 10s — `steps[]` for a 24h horizon at 5-min resolution = 288 slots × 3 assets = ~864 step objects. Each is small (~200 bytes JSON). Total payload ~170 KB uncompressed — acceptable.
- Decision Matrix renders up to 288 columns. Default view should be "FIRM only" (≈36 columns) with an expand button for full horizon. This avoids rendering 864 cells on load.
- `useTrace()` at 10s, `usePackets()` at 10s — both already used elsewhere, no extra load.

---

## Verification

1. `docker compose build test-ven-ui && docker compose run --build test-runner features/ven_ui/` — existing UI E2E tests still pass
2. Navigate to `/planner` → Header shows plan trigger badge and firm summary numbers
3. Decision Matrix renders FIRM columns with colored cells and tariff header gradient
4. Hover a cell → tooltip shows reason label; click → drawer shows full PlanStep detail
5. FIRM/FLEX boundary line visible; flexible columns are visibly faded
6. Packet Board shows fill gauges and deadlines matching `/packets` response
7. Trigger Timeline shows chips; newest-right; `RateChange` and `PlanCycle` visible
8. Collapse Decision Matrix → section hides; expand → re-renders
