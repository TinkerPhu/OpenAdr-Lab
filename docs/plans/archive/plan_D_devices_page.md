# Plan D — Devices Page (unified scheduling UI)

## Goal

Replace the two separate nav items "Device Sessions" and "User Requests" with a single **Devices**
page.  The page shows three device cards (tiles) side by side — EV, Heater, Shiftable Loads —
each displaying the active scheduling request and the appropriate create/cancel controls.  A
collapsible "All Requests" section below provides the full request history.

All scheduling goes through `POST /user-requests` (Plan C, already implemented on the backend).
The old per-device endpoint hooks (`useEvSession`, `usePostEvSession`, etc.) are removed from the UI.

---

## Background

Plan C unified the backend: `POST /user-requests` now handles EV, heater, and shiftable loads.
`GET /user-requests` returns `UserRequestWithSession[]` — each request enriched with the embedded
session detail (`EvSession`, `HeaterTarget`, or `ShiftableLoad` under `.session`).

The UI has not been migrated. `DeviceSessions.tsx` still calls nine old hooks; `UserRequests.tsx`
shows a raw JSON editor. Two nav items exist for what should be one surface.

### Backend gaps to fix as part of this plan

Two fields in `POST /user-requests` are hardcoded in the handler and never read from the body:

| Asset | Field | Current | Expected |
|-------|-------|---------|----------|
| EV | `soft_deadline` | hardcoded `false` (`hems.rs:253`) | read from `body.soft_deadline` |
| Heater | `target_temp_c` | hardcoded `55.0` (`hems.rs:272`) | read from `body.target_temp_c` |

Both must be added to `CreateUserRequestBody` and wired in the handler.

---

## Target UI

```
┌─────────────────────────────────────────────────────────────────┐
│  Devices                                                        │
├─────────────────┬─────────────────┬─────────────────────────────┤
│ ⚡ EV Charging  │ 🔥 Water Heater │ ⏱ Shiftable Loads          │
│                 │                 │                             │
│  [idle state]   │  [idle state]   │  No loads scheduled         │
│  No session     │  No target set  │                             │
│  planned        │                 │  [Add Load]                 │
│                 │  [Set Target]   │                             │
│  [Plan Charging]│                 │                             │
│                 │                 │                             │
│  ─────────────  │                 │                             │
│  Automatic      │                 │                             │
│  surplus: ON ●  │                 │                             │
└─────────────────┴─────────────────┴─────────────────────────────┘

▼ All Requests (4 total)
  [collapsible table — see below]
```

**EV card — active state:**
```
│ ⚡ EV Charging  │
│                 │
│  ● ACTIVE       │
│  → 80% SoC      │
│  Depart 07:00   │
│  Soft deadline  │  ← chip, shown only when soft_deadline = true
│  Est. €1.23     │
│                 │
│  [Unplan]       │
│  ─────────────  │
│  Automatic      │
│  surplus: ON ●  │
```

**Heater card — active state:**
```
│ 🔥 Water Heater │
│                 │
│  ● ACTIVE       │
│  → 55°C         │
│  Ready by 09:00 │
│  Est. €0.34     │
│                 │
│  [Clear]        │
```

**Shiftable Loads card — with loads:**
```
│ ⏱ Shiftable Loads          │
│                             │
│  wm  2kW  60min  by 14:00 [×]│
│  dw  1.8kW 45min by 18:00[×]│
│                             │
│  [Add Load]                 │
```

**All Requests section (collapsed by default):**
```
▼ All Requests (4 total — 2 active, 1 done, 1 cancelled)
  Device    Summary                  Status     Cost     Created
  ──────────────────────────────────────────────────────────────
  ⚡ EV     80% SoC · depart 07:00   ● ACTIVE  €1.23   [×]
  🔥 Heater  55°C · ready 09:00       ● ACTIVE  €0.34   [×]
  ⏱ wm      2kW 60min · by 14:00      ✓ DONE    €0.12    –
  ⏱ wm      2kW 60min · by 10:00      ✕ CNCLD  €0.00    –
```

---

## Component Tree

