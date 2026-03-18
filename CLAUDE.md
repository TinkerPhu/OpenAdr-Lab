# OpenAdr-Lab Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-03-15

## Active Technologies
- Rust (stable, 2021 edition) + tokio (async runtime), axum (HTTP), chrono (timestamps), serde/serde_json, uuid, VecDeque (std) (004-ven-controller-reform)
- In-memory ring buffers (`VecDeque`); persisted fields use existing JSON persistence — no schema changes (004-ven-controller-reform)
- Rust (stable 2021) + TypeScript 5 + axum, chrono, serde_json (Rust); React 18 + MUI + TanStack React Query + recharts (TS) (005-ven-timeline-ui)
- In-memory ring buffers (`AssetHistoryBuffer` — VecDeque, 3600 rows) + in-memory `Plan` struct (005-ven-timeline-ui)
- TypeScript 5 (React 18) + React 18 + MUI v5 + TanStack React Query v5 + recharts (006-ven-raw-diagnostics)
- N/A (read-only diagnostic view; no persistence) (006-ven-raw-diagnostics)

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
- 006-ven-raw-diagnostics: Added TypeScript 5 (React 18) + React 18 + MUI v5 + TanStack React Query v5 + recharts
- 005-ven-timeline-ui: Added Rust (stable 2021) + TypeScript 5 + axum, chrono, serde_json (Rust); React 18 + MUI + TanStack React Query + recharts (TS)
- 004-ven-controller-reform: Added Rust (stable, 2021 edition) + tokio (async runtime), axum (HTTP), chrono (timestamps), serde/serde_json, uuid, VecDeque (std)


<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
