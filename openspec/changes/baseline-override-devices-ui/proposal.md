## Why

`baseline_override` (GET/POST/DELETE `/baseline-override`, `VEN/src/routes/hems/baseline_override.rs`) is a live,
backend-tested capability that lets a user apply per-slot `add_kw` adjustments to the baseline-load
forecast. The VEN UI client method and React Query hooks for it
(`baselineOverride`/`postBaselineOverride`/`deleteBaselineOverride` in `VEN/ui/src/api/client.ts` and
`useBaselineOverride`/`usePostBaselineOverride`/`useDeleteBaselineOverride` in `VEN/ui/src/api/hooks.ts`) were
built but are never called from any page — there is no way for a user to see or set a baseline override
today. This is a `no-half-built-features` gap (BL-42, split off from BL-41 once investigation showed
`baseline_override` is a genuinely standalone capability, not superseded by the unified
`/user-requests` flow like EV/heater/shiftable-load are) and a `ui-transparency` gap (a backend capability
with zero UI surface).

## What Changes

- Add a `BaselineOverrideCard` control to the Devices page (`VEN/ui/src/pages/Devices.tsx`), alongside the
  existing per-device cards (`EvCard`, `HeaterCard`, `ShiftableLoadsCard`, `ComfortCurveCard`,
  `ArbiterSettingsCard`), that lets a user view the active override, edit/add/remove per-slot
  (`slot_start`, `add_kw`) rows, save them via `postBaselineOverride`, and clear the override via
  `deleteBaselineOverride`.
- Wire the card to the existing `useBaselineOverride` / `usePostBaselineOverride` / `useDeleteBaselineOverride`
  hooks — no backend or hook changes required; this is a UI-only addition.
- Add a UI unit test (`VEN/ui/src/__tests__/`) asserting the new card calls `postBaselineOverride` /
  `deleteBaselineOverride` with the edited slots, following the existing `ComfortCurveCard`-style test
  pattern.
- Add a BDD scenario (new `tests/features/ven_ui_devices.feature`, `@ven-ui` tag, testid-driven steps
  matching the `ven_ui_planner.feature` convention) exercising the Devices page baseline-override control
  end to end against the running VEN UI.
- Document the new control in `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (extend the existing
  "Devices" page card list at line ~20 to include "Baseline Override").

## Capabilities

### New Capabilities
- `baseline-override-ui`: the Devices-tab control surfacing the existing `/baseline-override` backend
  capability — viewing the active override, editing per-slot `slot_start`/`add_kw` rows, saving, and
  clearing it.

### Modified Capabilities
(none — `/baseline-override` backend behavior is unchanged; only a new UI consumer is added)

## Impact

- **Affected code**: `VEN/ui/src/pages/Devices.tsx` (mount new card); new
  `VEN/ui/src/components/devices/BaselineOverrideCard.tsx`; new UI unit test under
  `VEN/ui/src/__tests__/`; new `tests/features/ven_ui_devices.feature` (+ step defs if new testids need
  new generic steps, likely reusable from existing testid-based step library); `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`.
- **Not affected**: `VEN/src/routes/hems/baseline_override.rs`, `VEN/ui/src/api/client.ts`,
  `VEN/ui/src/api/hooks.ts`, `VEN/ui/src/api/types.ts` — all already correct and complete for this UI
  addition.
- **Dependencies**: none new; reuses existing MUI components and `@tanstack/react-query` hooks already in
  the codebase.
- **Verification closes BL-42**: after this change, `knip` (or equivalent unused-export check) should no
  longer report `useBaselineOverride`/`usePostBaselineOverride`/`useDeleteBaselineOverride` as unused.
