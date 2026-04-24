# Plan C — Unified /user-requests API

## Goal

Make `POST /user-requests` the single scheduling surface for all asset types
(EV, heater, washing machine, any future shiftable load).
The direct session endpoints (`/ev-session`, `/heater-target`, `/shiftable-loads`)
become read-only diagnostic/state endpoints — not scheduling entry points.

---

## Background and Problem

The API evolved organically:

| Endpoint | Created when | Purpose today |
|---|---|---|
| `POST /ev-session` | Early phase | Direct EV session; no lifecycle |
| `POST /heater-target` | Early phase | Direct heater target; no lifecycle |
| `POST /shiftable-loads` | Phase B | Direct WM scheduling; no lifecycle |
| `POST /user-requests` | Later phase | EV+heater wrapper with lifecycle — but WM is missing |

`POST /user-requests` already handles EV and heater (routes/hems.rs:55–150):
- EV: creates `EvSession`, links it via `session_id` field on `UserRequest`
- Heater: creates `HeaterTarget`, links it the same way
- WM / other: falls through to a bare `UserRequest` with no linked session (lines 115–121)

So the gap is: WM was added as a direct endpoint but never wired into the user-request lifecycle.
EV has **two** redundant create paths. This caused a real bug (a session existed but was
invisible to `GET /user-requests` because it was created via `/ev-session`).

### Why this matters
- `GET /user-requests` is supposed to answer "what is the HEMS managing right now?" but it
  misses anything created via the direct endpoints.
- No unified cancellation — `DELETE /user-requests/:id` handles EV and heater cancellation but
  not shiftable loads.
- No budget/deadline/interruptible metadata for WM, even though those fields are already on
  `UserRequest`.

---

## Key Code References

### Backend — entities
- `UserRequest` struct: `VEN/src/entities/user_request.rs:30–50`
  - `id`, `asset_id`, `target_soc`, `target_energy_kwh`, `desired_power_kw`
  - `deadlines: Vec<RequestDeadline>`, `completion_policy`
  - `max_total_cost_eur`, `tier_count`
  - `session_id: Option<Uuid>` — links to EvSession, HeaterTarget, or ShiftableLoad
  - `status: UserRequestStatus`, `estimated_cost_eur`, `estimated_co2_g`
  - `interruptible`, `tolerance_min`, `budget_eur`
  - `created_at`, `updated_at`
- `UserRequestStatus` enum: `VEN/src/entities/user_request.rs:19–26`
  - variants: `Active`, `Completed`, `Cancelled`, `Failed`
  - serialised as `SCREAMING_SNAKE_CASE` (e.g. `"ACTIVE"`)
- `EvSession` struct: `VEN/src/entities/device_session.rs:11–22`
  - `id`, `target_soc`, `departure_time`, `opportunistic`, `created_at`, `updated_at`
- `HeaterTarget` struct: `VEN/src/entities/device_session.rs:30–38`
  - `id`, `target_temp_c`, `ready_by`, `created_at`, `updated_at`
- `ShiftableLoad` struct: `VEN/src/entities/device_session.rs:44–59`
  - `id`, `asset_id`, `power_kw`, `duration_min`, `earliest_start`, `latest_end`, `created_at`, `updated_at`

### Backend — user_request controller
- `CreateUserRequestBody`: `VEN/src/controller/user_request.rs:15–27`
  - `asset_id`, `target_soc`, `target_energy_kwh`, `desired_power_kw`
  - `deadlines`, `completion_policy`, `comfort_rates`, `budget_eur`
  - `interruptible`, `tolerance_min`
  - **Missing**: `power_kw`, `duration_min`, `earliest_start`, `latest_end` (WM-specific fields)
- `create_from_body`: `VEN/src/controller/user_request.rs:65–157`
  - Looks up asset in `sim.assets` by `asset_id` — **fails for WM** (WM has no sim profile entry).
  - WM requests must be detected and handled **before** calling this function.

