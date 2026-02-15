# Plan: Active Event Filter + UC1 Improvements + Delete Error Handling

## Context

Events in OpenADR 3 are permanent contractual records — they can't be deleted once reports reference them (FK constraint). The system needs a way to filter out past events so VENs and UIs don't drown in historical data. Additionally, UC1 (Emergency Load Shed) has no documented way to end the emergency, and the VTN UI silently swallows delete errors.

## Changes

### 1. VTN (openleadr-rs): Add `?active=true` filter — application-level

Add an optional boolean query param to the events endpoint. Filtering happens **in Rust after fetching from DB** (not in SQL). DB-level optimization (computed column, index) deferred to later.

**Files:**
- `openleadr-rs/openleadr-vtn/src/api/event.rs` — add `active: Option<bool>` to `QueryParams`
- `openleadr-rs/openleadr-vtn/src/data_source/postgres/event.rs` — post-filter in `retrieve_all`: after fetching events, remove those where `interval_period.start + duration < now`. Events with no `interval_period` or no `duration` are always considered active.
- No migration, no SQLx cache change (SQL query unchanged)

**Filter logic (Rust):**
```
if active == Some(true):
  keep event if:
    - interval_period is None (no timing = always active), OR
    - interval_period.start is None, OR
    - interval_period.duration is None (open-ended = always active), OR
    - start + duration > now (not yet ended)
if active == Some(false):
  keep only past events (complement of above)
if active is None:
  return all (backward compatible)
```

### 2. BFF: Pass `active` query param through

**File:** `VTN/bff/src/routes/events.rs`
- Accept optional `?active=true` query param from UI
- Forward to VTN: `/events?active=true`

### 3. VTN UI: Delete error message

**Files:**
- `VTN/ui/src/pages/Events.tsx` — add `deleteError` state, `onError` handler on `deleteMut`, pass error to `ConfirmDialog`
- `VTN/ui/src/components/ConfirmDialog.tsx` — add optional `error` prop, show MUI `Alert` when set

When delete fails (HTTP 502 from BFF wrapping VTN's FK constraint error), show:
> "Cannot delete this event — reports or other records reference it. To end an active event, edit it and set a start time and duration instead."

### 4. UC1 Manual: "Ending the Emergency" section

**File:** `docs/USE-CASE-MANUAL.md`

After UC1 "What to Observe", add steps for ending the emergency by editing the event to add `intervalPeriod` (start time + duration). This is the correct OpenADR 3 pattern — the event becomes "completed" without being deleted.

### 5. Remove "Cleanup After a Use Case" section

**File:** `docs/USE-CASE-MANUAL.md`

Replace the cleanup section with a brief "Event Lifecycle" note explaining that events are permanent records, and the way to "close" an event is to add timing via edit. Mention that deletion is possible (reports first, then events, then programs) but discouraged.

Remove the cleanup pointer from UC1.

### 6. Wish list update

**File:** `docs/WHISH_LIST.md` — add the `?active=true` filter item, note DB optimization as future work.

## Implementation Order

1. Documentation changes (UC1 manual, cleanup section, wish list)
2. VTN UI delete error handling (frontend-only, no rebuild needed on Pi4)
3. VTN Rust: add `active` param + post-filter (requires Pi4 rebuild ~25 min)
4. BFF: pass through `active` param (requires Pi4 rebuild ~2 min)
5. Deploy and test

## Verification

- Create an event without timing → verify it shows with `?active=true`
- Edit it to add past start+duration → verify it disappears with `?active=true`
- Try to delete an event with reports → verify user-friendly error message appears
- Run existing behave test suite to check for regressions
- Walk through UC1 manual steps including "Ending the Emergency"

## Deferred

- **DB-level optimization**: Add `ends_at timestamptz` computed column + index for SQL-level filtering. Not needed until event tables grow large.
- **VEN polling with active filter**: Change VEN's `/events` poll to `/events?active=true` to reduce traffic.
- **Upstream PR**: Once proven, submit to OpenLEADR/openleadr-rs.
