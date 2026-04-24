# Plan: Planner Status — SSE Push Stream with Progress Events

## Context

The Planner tab already has an optimization-objective dropdown (implemented). When the user picks a new objective, a replan is triggered but there is no feedback — the plan silently updates 18–60 s later (HiGHS solve time on Pi4 ARM64). This change adds an SSE stream at `GET /plan/events` that pushes three event types to the browser: `solving_started`, `solving_progress` (1 s ticks), and `plan_ready`. The frontend renders a live status chip/progress bar using these events.

### Key constraints
- `good_lp` does **not** expose HiGHS internal callbacks → iteration count = wall-clock 1 s ticks from a concurrent tokio task, not true HiGHS iterations. This is honest: the comment at `loops.rs:647` already documents the 18–60 s block.
- `run_planner` is currently a **blocking call on a tokio async thread** — must move to `tokio::task::spawn_blocking` so the progress ticker task can fire concurrently (the async runtime is otherwise blocked during the solve).
- `tokio-stream` (with `sync` feature) is used for `ReceiverStream` in the SSE handler.
- Frontend `EventSource` is native browser API — no new npm package needed.

---

## Backend Changes

### 1. `VEN/Cargo.toml`
Add:
```toml
tokio-stream = { version = "0.1", features = ["sync"] }
```

### 2. New file `VEN/src/planner_events.rs`
Define the event enum serialised as a tagged SSE payload:
```rust
use crate::profile::PlannerObjective;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlannerEvent {
    SolvingStarted {
        objective: PlannerObjective,
        num_slots: usize,
        triggered_at: DateTime<Utc>,
    },
    SolvingProgress {
        elapsed_ms: u64,
        iteration: u32,   // wall-clock tick count (1 per second), not HiGHS internals
    },
    PlanReady {
        plan_id: Uuid,
        objective: PlannerObjective,
        solver_ms: u64,
        objective_eur: f64,
        slot_count: usize,
    },
}
```
Expose `pub type PlannerEventTx = Arc<tokio::sync::broadcast::Sender<PlannerEvent>>;`

### 3. `VEN/src/main.rs`
- Add `pub planner_event_tx: PlannerEventTx` field to `AppCtx`
- Initialise: `let (event_tx, _) = tokio::sync::broadcast::channel::<PlannerEvent>(128);`
- Pass `Arc::new(event_tx)` into `AppCtx` and `spawn_planning(…)`

### 4. `VEN/src/loops.rs` — `spawn_planning()`
Add `event_tx: PlannerEventTx` parameter. Restructure the solve section:

```rust
// ── Before solve ──────────────────────────────────────────────
let num_slots = profile.planner.plan_horizon_h as usize
    * 3600 / profile.planner.plan_step_s as usize;
let _ = event_tx.send(PlannerEvent::SolvingStarted {
    objective: obj,
    num_slots,
    triggered_at: now,
});

// ── Spawn 1 s progress ticker ──────────────────────────────────
let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
let progress_tx = event_tx.clone();
let ticker_task = tokio::spawn(async move {
    let start = std::time::Instant::now();
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut iteration: u32 = 0;
    let mut cancel_rx = cancel_rx;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                iteration += 1;
                let _ = progress_tx.send(PlannerEvent::SolvingProgress {
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    iteration,
                });
            }
            _ = &mut cancel_rx => break,
        }
    }
});

// ── Run blocking HiGHS solve off the async runtime ────────────
let solve_start = std::time::Instant::now();
let profile_clone = profile.clone();   // Arc<Profile>, cheap
let plan = tokio::task::spawn_blocking(move || {
    controller::milp_planner::run_planner(
        &sim_snap, &tariff_ts, &capacity, &profile_clone,
        now, trigger,
        ev_sess.as_ref(), heat_tgt.as_ref(), &shift_loads,
        bl_override.as_ref(), Some(obj),
    )
}).await.expect("planner task panicked");
let solver_ms = solve_start.elapsed().as_millis() as u64;

// ── Cancel ticker, emit plan_ready ────────────────────────────
let _ = cancel_tx.send(());
ticker_task.await.ok();
let _ = event_tx.send(PlannerEvent::PlanReady {
    plan_id: plan.id,
    objective: obj,
    solver_ms,
    objective_eur: plan.objective_eur,
    slot_count: plan.slots.len(),
});
```

All of `sim_snap`, `tariff_ts`, `capacity`, `ev_sess`, `heat_tgt`, `shift_loads`, `bl_override` are already owned local variables — they move into the `spawn_blocking` closure cleanly.

### 5. `VEN/src/routes/hems.rs`
Add the SSE handler:

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