```
DevicesPage                         pages/Devices.tsx  (replaces DeviceSessions.tsx)
├── card grid  (MUI Grid, 3 columns)
│   ├── EvCard                      components/devices/EvCard.tsx
│   │   ├── [active]  EvActiveView  (session summary + Unplan)
│   │   ├── [idle]    EvIdleView    (Plan Charging button)
│   │   ├── PlanEvDialog            (SoC slider + departure + soft-deadline)
│   │   └── SurplusToggle           (always shown in card footer)
│   ├── HeaterCard                  components/devices/HeaterCard.tsx
│   │   ├── [active]  HeaterActiveView
│   │   ├── [idle]    HeaterIdleView
│   │   └── SetHeaterDialog         (temperature + ready-by)
│   └── ShiftableLoadsCard          components/devices/ShiftableLoadsCard.tsx
│       ├── load list rows          (asset_id · power · duration · latest_end · [×])
│       ├── empty state
│       └── AddLoadDialog           (asset_id + power + duration + window)
└── AllRequestsSection              components/devices/AllRequestsSection.tsx
    └── collapsible MUI Accordion
        └── requests table          (all statuses, sorted created_at desc)
```

Sub-components live in `VEN/ui/src/components/devices/`.  The page itself stays thin —
data fetching only.

---

## Data Flow

All cards share **one** `useRequests()` call at page level.  Each card receives a pre-filtered
slice as props — no card fetches independently.

```
DevicesPage
  useRequests()        → allRequests: UserRequestWithSession[]
  useEvSettings()      → evSettings (surplus toggle)
  usePostRequest()     → shared create mutation
  useDeleteRequest()   → shared cancel mutation
  usePutEvSettings()   → surplus toggle mutation

  evRequest      = allRequests.find(r => r.session_type === "ev"             && r.status === "ACTIVE")
  heaterRequest  = allRequests.find(r => r.session_type === "heater"         && r.status === "ACTIVE")
  shiftableActive= allRequests.filter(r => r.session_type === "shiftable_load" && r.status === "ACTIVE")
```

Props per card:

- `EvCard`: `{ request, evSettings, postRequest, deleteRequest, putEvSettings, isPosting, isDeleting }`
- `HeaterCard`: `{ request, postRequest, deleteRequest, isPosting, isDeleting }`
- `ShiftableLoadsCard`: `{ loads, postRequest, deleteRequest, isPosting, isDeleting }`
- `AllRequestsSection`: `{ requests, deleteRequest, isDeleting }`

---

## Dialog Forms and POST Bodies

### Plan EV Charging dialog

Fields:
- SoC slider (20–100%, step 5, default 80)
- Departure datetime picker (default: now + 8h)
- Soft deadline toggle (default: off)

POST body to `/user-requests`:
```json
{
  "asset_id": "ev",
  "target_soc": 0.80,
  "target_energy_kwh": null,
  "desired_power_kw": 7.0,
  "soft_deadline": false,
  "completion_policy": "CONTINUE",
  "deadlines": [{
    "latest_end": "<departure_time ISO>",
    "max_total_cost_eur": null,
    "max_marginal_rate_eur_kwh": null,
    "min_completion": 0.8
  }],
  "comfort_rates": null
}
```

`desired_power_kw` is fixed at 7.0 (max AC charge rate).  `soft_deadline` is a new optional
field being added to `CreateUserRequestBody` (see backend steps).

### Set Heater Target dialog

Fields:
- Target temperature input (30–80°C, step 1, default 55)
- Ready by datetime picker (default: now + 4h)

POST body to `/user-requests`:
```json
{
  "asset_id": "heater",
  "target_soc": null,
  "target_energy_kwh": null,
  "desired_power_kw": null,
  "target_temp_c": 55.0,
  "completion_policy": "STOP",
  "deadlines": [{
    "latest_end": "<ready_by ISO>",
    "max_total_cost_eur": null,
    "max_marginal_rate_eur_kwh": null,
    "min_completion": 1.0
  }],
  "comfort_rates": null
}
```

`target_temp_c` is a new optional field being added to `CreateUserRequestBody`.

### Add Shiftable Load dialog

Fields:
- Asset ID text input (default: "wm")
- Power (kW) number input (default 2.0, min 0.1)
- Duration (min) number input (default 60, min 1)
- Earliest start datetime (default: now)
- Latest end datetime (default: now + 4h)

