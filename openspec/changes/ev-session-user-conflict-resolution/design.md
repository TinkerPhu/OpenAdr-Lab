# Design

## Context

See `proposal.md` — Why. This change is the second half of a pair:
`ev-session-queue-foundation` builds the queue, its single overlap authority
(`EvSessionQueue::conflicts` / `insert`), its conflict type (`EvSessionConflict`),
**the refusal itself**, and the `earliest_start` → `window_start` mapping that lets a
user hold more than one session. It must land first. What remains here is the offer:
naming the clash back to the user and acting on one confirmation.
Nothing here re-decides what an overlap is, nor re-derives which sessions clash —
both are read from the foundation's `insert` result.

**Where the concept lives today** (the `one-concept-one-function` inventory):

| Concept | Where it lives today | What happens to it |
| --- | --- | --- |
| Overlap detection | `EvSessionQueue::conflicts` (foundation change) | **Reused unchanged** — never re-implemented here |
| Checked insertion | `EvSessionQueue::insert -> Result<(), EvSessionConflict>` (foundation change) | **Reused unchanged** |
| Request validation | `controller/user_request.rs::create_from_body`, `RequestError` — `VEN/src/controller/user_request.rs:62-107` | Gains the conflict variant |
| EV session construction | `services/user_request.rs::create_ev:19-57` | Carries the window start |
| "Earliest this may start" on a request | `CreateUserRequestParams.earliest_start` — `controller/user_request.rs:35` (today used by shiftable loads) | **Reused** as the EV window start |
| Request submission route | `routes/hems/sessions.rs:355-358` | Refusal status/body; atomic replace |
| Request submission UI | `VEN/ui/src/components/` request form; `api/` client | Conflict prompt + confirm-and-resubmit |

**Constraint that shapes the approach**: the user's own framing of the problem is
that a *rejection* which only says "something clashed" costs them a search. So the
refusal is not the product — the refusal *plus the named conflict* is, and the UI
owes them a one-click resolution. The server, however, must still never remove a
standing commitment without being told to, because the whole failure being fixed
is a commitment disappearing unnoticed.

## Goals / Non-Goals

**Goals:**

- No session is ever removed without an explicit instruction naming it.
- The user resolves the clash in one confirmation, never by hunting for the old
  plan.
- The conflict reaches the UI with enough detail to describe the clashing plan
  (its departure, its target), so the prompt is specific rather than generic.
- One round-trip shape: the same endpoint, with an optional instruction — not a
  second "force" endpoint.

**Non-Goals:**

- No merging of overlapping sessions into one. The user's two plans express two
  different departures; silently fusing them invents a third intent neither asked
  for.
- No partial displacement: a submission either clears all its conflicts or none.
- No automatic resolution heuristic ("the later plan probably wins"). The decision
  is the user's; that is the entire point.
- No change to the simulated-usage producer — it keeps the foundation's
  precedence behaviour (skip on conflict).

## Decisions

### Decision 1 — The replace instruction names session ids, and is validated against the actual conflict set

`CreateUserRequestParams` gains
`replace_session_ids: Option<Vec<Uuid>>`. The route resolves the conflict set from
the queue first, then accepts the submission only when the instruction names
**exactly** that set. Naming fewer is refused (spec: "A partial replace instruction
is refused"); naming a session that is not in the conflict set is refused; naming
one that is no longer queued is refused.

*Why ids and not a boolean `force`*: a boolean authorises the removal of whatever
happens to clash *at the moment the server processes it*, which is not what the
user saw and confirmed. The queue can have changed between the refusal and the
confirmation — a simulated session inserted, another tab's submission landing, the
old plan expiring. Naming the ids makes the confirmation refer to the exact plans
the user was shown; anything else is refused and re-prompted with the current
truth. This is the same reasoning as an `If-Match` precondition.

*Alternative considered*: `force: true`. Rejected for the reason above — it
reintroduces "a commitment disappears unnoticed", just one round-trip later.

### Decision 2 — The refusal carries the clashing sessions, not just their ids

The foundation's `EvSessionConflict` carries ids. The UI needs more than an id — it
has to write "your plan for Tue 06:00" — so the route resolves those ids to their
sessions **within the same queue read that detected the conflict**, and the
response body carries each clashing session's id, charging window and target.

Resolving inside that one read is the point: having the UI fetch the queue
separately to look the ids up would make the prompt a second, independently
derived account of what the user is being asked about, which can disagree with the
refusal it is explaining. The route does not re-derive the conflict — it only
enriches the ids `insert` already returned.

### Decision 3 — Replace-then-insert happens under one lock, insert-checked

The route performs removal and insertion inside a single `HemsState` write
critical section, and the insertion is still the foundation's checked `insert`. If
that insert fails for any reason, the removals are not committed. This keeps the
invariant enforcement in exactly one place (the queue) while making the pair
atomic, and means a concurrent submission cannot interleave between the removal
and the insertion.

*Alternative considered*: remove, then insert, as two state calls. Rejected — it
opens a window in which the user's older plan is gone and the new one is not yet
queued, and a failing insert would leave them with neither.

### Decision 4 — moved to `ev-session-queue-foundation` (its Decision 10)

The `earliest_start` → `window_start` mapping belongs to the foundation change:
without it every stated session opens at the submission instant, so any two overlap
and the refusal makes a second session impossible. Deferring it here would have left
a queue only the simulated schedule could use.

### Decision 5 — The UI prompts; it does not pre-check

The client submits optimistically and reacts to the conflict refusal, rather than
fetching the queue and testing for overlap before submitting. A client-side
pre-check would be a second copy of the overlap rule — the exact duplication the
foundation change centralised — and would still have to handle the server's
refusal anyway for the racing case.

## Risks / Trade-offs

- **The user confirms against a stale view** (the clashing plan expired or changed
  between refusal and confirmation) → Decision 1's exact-id matching refuses the
  resubmission and the UI re-prompts with the current conflict rather than removing
  the wrong thing.
- **A two-step flow is two round-trips** and the user could abandon mid-way →
  acceptable and deliberate: the abandoned state is "nothing changed", which is the
  safe outcome. Decision 5 keeps the first trip cheap.
- **Refusing a partial replace instruction may feel pedantic** when a user
  consciously wants to keep one of two clashing plans → that combination is not
  satisfiable (the windows still overlap), so the refusal is the truth; the prompt
  lists all clashes together so the user is never offered an impossible subset.
- **A new error path on an existing endpoint** could be mistaken by existing
  clients for a generic validation failure → the conflict is reported distinctly
  (spec requirement), and the UI is updated in this same change per
  `no-half-built-features`.

## Migration Plan

Additive: `replace_session_ids` is optional and omitting it preserves today's
semantics for every non-conflicting submission. The only behaviour change visible
to an existing client is that a *conflicting* EV submission now fails instead of
displacing — which is the intent, and which cannot occur before the foundation
change makes a queue possible.

Rollback is a revert; the queue from the foundation change stands on its own
without this one.

## Open Questions

- Which HTTP status the conflict uses (409 versus 422). Deferrable: it changes no
  requirement, and the UI branches on the distinct error kind rather than the
  status code.
- Whether the prompt should offer "keep both by shortening the earlier plan" as a
  third option. Deferrable and probably out of scope — it edits a plan the user did
  not open, and the spec deliberately offers only replace-or-decline.
