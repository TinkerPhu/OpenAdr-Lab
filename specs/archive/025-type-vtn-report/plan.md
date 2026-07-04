# Implementation Plan: Type the VTN Report Interface

**Branch**: `025-type-vtn-report` | **Date**: 2026-05-14 | **Spec**: [spec.md](./spec.md)  
**Input**: Feature specification from `specs/025-type-vtn-report/spec.md`

## Summary

Replace every `serde_json::Value` on the public report-submission surface with four typed OpenADR 3 structs (`OadrReportBody`, `OadrReportResource`, `OadrReportInterval`, `OadrReportPayload`). Update `VtnPort::upsert_report` to accept `OadrReportBody` and return `Result<()>`. Convert all three reporter module public functions to return typed values. Align `VtnClient`, `MockVtn`, `routes/reports.rs`, and the three call-site tasks accordingly. This is a pure structural typing pass — no logic, behaviour, or data flow changes.

## Technical Context

**Language/Version**: Rust stable (2021 edition)  
**Primary Dependencies**: `serde`, `serde_json`, `axum`, `async_trait`, `anyhow`, `tokio`  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo test -p ven`  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: VEN backend Rust service (`VEN/src/`)  
**Performance Goals**: N/A — pure structural refactor  
**Constraints**: Zero logic changes; all existing test assertions must remain valid after signature updates  
**Scale/Scope**: Single crate, 8 source files touched

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I — OpenADR Spec Fidelity | ✅ Pass | All new struct field names match OpenADR 3 verbatim: `programID`, `eventID`, `clientName`, `reportName`, `resourceName`, `intervalPeriod`. Consistent with existing `OadrEvent` / `OadrPayload`. |
| II — BDD-First Testing | ⚠️ Partial | Internal type safety has no BDD representation. The **echo-back behavior change** on `POST /reports` (now returns submitted body instead of VTN body) IS observable — a BDD or integration test scenario must cover it. See Tasks. |
| III — Upstream Compatibility | ✅ N/A | No `openleadr-rs` submodule changes. |
| IV — Lean Architecture | ✅ Pass | No new abstractions. `json!{}` macros replaced directly with struct literals. `submit_report` (unused inherent method) removed. |
| V — Infrastructure Parity | ✅ N/A | No Docker / Compose changes. |
| VI — VEN Backend Hexagonal | ✅ Pass + pre-existing violation noted | This feature directly satisfies the grep invariant: `grep "serde_json::Value" VEN/src/vtn.rs → internal only`. **Pre-existing violation**: `reporter.rs` is 959 lines (limit: 500). Not introduced here; splitting is out of scope. Tracked in Complexity Tracking below. |

**Gate result**: PASS with one required action — BDD scenario for `POST /reports` echo-back (Principle II).

## Project Structure

### Documentation (this feature)

```text
specs/025-type-vtn-report/
├── plan.md              ← this file
├── research.md          ← Phase 0 output
├── data-model.md        ← Phase 1 output
├── quickstart.md        ← Phase 1 output
├── contracts/
│   └── post-reports-body.md   ← Phase 1 output
└── tasks.md             ← Phase 2 output (/speckit.tasks — not created here)
```

### Source Code (repository root)

```text
VEN/src/
├── controller/
│   ├── vtn_port.rs          ← add OadrReport{Body,Resource,Interval,Payload}; update VtnPort::upsert_report
│   └── reporter.rs          ← update 4 public fn return types; replace json!{} with struct construction
├── vtn.rs                   ← update VtnClient::upsert_report inherent + VtnPort impl; remove submit_report
├── routes/
│   └── reports.rs           ← update post_reports: typed request body, echo-back response
├── services/
│   ├── obligation.rs        ← call-site type update only (no logic change)
│   └── test_support/
│       └── mock_vtn.rs      ← update trait impl, submitted() return type, internal tests
└── tasks/
    ├── planning.rs          ← call-site type update only (no logic change)
    └── sim_tick/
        └── publish.rs       ← call-site type update only (no logic change)
```

**Structure Decision**: Single Rust crate (`VEN/`). All changes confined to `VEN/src/`. No new files created in source tree — all typed structs land in the existing `controller/vtn_port.rs` module alongside `OadrEvent`, `OadrProgram`, etc.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| `reporter.rs` is 959 lines (Constitution VI: 500-line limit) | Pre-existing violation — not introduced by this feature. Splitting reporter.rs would require a separate architectural task. | Splitting inside this PR would expand scope beyond a pure typing pass and risk merge conflicts with in-flight work. Deferred to a follow-up refactor. |