### Backend — routes
- `POST /user-requests` handler: `VEN/src/routes/hems.rs:55–150`
  - EV branch (lines 68–91): detects `asset_id == "ev"`, creates EvSession, calls `state.set_ev_session()`
  - Heater branch (lines 92–114): detects `asset_id == "heater"` or `"boiler"`, creates HeaterTarget
  - Other/fallthrough (lines 115–121): stores bare UserRequest, no linked session
  - Detection is hardcoded string matching on `asset_id` — there is no profile API for this.
- `DELETE /user-requests/:id`: `VEN/src/routes/hems.rs:153–176`
  - Calls `state.cancel_request(id)` which clears linked `ev_session` or `heater_target`
  - Does NOT clear a linked `shiftable_load`

### Backend — InnerState (state.rs:91–127)
All HEMS fields live on `InnerState`, accessed through `AppState`'s async wrapper methods.
`AppState` itself (line 87) holds only `inner: Arc<RwLock<InnerState>>`.
- `active_requests: Vec<UserRequest>` — `#[serde(skip)]`
- `ev_session: Option<EvSession>` — `#[serde(skip)]`
- `heater_target: Option<HeaterTarget>` — `#[serde(skip)]`
- `shiftable_loads: Vec<ShiftableLoad>` — `#[serde(skip)]`
- `cancel_request()` at line 303: acquires write lock on `inner`, marks request Cancelled,
  clears linked ev_session or heater_target — does not yet handle ShiftableLoad

### Backend — route registration
- `VEN/src/routes/mod.rs:67–100`
  - User-requests: GET/POST `user-requests`, DELETE `user-requests/:id`
  - EV-session: GET/POST/DELETE `ev-session`
  - Heater-target: GET/POST/DELETE `heater-target`
  - Shiftable-loads: GET/POST `shiftable-loads`, DELETE `shiftable-loads/:id`

---

## Design Decisions

### Session type discriminator

`UserRequest.session_id` currently links to any session type but there is no field indicating
which type. Add `session_type` to make lookups unambiguous:

```rust
// entities/user_request.rs — add to UserRequest
#[serde(default)]
pub session_type: Option<SessionType>,

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionType { Ev, Heater, ShiftableLoad }
```

`#[serde(default)]` is required so that existing serialised state (which has no `session_type`
key) deserialises cleanly to `None`.

### WM fields in CreateUserRequestBody

WM-specific fields are already on `ShiftableLoad`. Add them as optional to the body:

```rust
// controller/user_request.rs — add to CreateUserRequestBody
pub power_kw: Option<f64>,                    // WM: fixed run power
pub duration_min: Option<u32>,                // WM: run duration
pub earliest_start: Option<DateTime<Utc>>,    // WM: window open (default: now)
pub latest_end: Option<DateTime<Utc>>,        // WM: window close (required for WM)
```

For WM, `earliest_start` + `latest_end` replace the multi-tier `deadlines` system used for EV.
The WM path validates that `latest_end` is present; if absent it returns 422.

### Asset type detection

The actual detection order in `post_requests` is:

1. **WM fast-path** (checked first, before `create_from_body`): `body.power_kw.is_some() && body.duration_min.is_some()` → shiftable load. WM has no simulator asset profile entry (`AssetProfile` has variants `Ev | Heater | Pv | Battery | BaseLoad` — no `ShiftableLoad`). `create_from_body` would reject it with `UnknownAsset`. WM must be handled in a separate early-exit path that does not call `create_from_body`.

2. **EV**: `asset_id == "ev"` — goes through `create_from_body` (EV is a sim asset).

3. **Heater**: `asset_id == "heater"` or `"boiler"` — goes through `create_from_body` (heater is a sim asset).

4. **Other**: unknown `asset_id` without WM fields → 422.

---

## Step-by-Step Implementation

### Step 1 — Add `SessionType` enum and `session_type` field