POST body to `/user-requests`:
```json
{
  "asset_id": "wm",
  "target_soc": null,
  "target_energy_kwh": null,
  "desired_power_kw": null,
  "power_kw": 2.0,
  "duration_min": 60,
  "earliest_start": "<ISO>",
  "latest_end": "<ISO>",
  "completion_policy": null,
  "deadlines": [],
  "comfort_rates": null
}
```

---

## Label Fix

`"Opportunistic PV Charging"` is misleading — the feature activates on PV surplus *and* low
tariffs, not PV only.  Change the display label:

| Location | Old | New |
|----------|-----|-----|
| `EvCard` toggle label | `"Opportunistic PV Charging"` | `"Automatic surplus charging"` |
| paused chip | `"Paused — active plan"` | keep as-is |

`data-testid="ev-opportunistic-charging-switch"` stays unchanged (test compatibility).

---

## Step-by-Step Implementation

### Step 1 — Backend: add `soft_deadline` and `target_temp_c` to `CreateUserRequestBody`

**File: `VEN/src/controller/user_request.rs`**

Add two optional fields to the struct (after the existing shiftable-load fields):
```rust
pub soft_deadline: Option<bool>,   // EV: passed through to EvSession
pub target_temp_c: Option<f64>,    // Heater: target temperature
```

**File: `VEN/src/routes/hems.rs`**

EV branch (~line 253) — replace hardcoded `soft_deadline: false`:
```rust
soft_deadline: body.soft_deadline.unwrap_or(false),
```

Heater branch (~line 272) — replace hardcoded `target_temp_c: 55.0_f64`:
```rust
target_temp_c: body.target_temp_c.unwrap_or(55.0),
```

### Step 2 — UI types: extend `CreateUserRequestBody`

**File: `VEN/ui/src/api/types.ts`**

Add to `CreateUserRequestBody`:
```typescript
soft_deadline?: boolean;   // EV: soft deadline
target_temp_c?: number;    // Heater: target temperature
```

No other type changes needed — `UserRequestWithSession`, `SessionDetail`, and `EvSession`
(which already has `soft_deadline`) are already correct from Plan C.

### Step 3 — Create `components/devices/` sub-components

Create directory `VEN/ui/src/components/devices/`.

#### `EvCard.tsx`

State: `dialogOpen`, `targetSoc` (default 80), `departure` (default now+8h),
`softDeadline` (default false), `error`.

Render:
- MUI `Card` with full height, `data-testid="ev-card"`
- `CardHeader` title "EV Charging"
- `CardContent`:
  - **Active** (`request` defined): `data-testid="ev-active-view"`
    - Green `Chip` label "ACTIVE" `data-testid="ev-status-chip"`
    - `→ {(session.target_soc * 100).toFixed(0)}% SoC` `data-testid="ev-target-soc"`
    - `Depart: {formatted departure_time}` `data-testid="ev-departure"`
    - If `session.soft_deadline`: small chip "Soft deadline" `data-testid="ev-soft-deadline-chip"`
    - `Est. €{estimated_cost_eur.toFixed(2)}` `data-testid="ev-estimated-cost"`
    - `[Unplan]` button `data-testid="ev-unplan-btn"` → calls `deleteRequest(request.id)`
  - **Idle** (`request` undefined): `data-testid="ev-idle-view"`
    - "No session planned" text
    - `[Plan Charging]` button `data-testid="ev-plan-btn"` → opens dialog
- `Divider`
- `CardActions` (surplus toggle, always shown):
  - `data-testid="ev-settings-section"`
  - `FormControlLabel` with `Switch` `data-testid="ev-opportunistic-charging-switch"`
  - Label: "Automatic surplus charging"
  - Disabled when `evSettings?.paused_by_active_session`
  - If paused: chip "Paused — active plan" `data-testid="ev-opportunistic-paused-chip"`

`PlanEvDialog` (inside `EvCard.tsx`):
- `data-testid="ev-dialog"`
- SoC `Slider` `data-testid="ev-soc-slider"`
- Departure `TextField` type="datetime-local" `data-testid="ev-departure-input"`
- Soft deadline `Switch` `data-testid="ev-soft-deadline-switch"`
- Cancel `data-testid="ev-dialog-cancel"`, Confirm `data-testid="ev-dialog-confirm"`
- On confirm: calls `postRequest` with EV body (see Dialog Forms section)

