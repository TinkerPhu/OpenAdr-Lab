# Research: Type the VTN Report Interface

**Feature**: `025-type-vtn-report`  
**Phase**: 0 — Outline & Research  
**Date**: 2026-05-14

## Summary

All technical questions are resolved from existing codebase evidence. No external research required — the patterns, serde attributes, and axum behaviour needed are already present in the codebase.

---

## Decision 1: Struct placement — `controller/vtn_port.rs`

**Decision**: All four new structs (`OadrReportBody`, `OadrReportResource`, `OadrReportInterval`, `OadrReportPayload`) are defined in `VEN/src/controller/vtn_port.rs`.

**Rationale**: `OadrEvent`, `OadrProgram`, `OadrReport`, `OadrPayload`, and `OadrIntervalPeriod` all live in this module. The typed report structs are port-boundary DTOs; they belong in the same module as the `VtnPort` trait. All consumers already import from `controller::vtn_port`.

**Alternatives considered**: A dedicated `controller/report_dto.rs` module — rejected as unnecessary indirection (Principle IV). Three similar types in one module is preferable to splitting.

---

## Decision 2: `r#type` field pattern for reserved keyword `type`

**Decision**: Use `pub r#type: String` (raw identifier syntax), consistent with the existing `OadrPayload` struct.

**Evidence**: `VEN/src/controller/vtn_port.rs` line 72:
```rust
pub struct OadrPayload {
    pub r#type: String,
    ...
}
```
`serde` serializes `r#type` as `"type"` automatically. No `#[serde(rename)]` attribute needed.

**Rationale**: The existing `OadrPayload::r#type` proves this pattern works end-to-end in this codebase.

---

## Decision 3: `eventID: Option<String>` serialization — `skip_serializing_if`

**Decision**: Apply `#[serde(skip_serializing_if = "Option::is_none")]` to `OadrReportBody::eventID`.

**Rationale**: `build_status_report` produces reports with no `eventID`. The current `json!{}` macro simply omits the key when not constructed. The typed struct must replicate this: absent field (no key in JSON), not `"eventID": null`.

**Evidence**: `OadrEvent::eventName` uses `#[serde(default)]` for deserialization tolerance. For serialization, `skip_serializing_if` is the correct complement. Pattern also used throughout the codebase for optional OpenADR fields.

---

## Decision 4: `OadrReportPayload::values` stays `Vec<serde_json::Value>`

**Decision**: The `values` field remains `Vec<serde_json::Value>` — this is intentional, not a gap.

**Rationale**: OpenADR 3 defines `values` as a heterogeneous array. Current payloads include:
- `["ACTIVE"]` — string (OPERATING_STATE)
- `[1234.5]` — float64 (USAGE / power in watts)
- `["82.3"]` — string (STORAGE_CHARGE_LEVEL as formatted percentage)
- `["PlanCycle trigger=..."]` — string (TELEMETRY_STATUS)

A single concrete Rust type cannot represent this polymorphism without a custom enum. `Vec<serde_json::Value>` is the established pattern (used identically in `OadrPayload::values`). This field is internal structure, not a port boundary.

---

## Decision 5: `VtnClient::upsert_report` — 409 upsert logic after return type change

**Decision**: The inherent `upsert_report` method on `VtnClient` serializes `OadrReportBody` to `serde_json::Value` at the start, then proceeds with existing 409 handling, returning `Result<()>`.

**Rationale**: The existing 409 path extracts `reportName` from the body (`body.get("reportName")`). After typing, this becomes `&body.reportName` — a direct field access. The `update_report` call (which returns `Result<serde_json::Value>`) is called with `.await?` and its return value is discarded. No logic change — only the plumbing changes.

**Concrete before/after**:
```rust
// Before
let report_name = body.get("reportName").and_then(|v| v.as_str())...;
return self.update_report(&id, body).await;  // returns Result<serde_json::Value>

// After
let report_name = &body.reportName;
let value = serde_json::to_value(&body)?;
self.update_report(&id, value).await?;       // discard response, return Ok(())
return Ok(());
```

---

## Decision 6: `submit_report` removal

**Decision**: `VtnClient::submit_report` (line 259, `vtn.rs`) is removed.

**Rationale**: `submit_report` is defined but has zero call sites in the codebase (confirmed by `grep -r "submit_report" VEN/src/` — no matches outside `vtn.rs` itself). It wraps `post_json("/reports", body)` without upsert semantics and is superseded by `upsert_report`. Per Principle IV (Lean Architecture), dead code is removed.

---

## Decision 7: `post_reports` HTTP handler response

**Decision**: On success, `post_reports` returns `201 Created` with the submitted `OadrReportBody` echoed back as the JSON response body.

**Rationale**: Confirmed in clarification session (2026-05-14 Q1). The current handler returns the VTN response body; after `VtnPort::upsert_report` returns `Result<()>` the VTN body is no longer accessible through the trait. Echo-back is the least-breaking change: preserves 201 status code, gives callers a confirmation of what was accepted, requires no extra HTTP round-trip.

**Implementation**: Clone `body` before calling `upsert_report` (since `OadrReportBody` derives `Clone`), return the clone on success.

---

## Decision 8: Private helpers (`build_soc_intervals`, interval construction)

**Decision**: Private helpers in `reporter.rs` that currently return `Vec<serde_json::Value>` are updated to return `Vec<OadrReportInterval>` as part of this pass.

**Rationale**: The public functions return `Option<OadrReportBody>` / `Vec<OadrReportBody>`. Their internal helpers must also produce typed intervals, otherwise the boundary between private and public is still untyped. The helpers are already clearly scoped; updating them is part of the same typing pass.

---

## Pre-existing issues noted (not fixed in this PR)

| Issue | Location | Action |
|---|---|---|
| `reporter.rs` is 959 lines (> 500 limit, Constitution VI) | `VEN/src/controller/reporter.rs` | Deferred — splitting is a separate refactoring task |
| `run_measurement_reports` in `tasks/sim_tick/publish.rs` calls `&VtnClient` directly (bypasses `VtnPort` trait) | `VEN/src/tasks/sim_tick/publish.rs:123` | Pre-existing — no change in this PR |
