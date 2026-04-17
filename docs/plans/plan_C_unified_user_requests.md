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

`POST /user-requests` already handles EV and heater (routes/hems.rs:54–139):
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
- `UserRequestStatus` enum: `VEN/src/entities/user_request.rs:21–26`
  - variants: `Active`, `Completed`, `Cancelled`, `Failed`
- `EvSession` struct: `VEN/src/entities/device_session.rs:11–22`
  - `id`, `target_soc`, `departure_time`, `opportunistic`, `created_at`, `updated_at`
- `HeaterTarget` struct: `VEN/src/entities/device_session.rs:30–38`
  - `id`, `target_temp_c`, `ready_by`, `created_at`, `updated_at`
- `ShiftableLoad` struct: `VEN/src/entities/device_session.rs:45–59`
  - `id`, `asset_id`, `power_kw`, `duration_min`, `earliest_start`, `latest_end`

### Backend — user_request controller
- `CreateUserRequestBody`: `VEN/src/controller/user_request.rs:15–27`
  - `asset_id`, `target_soc`, `target_energy_kwh`, `desired_power_kw`
  - `deadlines`, `completion_policy`, `comfort_rates`, `budget_eur`
  - `interruptible`, `tolerance_min`
  - **Missing**: `power_kw`, `duration_min`, `earliest_start`, `latest_end` (WM-specific fields)

### Backend — routes
- `POST /user-requests` handler: `VEN/src/routes/hems.rs:54–139`
  - EV branch (lines 68–91): creates EvSession, calls `state.set_ev_session()`
  - Heater branch (lines 92–114): creates HeaterTarget, calls `state.set_heater_target()`
  - Other/fallthrough (lines 115–121): stores bare UserRequest, no linked session
- `DELETE /user-requests/:id`: `VEN/src/routes/hems.rs:152–176`
  - Calls `state.cancel_request(id)` which clears linked `ev_session` or `heater_target`
  - Does NOT clear a linked `shiftable_load`

### Backend — AppState (state.rs:87–127)
- `active_requests: Vec<UserRequest>` — `#[serde(skip)]`
- `ev_session: Option<EvSession>` — `#[serde(skip)]`
- `heater_target: Option<HeaterTarget>` — `#[serde(skip)]`
- `shiftable_loads: Vec<ShiftableLoad>` — `#[serde(skip)]`
- `cancel_request()` at line ~303: removes request, clears linked session — currently handles EV/heater only

### Backend — route registration
- `VEN/src/routes/mod.rs:66–96`
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
pub session_type: Option<SessionType>,

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionType { Ev, Heater, ShiftableLoad }
```

### WM fields in CreateUserRequestBody

WM-specific fields are already on `ShiftableLoad`. Add them as optional to the body:

```rust
// controller/user_request.rs — add to CreateUserRequestBody
pub power_kw: Option<f64>,           // WM: fixed run power
pub duration_min: Option<u32>,       // WM: run duration
pub earliest_start: Option<DateTime<Utc>>,  // WM: window open
pub latest_end: Option<DateTime<Utc>>,      // WM / heater deadline (replaces deadlines[] for simple cases)
```

For WM, `earliest_start` + `latest_end` replace the multi-tier `deadlines` system used for EV
(which is more complex — EV has ASAP vs BY_DEADLINE tiers). Keep both; for WM, if `deadlines`
is empty and `earliest_start`/`latest_end` are provided, derive a single deadline from them.

### Asset type detection

The `POST /user-requests` handler currently detects EV vs heater by matching `asset_id` against
the profile. Extend this logic:

```rust
// In the POST handler:
let asset_profile = profile.asset_by_id(&body.asset_id);
match asset_profile {
    Some(AssetProfile::Ev(_))      => { /* existing EV branch */ }
    Some(AssetProfile::Heater(_))  => { /* existing heater branch */ }
    Some(AssetProfile::Battery(_)) => { /* future */ }
    None | Some(_)                 => {
        // Unknown asset — treat as shiftable load if power_kw + duration_min present
        if body.power_kw.is_some() && body.duration_min.is_some() {
            /* new WM branch */
        } else {
            return (StatusCode::BAD_REQUEST, "unknown asset type").into_response();
        }
    }
}
```

For WM detection: either check the profile for `AssetProfile::BaseLoad` (WM is not base load)
or simply accept any `asset_id` with WM fields — the profile is optional for shiftable loads
since WM doesn't need a sim profile.

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

### Step 2 — Add WM fields to `CreateUserRequestBody`

`VEN/src/controller/user_request.rs`:
```rust
pub power_kw: Option<f64>,
pub duration_min: Option<u32>,
pub earliest_start: Option<DateTime<Utc>>,
pub latest_end: Option<DateTime<Utc>>,
```

### Step 3 — Add WM branch in POST handler

`VEN/src/routes/hems.rs`, in the `post_requests` handler after the heater branch (around line 115):

```rust
// WM / shiftable load branch
} else if body.power_kw.is_some() && body.duration_min.is_some() {
    let earliest = body.earliest_start.unwrap_or(now);
    let latest = body.latest_end
        .ok_or_else(|| (StatusCode::UNPROCESSABLE_ENTITY, "latest_end required for shiftable load"))?;
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
    request.session_id = Some(load.id);
    request.session_type = Some(SessionType::ShiftableLoad);
    ctx.state.add_shiftable_load(load).await;
```

### Step 4 — Update `cancel_request` in state.rs

Current `cancel_request()` (line ~303) clears `ev_session` or `heater_target` based on which is
linked. Extend:

```rust
match request.session_type {
    Some(SessionType::Ev)           => { self.ev_session = None; }
    Some(SessionType::Heater)       => { self.heater_target = None; }
    Some(SessionType::ShiftableLoad) => {
        if let Some(sid) = request.session_id {
            self.shiftable_loads.retain(|l| l.id != sid);
        }
    }
    None => {}
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

### Step 7 — Update UI Device Sessions page

`VEN/ui/src/pages/DeviceSessions.tsx` currently shows EV session, heater target, and shiftable
loads fetched from their separate endpoints. Migrate to use `GET /user-requests` as the primary
source (it now has embedded session details), keeping the direct GET endpoints only as fallback
or removing them from the UI entirely.

Update the "New Request" form to POST to `/user-requests` with asset-specific fields, instead
of the direct session endpoints.

---

## Files to Modify

| File | Change |
|---|---|
| `VEN/src/entities/user_request.rs` | Add `SessionType` enum + `session_type` field |
| `VEN/src/controller/user_request.rs` | Add WM fields to `CreateUserRequestBody` |
| `VEN/src/routes/hems.rs` | Add WM branch; update `get_requests` to embed sessions; deprecate direct POSTs |
| `VEN/src/state.rs` | Extend `cancel_request` for `ShiftableLoad` session type |
| `VEN/ui/src/api/types.ts` | Add `UserRequestWithSession`, `SessionType`, `SessionDetail` |
| `VEN/ui/src/api/client.ts` | Update `userRequests()` to return `UserRequestWithSession[]` |
| `VEN/ui/src/pages/DeviceSessions.tsx` | Drive from `GET /user-requests`; update new-request form |

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