pub async fn get_plan_events(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let mut bcast_rx = ctx.planner_event_tx.subscribe();
    // Bridge broadcast → mpsc so lagged clients don't kill the broadcast sender
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        loop {
            match bcast_rx.recv().await {
                Ok(evt) => {
                    if let Ok(data) = serde_json::to_string(&evt) {
                        if fwd_tx.send(Event::default().data(data)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let stream = ReceiverStream::new(fwd_rx).map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

### 6. `VEN/src/routes/mod.rs`
```rust
.route("/plan/events", get(hems::get_plan_events))
```

### 7. `VEN/src/main.rs` mod declarations
Add `mod planner_events;` alongside the other module declarations.

---

## Frontend Changes

### 8. `VEN/ui/src/api/types.ts`
```typescript
export type PlannerEvent =
  | { type: "solving_started"; objective: PlannerObjective; num_slots: number; triggered_at: string }
  | { type: "solving_progress"; elapsed_ms: number; iteration: number }
  | { type: "plan_ready"; plan_id: string; objective: PlannerObjective; solver_ms: number; objective_eur: number; slot_count: number };
```

### 9. `VEN/ui/src/api/hooks.ts`
```typescript
import { useRef, useEffect } from "react";
import { PlannerEvent } from "./types";

export function usePlannerEvents(onEvent: (event: PlannerEvent) => void): void {
  const { api } = useVenContext();
  // Ref keeps callback stable so EventSource isn't re-created on every render
  const cbRef = useRef(onEvent);
  cbRef.current = onEvent;

  useEffect(() => {
    const es = new EventSource(`${api.baseUrl}/plan/events`);
    es.onmessage = (e) => {
      try { cbRef.current(JSON.parse(e.data) as PlannerEvent); } catch { /* ignore */ }
    };
    return () => es.close();
  }, [api.baseUrl]); // reconnect only when VEN URL changes
}
```

### 10. `VEN/ui/src/pages/Planner.tsx`
Add planner status state and SSE subscription. Insert a `<PlannerStatusBar>` between the objective dropdown and `<PlanHeaderBar>`.

**State:**
```typescript
type PlannerStatus =
  | { phase: "idle" }
  | { phase: "solving"; elapsed_ms: number; iteration: number; objective: PlannerObjective }
  | { phase: "updated"; solver_ms: number };

const [plannerStatus, setPlannerStatus] = useState<PlannerStatus>({ phase: "idle" });
const queryClient = useQueryClient();
```

**SSE subscription:**
```typescript
usePlannerEvents(useCallback((event: PlannerEvent) => {
  if (event.type === "solving_started") {
    setPlannerStatus({ phase: "solving", elapsed_ms: 0, iteration: 0, objective: event.objective });
  } else if (event.type === "solving_progress") {
    setPlannerStatus((prev) =>
      prev.phase === "solving"
        ? { ...prev, elapsed_ms: event.elapsed_ms, iteration: event.iteration }
        : prev
    );
  } else if (event.type === "plan_ready") {
    setPlannerStatus({ phase: "updated", solver_ms: event.solver_ms });
    queryClient.invalidateQueries({ queryKey: ["plan"] });
    // Fade back to idle after 3 s
    setTimeout(() => setPlannerStatus({ phase: "idle" }), 3000);
  }
}, [queryClient]));
```

**`PlannerStatusBar` inline component** (can be a small function in the same file):
```tsx
function PlannerStatusBar({ status }: { status: PlannerStatus }) {
  if (status.phase === "idle") return null;
  if (status.phase === "solving") return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 1 }}>
      <CircularProgress size={16} />
      <Typography variant="body2" color="text.secondary">
        Solving ({OBJECTIVE_OPTIONS.find(o => o.value === status.objective)?.label})
        — {(status.elapsed_ms / 1000).toFixed(0)} s, iteration {status.iteration}
      </Typography>
      <LinearProgress sx={{ flex: 1 }} />
    </Box>
  );
  // phase === "updated"
  return (
    <Chip
      size="small"
      color="success"
      label={`Plan updated — solved in ${(status.solver_ms / 1000).toFixed(1)} s`}
      sx={{ mb: 1 }}
    />
  );
}
```

Render in `Planner.tsx` just above `<PlanHeaderBar>`:
```tsx
<PlannerStatusBar status={plannerStatus} />
```

---

## Files Modified

| File | Change |
|---|---|
| `VEN/Cargo.toml` | + `tokio-stream` |
| `VEN/src/planner_events.rs` | new — `PlannerEvent` enum |
| `VEN/src/main.rs` | broadcast channel + `AppCtx.planner_event_tx` |
| `VEN/src/loops.rs` | `spawn_blocking` + ticker task + 3 event emits |
| `VEN/src/routes/hems.rs` | `get_plan_events` SSE handler |
| `VEN/src/routes/mod.rs` | register `/plan/events` |
| `VEN/ui/src/api/types.ts` | `PlannerEvent` discriminated union |
| `VEN/ui/src/api/hooks.ts` | `usePlannerEvents` hook |
| `VEN/ui/src/pages/Planner.tsx` | status state + `PlannerStatusBar` inline component |

---

## Verification

1. `cargo build` in `VEN/` — no errors
2. Deploy to Pi4, open Planner tab
3. Change objective → within 1 s: spinner appears with "Solving (Autarky) — 0 s, iteration 0"
4. Each second: iteration counter increments, elapsed time ticks up
5. After HiGHS completes (~20–60 s on Pi4): green "Plan updated — solved in 23.4 s" chip appears, plan data refreshes immediately, chip fades after 3 s
6. Verify multiple browser tabs all receive events simultaneously (broadcast channel)
7. Run existing BDD suite — no regressions (SSE endpoint is additive; `spawn_blocking` doesn't change plan output)
8. Verify `GET /plan/events` response has `Content-Type: text/event-stream`
