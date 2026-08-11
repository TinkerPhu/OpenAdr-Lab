## Context

`baseline_override` is fully implemented and tested on the VEN backend
(`VEN/src/routes/hems/baseline_override.rs`):

- `GET /baseline-override` → `BaselineOverride | 204`
- `POST /baseline-override` with `CreateBaselineOverrideBody { slots: [{ slot_start, add_kw }] }` →
  `201` + the created `BaselineOverride { id, slots, created_at, updated_at }`, and triggers a replan
  (`PlanTrigger::UserRequest`)
- `DELETE /baseline-override` → `204`, clears the override, triggers a replan

The VEN UI already has a matching, correctly-typed client + hooks layer (no DTO renaming — the UI
types in `VEN/ui/src/api/types.ts` use the same field names as the Rust structs: `slot_start`, `add_kw`):

```ts
// types.ts
export type BaselineSlot = { slot_start: string; add_kw: number };
export type BaselineOverride = { id: string; slots: BaselineSlot[]; created_at: string; updated_at: string };
export type CreateBaselineOverrideBody = { slots: BaselineSlot[] };

// hooks.ts
useBaselineOverride()        // GET, refetchInterval 10s
usePostBaselineOverride()    // POST, invalidates ["baseline_override"] and ["plan"]
useDeleteBaselineOverride()  // DELETE, invalidates ["baseline_override"] and ["plan"]
```

None of `client.ts`, `hooks.ts`, `types.ts`, or the Rust route need to change. The only gap is a
consuming UI component. `VEN/ui/src/pages/Devices.tsx` currently renders five cards in a `Grid`
(`EvCard`, `HeaterCard`, `ShiftableLoadsCard`, `ComfortCurveCard`, `ArbiterSettingsCard`) — each an
independent, self-contained component owning its own local edit state and calling its own
mutation hooks, matching the `declare-dont-branch` / one-component-per-case pattern already used on
this page.

The closest structural precedent is `ComfortCurveCard`
(`VEN/ui/src/components/devices/ComfortCurveCard.tsx`): it fetches a list-of-rows resource, holds an
`edited` local-state overlay (`ComfortRate[] | null`) that mirrors server data until the user edits,
renders each row as an inline `TextField` pair with an add/remove control, and exposes Save / Reset
actions wired to `useMutation` hooks with `mutateAsync`.

## Goals / Non-Goals

**Goals:**
- Give the user a way to view the currently active baseline override (if any), edit its per-slot
  `slot_start` / `add_kw` rows, add/remove rows, save (POST) the edited set, and clear (DELETE) the
  override entirely — all from the Devices page.
- Reuse the existing hooks unchanged; this is additive UI wiring only.
- Match the existing per-card UX and testing conventions on the Devices page (MUI `Card`, local edit
  overlay, `data-testid`s, a colocated Vitest unit test, section state cleared after save/reset).
- Close the BL-42 verification bar: a UI unit test asserts the card calls
  `postBaselineOverride`/`deleteBaselineOverride`; `knip` (or equivalent) stops flagging the three hooks
  as unused; a BDD scenario exercises the control against the live UI.

**Non-Goals:**
- No backend changes to `baseline_override.rs`, its route wiring, or `entities::device_session`.
- No change to how `slot_start`/`add_kw` are named or typed anywhere in the stack (per the `dto` rule,
  the UI keeps the backend's field names verbatim — no `slotStart`/`addKw` camelCase translation layer).
- No new global override-management UX (e.g. a history of past overrides, multi-override support) —
  the backend models exactly one active override at a time (`GET` returns the single active one or
  `204`), and the UI matches that 1:1.
- No change to how the planner consumes `BaselineOverride` — out of scope, already implemented.

## Decisions

**1. New standalone `BaselineOverrideCard` component, not inline in `Devices.tsx`.**
Every other per-device control on the page is already its own file under
`VEN/ui/src/components/devices/`. Keeping `Devices.tsx` a thin composition root (fetch nothing beyond
what it currently fetches, mount cards, pass hook results down) matches the existing pattern and keeps
`Devices.tsx` from growing sxized branching logic. Alternative considered: extend `ComfortCurveCard`'s
asset-selector pattern to include "baseline" as a pseudo-asset — rejected because the data shapes
(`ComfortRate[]` vs `BaselineSlot[]`, per-asset vs. single site-wide override, different mutation
verbs and semantics for "no override") are different enough that forcing a shared component would need
its own branching by kind, working against `generic-over-bespoke` rather than for it.

**2. Local edit-overlay state, mirroring `ComfortCurveCard`.**
`const [edited, setEdited] = useState<BaselineSlot[] | null>(null); const rows = edited ?? data?.slots ?? [];`
This lets the card show live server state until the user starts editing, and reverts to that pattern
(`setEdited(null)`) after a successful Save or Clear — consistent with how `ComfortCurveCard` avoids a
`useEffect` sync between query data and local edit buffer.

**3. Row editor: two `TextField`s per row (datetime-local for `slot_start`, number for `add_kw`), add/remove buttons, matching `ComfortCurveCard`'s row layout.**
`slot_start` is an ISO datetime string on the wire; the input uses
`type="datetime-local"` with explicit conversion to/from ISO 8601 at the edit boundary (the raw string
in local component state, converted only when read from/written to the API types) — no new shared
datetime-parsing utility is introduced since this is the only place in the Devices cards editing an
absolute timestamp (other cards deal in durations/percentages). `add_kw` follows the
`naming` rule (unit-suffixed already, kept as-is, `type="number"` with `step` sized for kW granularity,
e.g. `0.1`).

**4. Save / Clear actions.**
"Save" calls `postBaselineOverride({ slots: rows })` via `usePostBaselineOverride().mutateAsync`, disabled
while pending or when `rows.length === 0` (an empty save is a no-op state the user should reach via
Clear, not Save-with-nothing) — matching `ComfortCurveCard`'s `disabled={... || rows.length === 0}`
guard. "Clear" calls `useDeleteBaselineOverride().mutateAsync()`, disabled while pending or when there is
no active override (`!data`) — surfacing the override's presence/absence directly (`declare-dont-branch`:
button enablement is declared from data, not from a separately-tracked UI flag).