`VEN/src/entities/user_request.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionType { Ev, Heater, ShiftableLoad }

// Add to UserRequest struct:
#[serde(default)]
pub session_type: Option<SessionType>,
```

Also update the comment on `session_id` to include ShiftableLoad:
```rust
pub session_id: Option<Uuid>, // linked DeviceSession (EvSession, HeaterTarget, or ShiftableLoad)
```

### Step 2 — Add WM fields to `CreateUserRequestBody`

`VEN/src/controller/user_request.rs`:
```rust
pub power_kw: Option<f64>,
pub duration_min: Option<u32>,
pub earliest_start: Option<DateTime<Utc>>,
pub latest_end: Option<DateTime<Utc>>,
```

These fields are only read by the WM early-exit path in the handler; `create_from_body` ignores them.

### Step 3 — Add WM branch in POST handler

`VEN/src/routes/hems.rs` — restructure `post_requests` to handle WM before the `create_from_body`
call. WM has no sim asset entry, so it must bypass `create_from_body` entirely:

```rust
pub async fn post_requests(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateUserRequestBody>,
) -> impl IntoResponse {
    let now = Utc::now();

    // WM fast-path: must run before create_from_body; WM has no sim-asset profile entry
    // and create_from_body would return UnknownAsset for it.
    if body.power_kw.is_some() && body.duration_min.is_some() {
        let earliest = body.earliest_start.unwrap_or(now);
        let latest = match body.latest_end {
            Some(t) => t,
            None => return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "latest_end required for shiftable load"})),
            ).into_response(),
        };
        let load = ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            power_kw: body.power_kw.unwrap(),
            duration_min: body.duration_min.unwrap(),
            earliest_start: earliest,
            latest_end: latest,
            created_at: now,
            updated_at: now,
        };
        let user_req = UserRequest {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            target_soc: None,
            target_energy_kwh: (body.power_kw.unwrap() * body.duration_min.unwrap() as f64) / 60.0,
            desired_power_kw: body.power_kw.unwrap(),
            deadlines: vec![],
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: Some(load.id),
            session_type: Some(SessionType::ShiftableLoad),
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: body.budget_eur,
            created_at: now,
            updated_at: now,
        };
        ctx.state.add_shiftable_load(load).await;
        ctx.state.upsert_request(user_req.clone()).await;
        let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
        return (StatusCode::CREATED, Json(serde_json::to_value(user_req).unwrap_or_default()))
            .into_response();
    }

    // EV / heater path — requires sim-asset lookup
    let (assets, asset_configs) = {
        let sim = ctx.sim.lock().await;
        (sim.assets.clone(), sim.asset_configs.clone())
    };
    match crate::controller::user_request::create_from_body(body, &assets, &asset_configs, now) {
        Ok(mut user_req) => {
            if user_req.asset_id == "ev" {
                // ... existing EV branch — set session_type = Some(SessionType::Ev)
                user_req.session_type = Some(SessionType::Ev);
            } else if user_req.asset_id == "heater" || user_req.asset_id == "boiler" {
                // ... existing heater branch — set session_type = Some(SessionType::Heater)
                user_req.session_type = Some(SessionType::Heater);
            }
            // ... rest of existing Ok branch unchanged
        }
        Err(e) => { /* 422 */ }
    }
}
```

Also add `session_type` assignment to the existing EV and heater branches in the `Ok` arm.

### Step 4 — Update `cancel_request` in state.rs

Replace the body of `cancel_request` (state.rs:303–318) to handle ShiftableLoad. All field access
must go through the acquired write lock `inner` — the current implementation already uses this
pattern and the new match must follow the same pattern:

