# OpenAdr-Lab Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-03-15

## Active Technologies
- Rust (stable, 2021 edition) + tokio (async runtime), axum (HTTP), chrono (timestamps), serde/serde_json, uuid, VecDeque (std) (004-ven-controller-reform)
- In-memory ring buffers (`VecDeque`); persisted fields use existing JSON persistence — no schema changes (004-ven-controller-reform)
- Rust (stable 2021) + TypeScript 5 + axum, chrono, serde_json (Rust); React 18 + MUI + TanStack React Query + recharts (TS) (005-ven-timeline-ui)
- In-memory ring buffers (`AssetHistoryBuffer` — VecDeque, 3600 rows) + in-memory `Plan` struct (005-ven-timeline-ui)
- TypeScript 5 (React 18) + React 18 + MUI v5 + TanStack React Query v5 + recharts (006-ven-raw-diagnostics)
- N/A (read-only diagnostic view; no persistence) (006-ven-raw-diagnostics)
- Rust (stable, 2021 edition) + chrono (timestamps), serde_json (report payloads), uuid, tokio (async runtime), axum (HTTP) (012-reporter-resampling)
- TypeScript 5 (React 18) + MUI v5, TanStack React Query v5, React Router v6 (all existing) (014-planner-viz-page)
- N/A — read-only diagnostic view; no persistence (014-planner-viz-page)
- Rust stable 2021 (VEN backend) + `axum`, `tokio`, `serde`, `uuid`, `chrono`, `good_lp`/HiGHS (015-planner-state-forecast)
- In-memory only — `HashMap` per `PlanTimeSlot`; no DB changes (015-planner-state-forecast)
- Rust stable (2021 edition) + tokio, axum, serde/serde_json, serde_yaml, uuid, chrono, (016-refactor-ven-backend)
- N/A — no new storage; JSON persistence via `state.json` is unchanged (016-refactor-ven-backend)
- Rust (stable, 2021 edition) + tokio, axum, serde, serde_json, serde_yaml, uuid, chrono, good_lp (HiGHS), sqlx (openleadr-rs) (018-split-loops-tasks)
- None (VEN persists state to JSON in /data/state.json) (018-split-loops-tasks)
- Rust stable (2021 edition) + tokio (async runtime), axum (HTTP), serde/serde_json, chrono, uuid (019-introduce-simulator-port)
- N/A — no new persistence; existing `/data/state.json` JSON persistence is unchanged (019-introduce-simulator-port)
- Rust stable (2021 edition) + `good_lp` (HiGHS MILP solver), `tokio`, `axum`, `serde`, `chrono` (020-milp-asset-port)
- N/A — no persistence changes (020-milp-asset-port)
- Rust stable 2021 edition + tokio (async runtime), axum (HTTP), serde/serde_yaml (infra ring only after Phase 4), good_lp / HiGHS (MILP solver — unchanged) (021-decouple-profile-domain)
- N/A — no persistence schema changes; existing `/data/sim_state.json` format is unchanged (021-decouple-profile-domain)
- Rust stable 2021 edition + tokio (async runtime), axum (HTTP), serde/serde_json (inject body), good_lp / HiGHS (MILP solver — unchanged) (022-deterministic-test-env)
- N/A — no persistence schema changes; `pv_plan_kw` is in-memory only (022-deterministic-test-env)

- [e.g., Python 3.11, Swift 5.9, Rust 1.75 or NEEDS CLARIFICATION] + [e.g., FastAPI, UIKit, LLVM or NEEDS CLARIFICATION] (004-ven-controller-reform)

## Project Structure

```text
backend/
frontend/
tests/
```

## Commands

cd src; pytest; ruff check .

## Code Style

[e.g., Python 3.11, Swift 5.9, Rust 1.75 or NEEDS CLARIFICATION]: Follow standard conventions

## Recent Changes
- 022-deterministic-test-env: Added Rust stable 2021 edition + tokio (async runtime), axum (HTTP), serde/serde_json (inject body), good_lp / HiGHS (MILP solver — unchanged)
- 021-decouple-profile-domain: Added Rust stable 2021 edition + tokio (async runtime), axum (HTTP), serde/serde_yaml (infra ring only after Phase 4), good_lp / HiGHS (MILP solver — unchanged)
- 020-milp-asset-port: Added Rust stable (2021 edition) + `good_lp` (HiGHS MILP solver), `tokio`, `axum`, `serde`, `chrono`


<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
