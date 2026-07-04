# Research: Fix VtnClient References in Remaining Task Files

**Status**: Complete — no unknowns  
**Date**: 2026-05-16

## Summary

No external research required. All decisions are fully specified in
`docs/plans/post_refactoring_fixes.md` (Item 1) and confirmed by reading the four source files.

## Findings

### Decision: Use `Arc<dyn VtnPort>` as the parameter type

**Rationale**: Consistent with the already-refactored `planning.rs` and `sim_tick/` tasks, which
receive `vtn_port: Arc<dyn VtnPort>`. The `Arc` wrapper is necessary because the async closure
moves the value and may be polled across await points.

**Alternatives considered**: `Box<dyn VtnPort>` — rejected; `Arc` is already the project-wide
convention for shared, Send-safe trait objects in tokio tasks. `&dyn VtnPort` — rejected; cannot
cross the async move boundary.

### Decision: `vtn.as_ref()` for the obligation service call

**Rationale**: `ObligationService::check_and_report` already accepts `&dyn VtnPort`. The
`Arc<dyn VtnPort>::as_ref()` call coerces correctly to `&dyn VtnPort` without reborrowing issues.
Alternative `&*vtn` also works but `as_ref()` is more idiomatic for `Arc`.

### Confirmed: `vtn` (VtnClient) remains in `main.rs` scope

`main.rs` line 242 assigns `vtn` into `AppCtx { vtn, ... }`. That usage is in routes (driving
adapter) where the concrete type is acceptable. Only the four spawn call sites are changed.
