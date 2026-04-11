# Data Model: Planner Visualization Page

**Branch**: `014-planner-viz-page` | **Date**: 2026-04-04

All types are additions or extensions to `VEN/ui/src/api/types.ts`.
No backend changes. Field names match backend serialization verbatim (Constitution I).

---

## New Types

### `PlanReason` (discriminated union)

Represents why the planner chose a setpoint for one asset in one time slot.
12 variants — each carries the numeric parameters that drove the decision.

```typescript
export type PlanReason =
  | { type: "Idle" }
  | { type: "CheapTariff";      tariff_eur_per_kwh: number; threshold_eur_per_kwh: number }
  | { type: "ExpensiveTariff";  tariff_eur_per_kwh: number; threshold_eur_per_kwh: number }
  | { type: "FirmObligation";   source: string; required_kw: number }
  | { type: "UserOverride";     request_id: string; mode: string }
  | { type: "SocCeiling";       soc_pct: number }
  | { type: "SocFloor";         soc_pct: number }
  | { type: "ComfortBound";     asset_id: string; bound_type: string }
  | { type: "GridImportLimit";  limit_kw: number }
  | { type: "GridExportLimit";  limit_kw: number }
  | { type: "PolicyReserve";    policy_id: string }
  | { type: "OpportunityMissed"; reason: string };
```

### `PlanStep`

One planning decision: one asset in one time slot. Forms the audit trail stored in `Plan.steps`.

```typescript
export type PlanStep = {
  ts: string;                    // ISO timestamp of the slot start
  asset_id: string;
  setpoint_kw: number;           // What the planner decided
  actual_power_kw: number;       // What the device actually did
  reason: PlanReason;            // Why (discriminated union above)
  state_before: string;          // Asset state enum value before the decision
  avail_max_import_kw: number;   // Physical import capacity at this step
  avail_max_export_kw: number;   // Physical export capacity at this step
};
```

---

## Extended Types

### `Plan` (extended)

Adds `steps`, the per-slot per-asset audit trail. The backend already returns this field when
`GET /plan` is called without `?summary`. The type was simply incomplete.

```typescript
export type Plan = {
  id: string;
  created_at: string;
  trigger: string;
  firm_slots: PlanTimeSlot[];
  flexible_slots: PlanTimeSlot[];
  firm_boundary: string;         // NEW: ISO timestamp — divides FIRM from FLEXIBLE zone
  firm_summary: FirmSummary;
  warnings: Array<{
    severity: string;
    message: string;
    packet_id: string | null;
    suggested_action: string | null;  // ADDED: was missing from type
  }>;
  steps: PlanStep[];             // NEW: full per-slot per-asset audit trail
};
```

Note: `firm_boundary` is added because the Decision Matrix needs it to draw the FIRM/FLEX divider.
It is already present in the backend response.

---

## UI-Only Types (in component files, not exported from types.ts)

### `ReasonMeta` (in `PlanDecisionMatrix.tsx`)

Lookup table entry for rendering a reason cell. Defined locally in the component, not in the API
layer (it is UI presentation data, not API contract data).

```typescript
type ReasonMeta = {
  label: string;    // Short display label, e.g. "Cheap"
  color: string;    // MUI color token or hex, e.g. "success.main"
  icon: string;     // Unicode icon, e.g. "↓€"
  title: string;    // Full tooltip title, e.g. "Cheap Tariff — tariff below threshold"
};

const REASON_META: Record<string, ReasonMeta> = {
  Idle:               { label: "—",    color: "grey.400",         icon: "—",  title: "Idle" },
  CheapTariff:        { label: "↓€",   color: "success.main",     icon: "↓€", title: "Cheap Tariff" },
  ExpensiveTariff:    { label: "↑€",   color: "warning.main",     icon: "↑€", title: "Expensive Tariff" },
  FirmObligation:     { label: "⚡",   color: "primary.main",     icon: "⚡", title: "Firm Obligation" },
  UserOverride:       { label: "U",    color: "secondary.main",   icon: "U",  title: "User Override" },
  SocCeiling:         { label: "⬆",   color: "warning.light",    icon: "⬆", title: "SoC Ceiling" },
  SocFloor:           { label: "⬇",   color: "error.dark",       icon: "⬇", title: "SoC Floor" },
  ComfortBound:       { label: "C",    color: "info.main",        icon: "C",  title: "Comfort Bound" },
  GridImportLimit:    { label: "←",    color: "error.light",      icon: "←",  title: "Grid Import Limit" },
  GridExportLimit:    { label: "→",    color: "error.light",      icon: "→",  title: "Grid Export Limit" },
  PolicyReserve:      { label: "P",    color: "grey.600",         icon: "P",  title: "Policy Reserve" },
  OpportunityMissed:  { label: "✗",   color: "error.main",       icon: "✗", title: "Opportunity Missed" },
};
```

### `MatrixCell` (in `PlanDecisionMatrix.tsx`)

Derived data for one cell in the matrix. Built by grouping `Plan.steps` by `(ts, asset_id)`.

```typescript
type MatrixCell = {
  ts: string;
  asset_id: string;
  step: PlanStep | null;         // null = no step recorded (treated as Idle)
  slotType: "FIRM" | "FLEXIBLE";
  importTariff: number;          // For column header gradient
};
```

---

## State Transitions

### EnergyPacket status lifecycle (already defined in types.ts)

```
PENDING → SCHEDULED → ACTIVE → COMPLETED
                             → PARTIAL_COMPLETED
                             → PAUSED → ACTIVE (resume)
         → ABANDONED (tier expired or user cancelled)
                             → FAILED (device failure)
```

The Packet Board uses this to assign cards to groups:
- Active group: `ACTIVE`
- Queued group: `PENDING`, `SCHEDULED`, `PAUSED`
- Done group: `COMPLETED`, `PARTIAL_COMPLETED`, `ABANDONED`, `FAILED`

---

## Data Flow

```
usePlan()  ──────────────────────────────────┐
  GET /plan (full, 10s poll)                  ├─► PlanHeaderBar      (trigger, summary, warnings)
  returns: Plan { firm_slots, flexible_slots, ├─► PlanDecisionMatrix (steps[], firm_boundary)
           firm_boundary, steps[], warnings } └──────────────────────────────────────────

useTrace() ──────────────────────────────────► PlanTriggerTimeline  (last 20 events as chips)
  GET /trace/events?limit=20 (10s poll)
  returns: TraceEntry[] (newest first)

usePackets() ────────────────────────────────► PacketProgressBoard  (packet cards by status)
  GET /packets (10s poll)
  returns: EnergyPacket[]
```

No new API calls introduced. No new backend endpoints.
