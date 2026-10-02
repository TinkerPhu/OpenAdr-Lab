# Proposal

## Why

**Depends on `ev-session-queue-foundation`**, which makes an EV hold an ordered
queue of non-overlapping charging sessions and gives the queue a single overlap
authority. That change deliberately leaves the human path alone: a user
submitting a session that overlaps one they already have still simply displaces
it, now by failing the checked insert rather than by silent overwrite.

That is the dangerous case. A user sets a standing weekly plan, stops thinking
about it, then weeks later adds a spontaneous trip whose charging window overlaps
it. One of the two deadlines cannot be met — not by the planner and not by the
user, because the car is physically away. Today nothing tells them: the newer
request wins and the older commitment quietly stops being achievable. The
failure is physically consequential (the car is not ready when they leave) and is
discovered at the worst possible moment.

Rejecting outright would be safe but hostile: the user would have to go find and
delete the older plan themselves, with the error telling them only that something
clashed. The decision that actually needs a human — *which of the two plans
wins* — can be asked directly.

## What Changes

- `POST /user-requests` for the EV **refuses** a session that conflicts with an
  existing queued session, and the refusal names the conflicting sessions
  (carried by the queue's existing conflict type) instead of reporting a generic
  validation failure.
- The same request accepts an explicit **replace instruction**: when the caller
  states which conflicting sessions it intends to displace, the server removes
  exactly those and inserts the new session as one atomic operation. Without
  that instruction nothing is removed.
- The VEN UI turns the refusal into a **prompt** naming the conflicting plan
  ("This conflicts with your charging plan for Tue 06:00 — replace it?") and
  resubmits with the replace instruction only on confirmation.
- The EV request form lets the user state when the car becomes available for the
  session (the window start the foundation change introduced), reusing the
  request body's existing `earliest_start` field rather than adding an
  EV-specific one.
- Where a submission conflicts with **more than one** existing session, the
  prompt lists all of them and replaces all or none.

## Capabilities

### New Capabilities
- `ev-session-conflict-resolution`: what happens when a user submits an EV
  charging session that overlaps one already queued — how the conflict is
  reported, what the user is offered, and what is removed when they confirm.

### Modified Capabilities
<!-- `openspec/specs/` is empty: this project folds implemented specs into
     `docs/` and deletes them (workflow rule 3). The queue behaviour this change
     builds on is specified in the `ev-session-queue` capability of the
     `ev-session-queue-foundation` change, which must land first. -->

## Impact

Code:
- `VEN/src/controller/user_request.rs` — `RequestError` gains the conflict
  variant; the replace instruction enters through `CreateUserRequestParams`.
- `VEN/src/services/user_request.rs` — `create_ev` carries the window start.
- `VEN/src/routes/hems/sessions.rs` — refusal status and body; atomic
  replace-then-insert.
- `VEN/ui/src/api/` and the EV request form / session components — conflict
  prompt, confirm-and-resubmit, window-start input.

APIs: `POST /user-requests` gains an optional replace instruction and a new
conflict error response; no existing successful request shape changes.

Dependencies: none added. Hard prerequisite: `ev-session-queue-foundation`.
