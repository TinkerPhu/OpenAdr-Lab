## Context

The arbiter (`VEN/src/controller/arbiter.rs::reconcile`) runs once per sim tick and returns an
`ArbiterOutcome` carrying `active_lever: Option<&'static str>` — the cheapest lever that fired
this tick, or `None` if the deviation was within the dead band (or no plan exists yet). The tick
loop already threads this through `tasks::sim_tick::arbiter_glue::record_arbiter_outcome`, which
does two things every tick regardless of whether anything changed:

```rust
pub(crate) async fn record_arbiter_outcome(
    state: &crate::state::AppState,
    (active_lever, net_kw, dev_kw): (Option<String>, Option<f64>, Option<f64>),
    now: DateTime<Utc>,
) {
    state.set_arbiter_diagnostics(net_kw, dev_kw, active_lever.clone(), now).await;
    state.set_arbiter_active_lever(active_lever).await;
}
```

`AppState::arbiter_active_lever()` still holds the *previous* tick's value at the point this
function is entered (it's overwritten by the second line), so the previous/current pair needed
for edge detection is available for free at this exact call site — no new state field is needed.

Separately, `services/notify.rs::Notifier` is the established fan-out point (in-memory ring → SSE
broadcast → history store) for anything the resident should see, already used by three producers
(`notify_new_plan_warnings`, `notify_outage_edge`, and the dedup-aware `notify` core). All three
existing producer call sites live in `services/notify.rs` itself and are called from a task that
already holds a `Notifier` — `tasks::sim_tick` does not currently hold one.

Investigation for this change found `CorrectionBanner` (`VEN/ui/src/pages/Planner.tsx`) is
presently dead UI: it renders from SSE event types (`correction_active`/`correction_cleared`)
that `VEN/src/planner_events.rs`'s `PlannerEvent` enum has no variants for, and no code constructs
them (documented as a DRIFT callout in `wiki/components/deviation-arbiter.md` and
`wiki/components/ven-ui.md`). This change deliberately does not depend on or repair that path —
the new producer reads the arbiter's own tracked state directly (`arbiter_active_lever`), so
BL-37's fix lands independent of whether the Planner-tab banner is ever rewired.

Separately, three call sites duplicate the same push-and-evict-oldest-at-capacity ring logic:
`state/mod.rs::AppState::push_notification` (notifications, cap 200), `state/event_log.rs`
(operational event log, cap 200), and `state/report_submissions.rs` (report outcomes, cap 100).
Since this change already touches and re-tests the first of these (adding a new call path
through it), extracting the shared logic now avoids a second touch-and-retest round on the same
files later (R-46).

## Goals / Non-Goals

**Goals:**
- A sustained reactive correction (arbiter active-lever going from idle to firing) produces
  exactly one notification, visible via `GET /notifications`, the notification SSE stream, and
  `GET /notifications/history`, on every tab — not just Planner.
- Clearing produces at most one follow-up notification.
- No duplicate notifications while the underlying condition (something is actively correcting)
  stays continuously true, even if the specific lever handling it changes tick to tick.
- `RingBuffer<T>` becomes the single implementation of bounded push-and-evict-oldest, used by all
  three existing ring call sites, with identical externally observable eviction order/behavior.

**Non-Goals:**
- Rewiring `PlannerEvent::CorrectionActive`/`CorrectionCleared` or fixing `CorrectionBanner` —
  tracked separately (wiki DRIFT note), not part of this change.
- Per-lever or per-asset notification detail (e.g. "battery is now discharging to correct
  export excess") — the notification is intentionally generic; lever/asset-level detail already
  has a dedicated, richer surface (`GET /arbiter-diagnostics`, `ArbiterSettingsCard`).
- Any change to `RESIDUAL_THRESHOLD_FRACTION`/`ResidualThreshold` replanning — unrelated signal,
  already notified indirectly via `notify_new_plan_warnings` when a replan lands.
- Changing `NOTIFICATION_RING_CAP`, `EVENT_LOG_RING_CAP`, or `REPORT_SUBMISSION_RING_CAP` values.

## Decisions

### D1: Producer message is lever-agnostic, not lever-specific
The notification text is fixed ("Reactive correction active — a Layer-1 lever is adjusting a
setpoint to correct a sustained deviation" / "Reactive correction cleared") rather than
interpolating `active_lever`, `asset_id`, or the live `dev_kw` magnitude. Two reasons: (1) the
producer is edge-triggered on `is_some()` transitions only — if the lever handling a *continuing*
correction changes (e.g. battery hands off to heater mid-correction because the battery hit a SoC
bound), that is not a new edge and must not re-fire; a lever-specific message would then go stale
against the still-open notification. (2) BL-37's backlog text explicitly calls for "stable dedup
text" — a fixed string keeps the dedup key and message content decoupled from tick-to-tick
numeric noise. Lever/asset/magnitude detail remains available on the richer diagnostics surface
(`GET /arbiter-diagnostics`), consistent with `ui-transparency`'s "every derived state needs a
visible surface" being satisfied by that existing route, not duplicated here.

**Alternative considered**: embed `active_lever` in the message at edge time only (frozen at
creation, never updated on a lever handoff). Rejected — it would tell the user which lever
started the correction, but tick 2's actual lever could already differ, making the message
misleading rather than just generic; the diagnostics route is the correct place to look for
current lever.

### D2: Edge detection reuses `arbiter_active_lever`'s existing prev/current read, keyed on `is_some()`
`record_arbiter_outcome` already reads the previous tick's `arbiter_active_lever()` before
overwriting it. The new producer call is inserted between those two lines, comparing
`prev.is_some()` vs `active_lever.is_some()` (not equality of the two `Option<String>` values) —
a handoff between two different levers while a correction stays continuously active is `Some →
Some`, not an edge, and must not notify (goal: "no duplicates while continuously active").

**Alternative considered**: compare full `Option<String>` equality, treating a lever handoff as
clear-then-reactivate (two notifications). Rejected — contradicts the "no duplicates while the
correction condition stays continuously active" acceptance criterion; a handoff is not the user-
relevant event, "a correction is/isn't happening" is.

### D3: Notifier threaded into `tasks::sim_tick` via existing parameter-passing pattern
`spawn_sim_tick`, `tick_once`, and `record_arbiter_outcome` each gain a `notifier: Notifier`
parameter (or `&Notifier`), passed from `main.rs`'s already-constructed `notifier` — the same
shape already used for `poll_events`, `poll_signals`, and `planning`'s tasks. `Notifier` is
`Clone` (wraps a `broadcast::Sender` + `Option<Arc<dyn HistoryPort>>`), so cloning it into the
tick loop closure is cheap and consistent with how `state`/`vtn`/`weather` are already cloned
per tick. `spawn_sim_tick` and `tick_once` already carry `#[allow(clippy::too_many_arguments)]`
for the same reason (many independently-evolved feature inputs), so one more parameter doesn't
introduce a new lint suppression.

**Alternative considered**: put a `Notifier` handle directly on `AppState` so any task can reach
it without parameter threading. Rejected — out of scope scope-creep for this change, inconsistent
with the existing pattern (every other producer task takes `Notifier` as an explicit constructor/
call parameter, not via `AppState`), and would blur the Application/Adapter boundary (`AppState`
is plain shared state, `Notifier` is an application service with its own SSE channel).

### D4: `RingBuffer<T>` lives in `entities/` as a generic, zero-dependency type
New file `VEN/src/entities/ring_buffer.rs` wraps a `VecDeque<T>` with a fixed `capacity` and one
method, `push`, that evicts the oldest entry once at capacity before pushing (mirrors the exact
semantics already duplicated three times: `report_submissions.rs`'s pre-push evict-if-at-cap and
`event_log.rs`'s post-push evict-while-over-cap are the same observable behavior for a
single-push-at-a-time caller). Read accessors (`iter`, `len`, `into_vec` / oldest-first,
newest-first) stay minimal — each of the three call sites keeps its own domain-specific
`snapshot`/`since`-style read method, only the eviction-bearing write path is shared.
`entities/` is the Domain ring's natural home for a generic, business-logic-free container type:
it has zero outward dependencies (satisfies "inner rings never import outer rings" trivially),
and `state/` (which already imports `entities::*` throughout) can import it without violating the
dependency rule in either direction.

**Alternative considered**: a new top-level `VEN/src/util/` module. Rejected — the ring map in
`docs/architecture/VEN_ARCHITECTURE.md` doesn't define a `util/` ring, and `entities/` already
plays this role for other structural (non-domain-rule) types; introducing a new top-level module
for one type is unwarranted.

### D5: `RingBuffer<T>` does not use `DomainError`
Push-and-evict never fails (`VecDeque::push_back` is infallible short of OOM, which the codebase
does not model as a `Result` anywhere else) — there is no fallible cross-layer boundary being
crossed here, so `error-handling`'s `DomainError` translation rule doesn't apply. `RingBuffer<T>`
exposes only infallible methods.

### D6: Dedup key mirrors the `030` mechanism already in `notify.rs`, on top of edge-triggering
The producer still passes a stable `dedup_key` (e.g. `"arbiter-correction-active"`) to `notify()`
even though edge-triggering alone already prevents duplicate calls under normal operation. This
is defense-in-depth consistent with every other keyed producer in this codebase (`030`'s
established pattern) — cheap to add, and guards against a hypothetical rapid flap (active →
clear → active within the same tick's dedup window) collapsing into one entry instead of a burst,
without requiring new invariants beyond what `notify()` already provides. The "cleared" message
uses a separate key (`"arbiter-correction-cleared"`) since it is a distinct semantic event, not a
repeat of the active one.

## Risks / Trade-offs

- [Risk] A lever handoff mid-correction (D2) means the notification text can't say which lever is
  *currently* active without going stale. → Mitigation: message is deliberately lever-agnostic
  (D1); current lever stays discoverable via `GET /arbiter-diagnostics`.
- [Risk] Threading `Notifier` through three more function signatures (`spawn_sim_tick`,
  `tick_once`, `record_arbiter_outcome`) grows already-long parameter lists. → Mitigation: all
  three already carry `#[allow(clippy::too_many_arguments)]` with a documented rationale; this
  change doesn't introduce a new suppression, and a params-struct refactor of that call chain is
  an existing, separately-tracked concern, not this change's scope.
- [Risk] `RingBuffer<T>` migration touches three files whose existing unit tests (eviction order,
  cap boundaries) must keep passing unmodified. → Mitigation: test-first — the existing eviction
  tests in `event_log.rs`, `report_submissions.rs`, and `notify.rs` (`test_notification_ring_bounded_evicts_oldest`)
  are the regression net; `RingBuffer<T>`'s own new unit tests are written first and confirmed
  failing before implementation, then the three call sites are refactored one at a time with the
  existing suites re-run green after each.

## Migration Plan

Purely additive/internal — no data migration, no API/schema change, no feature flag needed (the
arbiter itself stays gated by the pre-existing `deviation_arbiter_enabled`, default `false`; this
change only adds a notification producer downstream of it, so it is inert wherever the arbiter is
already inert). Rollback is a plain revert — no persisted state format changes.

## Open Questions

None — the design closes the scope questions raised by BL-37's original text (which assumed the
SSE variants already existed) against what investigation found (they don't); see the Context
section's DRIFT reference.