**5. Empty state and status affordance.**
When `data` is `null`/`undefined` (204 from GET, i.e. no active override), the card shows a
"No baseline override active" message, matching `ComfortCurveCard`'s
`{rows.length === 0 ? <Typography>No curve points</Typography> : ...}` empty-state pattern. A small
chip or caption shows `updated_at` when an override is active, giving the user confidence about
freshness (`ui-transparency`: don't just show raw slots, show provenance of the currently-applied
value).

**6. BDD coverage: new `tests/features/ven_ui_devices.feature` file, not an extension of an existing one.**
No existing feature file covers the Devices page through the browser (`ComfortCurveCard`,
`ArbiterSettingsCard`, etc. are covered only by Vitest unit tests today) — there is nothing to extend.
A new file, tagged `@ven-ui` and following the testid-driven step vocabulary already used by
`tests/features/ven_ui_planner.feature` ("Given the VEN UI is open", "When I click ... testid",
"Then I see an element with testid ..."), is added scoped to the Devices page and specifically the new
baseline-override control; it reuses existing generic step definitions (no new step-definition Python
needed unless a baseline-override-specific interaction, e.g. "I fill the baseline slot row", isn't
already covered by the existing generic form-fill steps — checked at implementation time before adding
new step defs).

**7. Use-case doc update, not a new use-case file.**
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` already lists the Devices page's card inventory
(line ~20: "Per-device settings cards (EV overlay, Deviation Arbiter, Heater, Shiftable loads, Comfort
curve)"). This change extends that line to include "Baseline Override" rather than creating a new
use-case doc, since the underlying use case ("a user directly adjusts VEN behavior from the Devices
page") is already documented there and baseline override is one more instance of it.

## Risks / Trade-offs

- [Risk] `slot_start` datetime-local input UX is fiddly (timezone-naive HTML input vs. UTC ISO strings
  on the wire) → Mitigation: convert explicitly at the component boundary using the same
  `Date`/ISO-string conversion idiom likely already used elsewhere in the UI for timestamp inputs (check
  during implementation for an existing helper before writing a new one); cover the conversion with a
  unit test on a couple of representative slot values (midnight UTC, a DST-adjacent time) rather than
  relying on manual QA.
- [Risk] A user could Save with rows the backend has never had to validate before (e.g. duplicate
  `slot_start` values, unsorted order) since this UI is the first real client of the endpoint →
  Mitigation: this is a backend validation concern, not a UI scope change per the Non-Goals section;
  note it as a candidate backlog item if the backend accepts nonsensical input silently, but do not
  add speculative client-side validation beyond basic "row has both fields filled" gating on Save.
- [Trade-off] No shared "editable row list" abstraction is extracted even though `ComfortCurveCard` and
  the new `BaselineOverrideCard` share a similar shape → accepted for now: two instances is not yet a
  pattern per `generic-over-bespoke` (which targets N≥3 near-identical helpers); if a third per-slot
  editor card appears later, extracting a shared row-editor component becomes the right call and should
  be raised then, not speculatively now.

## Migration Plan

Purely additive — no data migration, no flag, no rollback complexity beyond a normal revert. Deploy
order: implement + test in one change (per `no-half-built-features`), merge to `main`, redeploy VEN UI
via the standard `deploy-node1` flow. No coordination needed with backend or other services.

## Open Questions

- Exact `data-testid` naming for the new card and its rows — resolved at implementation time following
  the `comfort-*` (`comfort-curve-card`, `comfort-row-{i}`, `comfort-add-btn`, `comfort-save-btn`,
  `comfort-reset-btn`) naming convention, e.g. `baseline-override-card`, `baseline-row-{i}`,
  `baseline-add-btn`, `baseline-save-btn`, `baseline-clear-btn`.