#### `HeaterCard.tsx`

State: `dialogOpen`, `tempC` (default "55"), `readyBy` (default now+4h), `error`.

Render:
- MUI `Card`, `data-testid="heater-card"`
- `CardHeader` title "Water Heater"
- `CardContent`:
  - **Active**: `data-testid="heater-active-view"`
    - Status chip `data-testid="heater-status-chip"`
    - `→ {session.target_temp_c}°C` `data-testid="heater-temp"`
    - `Ready by: {formatted ready_by}` `data-testid="heater-ready-by"`
    - `Est. €{estimated_cost_eur.toFixed(2)}` `data-testid="heater-estimated-cost"`
    - `[Clear]` button `data-testid="heater-clear-btn"` → calls `deleteRequest(request.id)`
  - **Idle**: `data-testid="heater-idle-view"`
    - "No target set"
    - `[Set Target]` button `data-testid="heater-set-btn"`

`SetHeaterDialog`:
- `data-testid="heater-dialog"`
- Temperature `TextField` `data-testid="heater-temp-input"`
- Ready by `TextField` type="datetime-local" `data-testid="heater-readyby-input"`
- Cancel `data-testid="heater-dialog-cancel"`, Confirm `data-testid="heater-dialog-confirm"`
- On confirm: calls `postRequest` with heater body (see Dialog Forms section)

#### `ShiftableLoadsCard.tsx`

Render:
- MUI `Card`, `data-testid="shiftable-card"`
- `CardHeader` title "Shiftable Loads"
- `CardContent`:
  - If `loads.length === 0`: "No loads scheduled" `data-testid="shiftable-empty"`
  - Else: list rows, each `data-testid={`shiftable-row-${req.id}`}`:
    - `{session.asset_id} · {session.power_kw}kW · {session.duration_min}min · by {fmt(session.latest_end)}`
    - `[×]` icon button `data-testid={`shiftable-cancel-${req.id}`}` → `deleteRequest(req.id)`
- `CardActions`:
  - `[Add Load]` button `data-testid="shiftable-add-btn"`

`AddLoadDialog`:
- `data-testid="shiftable-dialog"`
- Asset ID `data-testid="shiftable-asset-input"`
- Power `data-testid="shiftable-power-input"`
- Duration `data-testid="shiftable-duration-input"`
- Earliest start `data-testid="shiftable-earliest-input"`
- Latest end `data-testid="shiftable-latest-input"`
- Cancel `data-testid="shiftable-dialog-cancel"`, Confirm `data-testid="shiftable-dialog-confirm"`

Session data for display: `(req.session as ShiftableLoad & { type: "shiftable_load" })`.

#### `AllRequestsSection.tsx`

Render:
- MUI `Accordion` collapsed by default, `data-testid="all-requests-accordion"`
- Summary text: `All Requests ({requests.length} total)`
- Details: MUI `Table` `data-testid="all-requests-table"`
  - Columns: Device | Summary | Status | Est. Cost € | Created | (cancel)
  - Rows sorted: ACTIVE first, then by `created_at` descending
  - Each row `data-testid={`all-requests-row-${req.id}`}`
  - Summary column: human-readable from session (see `sessionSummary` below)
  - `[×]` cancel button — enabled only for ACTIVE, `data-testid={`all-requests-cancel-${req.id}`}`

```typescript
function sessionSummary(req: UserRequestWithSession): string {
  const s = req.session;
  if (!s) return req.asset_id;
  if (s.type === "ev")           return `${(s.target_soc * 100).toFixed(0)}% SoC · depart ${fmt(s.departure_time)}`;
  if (s.type === "heater")       return `${s.target_temp_c}°C · ready ${fmt(s.ready_by)}`;
  if (s.type === "shiftable_load") return `${s.power_kw}kW ${s.duration_min}min · by ${fmt(s.latest_end)}`;
  return req.asset_id;
}
```

### Step 4 — Create `pages/Devices.tsx`

Thin page: fetches, filters, passes slices to cards.