```rust
pub async fn cancel_request(&self, id: uuid::Uuid) -> bool {
    let mut inner = self.inner.write().await;
    if let Some(req) = inner.active_requests.iter_mut().find(|r| r.id == id) {
        req.status = UserRequestStatus::Cancelled;
        // Clone these out before the match: req borrows inner mutably and we
        // need inner.shiftable_loads below.
        let session_type = req.session_type.clone();
        let session_id = req.session_id;
        match session_type {
            Some(SessionType::Ev) => { inner.ev_session = None; }
            Some(SessionType::Heater) => { inner.heater_target = None; }
            Some(SessionType::ShiftableLoad) => {
                if let Some(sid) = session_id {
                    inner.shiftable_loads.retain(|l| l.id != sid);
                }
            }
            None => {}
        }
        true
    } else {
        false
    }
}
```

### Step 5 — Deprecate direct POST endpoints

In `routes/mod.rs`, keep `GET /ev-session`, `GET /heater-target`, `GET /shiftable-loads` as
read-only diagnostic endpoints. Remove the POST/DELETE handlers from the primary routes or
return `410 Gone` with a message pointing to `/user-requests`.

At minimum: add a note in the handler body:
```rust
// Deprecated: use POST /user-requests with asset_id = "ev"
// Kept for backward compatibility.
```

Do not remove them until all UI and BDD steps are migrated away.

### Step 6 — Update GET /user-requests response to include session details

Currently `GET /user-requests` returns `Vec<UserRequest>` which has `session_id` but not the
session's current fields (e.g., WM's `power_kw`, EV's `target_soc`). Extend the response type
with an embedded session:

```rust
// New response type:
#[derive(Serialize)]
pub struct UserRequestWithSession {
    #[serde(flatten)]
    pub request: UserRequest,
    pub session: Option<SessionDetail>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionDetail {
    Ev(EvSession),
    Heater(HeaterTarget),
    ShiftableLoad(ShiftableLoad),
}
```

Build this in the `get_requests` handler by joining each request with its session from state.

**TypeScript — fix `UserRequest` base type first.** The existing `UserRequest` in `types.ts`
has a `packet_id` field that does not exist on the backend, and is missing `session_id`,
`interruptible`, `tolerance_min`, `budget_eur`, `max_total_cost_eur`, and `tier_count`. Fix the
base type as part of this step before layering `UserRequestWithSession` on top:

```typescript
export type SessionType = "ev" | "heater" | "shiftable_load";

export type UserRequest = {
  id: string;
  asset_id: string;
  target_energy_kwh: number;
  target_soc: number | null;
  desired_power_kw: number;
  completion_policy: string;
  max_total_cost_eur: number | null;
  tier_count: number;
  deadlines: Array<{
    latest_end: string;
    max_total_cost_eur: number | null;
    min_completion: number;
  }>;
  session_id: string | null;        // replaces non-existent packet_id
  session_type?: SessionType | null; // optional: absent in responses predating Step 1
  status: UserRequestStatus;
  estimated_cost_eur: number;
  estimated_co2_g: number;
  interruptible: boolean;
  tolerance_min: number | null;
  budget_eur: number | null;
  created_at: string;
  updated_at: string;
};

export type SessionDetail =
  | ({ type: "ev" } & EvSession)
  | ({ type: "heater" } & HeaterTarget)
  | ({ type: "shiftable_load" } & ShiftableLoad);

export type UserRequestWithSession = UserRequest & {
  session: SessionDetail | null;
};
```

Search for `packet_id` in the UI codebase and remove any reads of that field before committing.

### Step 7 — Update UI Device Sessions page

`VEN/ui/src/pages/DeviceSessions.tsx` currently shows EV session, heater target, and shiftable
loads fetched from their separate endpoints. Migrate to use `GET /user-requests` as the primary
source (it now has embedded session details), keeping the direct GET endpoints only as fallback
or removing them from the UI entirely.

Update the "New Request" form to POST to `/user-requests` with asset-specific fields, instead
of the direct session endpoints.

The component currently imports `useEvSession`, `usePostEvSession`, `useDeleteEvSession`,
`useHeaterTarget`, `usePostHeaterTarget`, `useDeleteHeaterTarget`, `useShiftableLoads`,
`usePostShiftableLoad`, `useDeleteShiftableLoad` from `../api/hooks`. Step 7 replaces these
with `useRequests`, `usePostRequest`, `useDeleteRequest`. **The test file
`DeviceSessions.test.tsx` mocks all of the old hooks explicitly — it must be rewritten in the
same commit to mock the new hooks, otherwise all tests in that file break.**

---

## Files to Modify

| File | Change | Notes |
|---|---|---|
| `VEN/src/entities/user_request.rs` | Add `SessionType` enum + `session_type` field | `#[serde(default)]` required; update `session_id` comment |
| `VEN/src/controller/user_request.rs` | Add WM fields to `CreateUserRequestBody` | Fields unused by `create_from_body`; consumed by the WM early-exit path in the handler |
| `VEN/src/routes/hems.rs` | Add WM early-exit path before `create_from_body`; set `session_type` in EV/heater branches; extend `get_requests` to embed sessions; deprecate direct POSTs | WM must be handled before `create_from_body` — see Step 3 |
| `VEN/src/state.rs` | Replace `cancel_request` body to handle `ShiftableLoad` | All field mutations go through `inner`, not `self` — see Step 4 |
| `VEN/ui/src/api/types.ts` | Fix `UserRequest` base type (remove `packet_id`; add `session_id`, `interruptible`, `tolerance_min`, `budget_eur`, `max_total_cost_eur`, `tier_count`); add `SessionType`, `SessionDetail`, `UserRequestWithSession` | Fix base type first; search for `packet_id` usages before committing |
| `VEN/ui/src/api/client.ts` | Update `userRequests()` to return `UserRequestWithSession[]` | — |
| `VEN/ui/src/api/hooks.ts` | Verify `useRequests` type inference after client return type change; update `CreateUserRequestBody` import if WM fields added | No new hooks needed; existing `useRequests`/`usePostRequest`/`useDeleteRequest` cover all cases |
| `VEN/ui/src/pages/DeviceSessions.tsx` | Drive from `GET /user-requests`; update new-request form; remove old per-endpoint hook imports | — |
| `VEN/ui/src/__tests__/DeviceSessions.test.tsx` | Rewrite `vi.mock("../api/hooks", ...)` to mock `useRequests`, `usePostRequest`, `useDeleteRequest` instead of the nine per-endpoint hooks | Must be done in the same commit as Step 7; old mocks become stale the moment the component imports change |

**Do NOT modify**: `VEN/src/routes/mod.rs` — route registration is already complete and does not
change (direct POST endpoints are kept per the migration policy).

---

## Migration Path (avoid breaking BDD tests)

BDD tests currently use the direct endpoints (`POST /ev-session`, `POST /shiftable-loads`, etc.).
Do not remove those routes — only deprecate the POST variants. The tests will continue to work.

After Plan C is complete, a follow-up can migrate BDD steps to `POST /user-requests` and remove
the deprecated handlers. Mark that as a separate task.

---

## Acceptance Criteria

1. `POST /user-requests` with EV, heater, and WM fields all create the correct linked session and return the request with `session_type` set.
2. `GET /user-requests` returns all active scheduling requests including WM, each with embedded `session` detail.
3. `DELETE /user-requests/:id` cancels EV, heater, and WM requests correctly, cleaning up the linked session in all three cases.
4. `GET /ev-session`, `GET /heater-target`, `GET /shiftable-loads` still work as read endpoints.
5. All existing BDD tests pass (direct endpoints untouched).
6. The Device Sessions UI page shows all scheduled assets via the unified endpoint.

---

## Dependencies

- Plan B (shiftable load runtime) should be done first or in parallel — the runtime state
  (running/completed) is what makes `UserRequest.status` automatically transition to `Completed`
  when a WM finishes. Without Plan B, status must be updated manually or stays `Active` forever.
- Plan A is independent — can be done before or after.