```typescript
export function DevicesPage() {
  const { data: allRequests = [], isLoading, isError, error } = useRequests();
  const { data: evSettings } = useEvSettings();
  const postMut    = usePostRequest();
  const deleteMut  = useDeleteRequest();
  const putEvMut   = usePutEvSettings();

  const evRequest       = allRequests.find(r => r.session_type === "ev"              && r.status === "ACTIVE");
  const heaterRequest   = allRequests.find(r => r.session_type === "heater"          && r.status === "ACTIVE");
  const shiftableActive = allRequests.filter(r => r.session_type === "shiftable_load" && r.status === "ACTIVE");

  return (
    <Box data-testid="devices-page">
      <Typography variant="h5" gutterBottom>Devices</Typography>
      {isLoading && <CircularProgress />}
      {isError   && <Alert severity="error">{String(error)}</Alert>}
      <Grid container spacing={2} sx={{ mb: 3 }}>
        <Grid item xs={12} md={4}>
          <EvCard request={evRequest} evSettings={evSettings}
            postRequest={postMut.mutateAsync} deleteRequest={deleteMut.mutateAsync}
            putEvSettings={putEvMut.mutate}
            isPosting={postMut.isPending} isDeleting={deleteMut.isPending} />
        </Grid>
        <Grid item xs={12} md={4}>
          <HeaterCard request={heaterRequest}
            postRequest={postMut.mutateAsync} deleteRequest={deleteMut.mutateAsync}
            isPosting={postMut.isPending} isDeleting={deleteMut.isPending} />
        </Grid>
        <Grid item xs={12} md={4}>
          <ShiftableLoadsCard loads={shiftableActive}
            postRequest={postMut.mutateAsync} deleteRequest={deleteMut.mutateAsync}
            isPosting={postMut.isPending} isDeleting={deleteMut.isPending} />
        </Grid>
      </Grid>
      <AllRequestsSection requests={allRequests}
        deleteRequest={deleteMut.mutateAsync} isDeleting={deleteMut.isPending} />
    </Box>
  );
}
```

### Step 5 — Update `App.tsx`

- Remove imports for `DeviceSessionsPage` and `UserRequestsPage`
- Add `import { DevicesPage } from "./pages/Devices"`
- Replace the two nav buttons with one:
  ```tsx
  <Button component={Link} to="/devices" data-testid="nav-devices">Devices</Button>
  ```
- Replace the two routes with one:
  ```tsx
  <Route path="/devices" element={<DevicesPage />} />
  ```

### Step 6 — Write `__tests__/Devices.test.tsx`

Replaces `DeviceSessions.test.tsx`.  Five mocks replace the old nine:

```typescript
vi.mock("../api/hooks", () => ({
  useRequests:      () => ({ data: mockRequests(),   isLoading: false, isError: false }),
  useEvSettings:    () => ({ data: mockEvSettings(), isLoading: false }),
  usePostRequest:   () => ({ mutateAsync: mockPostRequest,   isPending: false }),
  useDeleteRequest: () => ({ mutateAsync: mockDeleteRequest, isPending: false }),
  usePutEvSettings: () => ({ mutate: mockPutEvSettings,      isPending: false }),
}));
```

Test scenarios:

| # | Scenario | Assert |
|---|----------|--------|
| 1 | All idle | `ev-idle-view`, `ev-plan-btn`, `heater-idle-view`, `heater-set-btn`, `shiftable-empty` |
| 2 | EV active request in data | `ev-active-view`, `ev-target-soc` = "80%", `ev-unplan-btn` |
| 3 | Click Plan Charging | `ev-dialog` appears |
| 4 | Confirm EV dialog | `mockPostRequest` called with `asset_id:"ev"`, `target_soc`, `deadlines` |
| 5 | Click Unplan | `mockDeleteRequest` called with the EV request id |
| 6 | Heater active request | `heater-active-view`, `heater-temp` = "55°C" |
| 7 | Click Set Target | `heater-dialog` appears |
| 8 | Confirm heater dialog | `mockPostRequest` called with `asset_id:"heater"`, `target_temp_c`, `deadlines` |
| 9 | Click Clear | `mockDeleteRequest` called with heater request id |
| 10 | Shiftable load in data | `shiftable-row-{id}` with correct asset/power/duration |
| 11 | Click Add Load | `shiftable-dialog` appears |
| 12 | Confirm Add Load | `mockPostRequest` called with `power_kw`, `duration_min`, `latest_end` |
| 13 | Click `[×]` on load | `mockDeleteRequest` called with load request id |
| 14 | Surplus toggle rendered | switch present and checked |
| 15 | Toggle surplus | `mockPutEvSettings` called with `{ opportunistic_charging_enabled: false }` |
| 16 | Paused state | chip shown, switch disabled when `paused_by_active_session: true` |
| 17 | All Requests expand | accordion opens, table visible with all rows |
| 18 | Cancel in All Requests | `mockDeleteRequest` called; disabled for non-ACTIVE rows |

### Step 7 — Delete old files

```
VEN/ui/src/pages/DeviceSessions.tsx          (replaced by Devices.tsx)
VEN/ui/src/pages/UserRequests.tsx            (AllRequestsSection covers its surface)
VEN/ui/src/__tests__/DeviceSessions.test.tsx (replaced by Devices.test.tsx)
VEN/ui/src/__tests__/UserRequests.test.tsx   (no longer a standalone page)
```

---

## Files to Create / Modify / Delete

| File | Action | Notes |
|------|--------|-------|
| `VEN/src/controller/user_request.rs` | Modify | Add `soft_deadline: Option<bool>`, `target_temp_c: Option<f64>` |
| `VEN/src/routes/hems.rs` | Modify | Wire `body.soft_deadline` in EV branch, `body.target_temp_c` in heater branch |
| `VEN/ui/src/api/types.ts` | Modify | Add `soft_deadline?`, `target_temp_c?` to `CreateUserRequestBody` |
| `VEN/ui/src/components/devices/EvCard.tsx` | Create | — |
| `VEN/ui/src/components/devices/HeaterCard.tsx` | Create | — |
| `VEN/ui/src/components/devices/ShiftableLoadsCard.tsx` | Create | — |
| `VEN/ui/src/components/devices/AllRequestsSection.tsx` | Create | — |
| `VEN/ui/src/pages/Devices.tsx` | Create | — |
| `VEN/ui/src/App.tsx` | Modify | Route + nav |
| `VEN/ui/src/__tests__/Devices.test.tsx` | Create | — |
| `VEN/ui/src/pages/DeviceSessions.tsx` | Delete | — |
| `VEN/ui/src/pages/UserRequests.tsx` | Delete | — |
| `VEN/ui/src/__tests__/DeviceSessions.test.tsx` | Delete | — |
| `VEN/ui/src/__tests__/UserRequests.test.tsx` | Delete | — |

---

## Acceptance Criteria

1. `POST /user-requests` for EV uses `soft_deadline` from the request body (not hardcoded false).
2. `POST /user-requests` for heater uses `target_temp_c` from the request body (not hardcoded 55.0).
3. Devices page shows three cards as tiles side by side (md+ screens), stacked on mobile.
4. EV idle: "No session planned" + `[Plan Charging]` button visible.
5. EV active: SoC %, departure, estimated cost, `[Unplan]` button visible.
6. Plan Charging dialog: SoC slider + departure picker + soft-deadline toggle; submits to `/user-requests`.
7. Heater idle: "No target set" + `[Set Target]` button visible.
8. Heater active: temperature, ready-by, estimated cost, `[Clear]` button visible.
9. Set Heater dialog: temperature + ready-by inputs; submits to `/user-requests` with `target_temp_c`.
10. Shiftable Loads: active loads listed with asset/power/duration/window + `[×]` per row.
11. Add Load dialog submits to `/user-requests` with `power_kw`, `duration_min`, `latest_end`.
12. `[Unplan]` / `[Clear]` / `[×]` all call `DELETE /user-requests/:id`.
13. Surplus toggle label reads "Automatic surplus charging".
14. Surplus toggle disabled + paused chip shown when `paused_by_active_session = true`.
15. All Requests accordion is collapsed by default; expands to the full history table.
16. Cancel `[×]` in the history table enabled for ACTIVE rows, disabled for others.
17. Nav shows one "Devices" item; "Device Sessions" and "User Requests" items are gone.
18. All 18 `Devices.test.tsx` scenarios pass.
19. Existing BDD tests pass (direct backend endpoints are not removed).

---

## Dependencies

- **Plan C backend**: complete — the only backend work here is the two field fixes in Step 1.
- **Plan B** (shiftable load runtime): independent — without it, shiftable requests stay ACTIVE
  until cancelled manually. Display still works correctly.
