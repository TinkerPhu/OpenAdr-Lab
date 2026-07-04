# Research: 024-arch-gaps-complete

## Decision 1 — VtnPort async strategy

**Decision**: Add `async_trait = "0.1"` to `VEN/Cargo.toml`; annotate `VtnPort` trait and all
implementations (including `MockVtn`) with `#[async_trait::async_trait]`.

**Rationale**: `VtnClient` methods use `reqwest` (inherently async). `VtnPort` must be callable
as `&dyn VtnPort` so `MockVtn` can substitute in tests. Rust 1.75 `async fn in trait` works for
static dispatch (`impl VtnPort`) but not for dynamic dispatch (`dyn VtnPort`) without boxing.
`async_trait` handles the boxing automatically and is the established Rust pattern for this case.
No alternative avoids a new dependency: manually writing `Pin<Box<dyn Future>>` signatures is
semantically equivalent but far noisier, and would require wider edits.

**Alternatives considered**:
- `fn ... -> Pin<Box<dyn Future<Output=...> + Send + '_>>` — avoids new dep but every method
  signature becomes 3 lines; rejected for readability.
- `tokio::sync::oneshot` channel pattern — would change the calling convention significantly;
  rejected.

---

## Decision 2 — Typed VTN struct placement

**Decision**: Define `OadrEvent`, `OadrProgram`, `OadrReport`, and all nested types in
`VEN/src/controller/vtn_port.rs` alongside the `VtnPort` trait.

**Rationale**: `SimSnapshot`, `AssetSnapshot`, and `SnapshotError` all live in
`controller/simulator_port.rs` alongside `SimulatorPort`. Mirroring that pattern keeps port
types co-located with their trait definition and consistent across the codebase.
`openadr_interface.rs` (also in `controller/`) can import them directly with no circular path.

**Alternatives considered**:
- `entities/openadr.rs` — entities ring should be pure domain types; these are VTN API DTOs,
  not business entities. Rejected to preserve ring boundary clarity.
- `vtn.rs` (alongside `VtnClient`) — this is the driven adapter; mixing port traits with
  concrete implementations violates the hexagonal model. Rejected.

---

## Decision 3 — Minimal struct field scope

**Decision**: Model only fields currently consumed via `.get("field")` calls in the existing
codebase. Do not attempt to model the full OpenADR 3 schema.

**Rationale**: Confirmed by clarification Q1 — the project targets OpenADR 3 but OpenADR 3.1
introduces breaking field/type name changes. Minimal structs reduce migration cost when 3.1
is eventually adopted. Unknown fields are silently ignored (FR-016), so adding fields later
is non-breaking.

**Consumed fields (from codebase grep)**:

| Struct | Fields |
|--------|--------|
| `OadrEvent` | `id`, `programID`, `eventName` (opt), `intervals`, `reportDescriptors` |
| `OadrInterval` | `intervalPeriod` (opt), `payloads` |
| `OadrIntervalPeriod` | `start` (opt), `duration` (opt) |
| `OadrPayload` | `type`, `values` |
| `OadrReportDescriptor` | `payloadType`, `readingType` (opt), `frequency` (opt) |
| `OadrProgram` | `id`, `programName` |
| `OadrReport` | `id`, `reportName` |

`OadrPayload::values` remains `Vec<serde_json::Value>` — the values array contains mixed types
(float, string) depending on payload type. This internal use of `serde_json::Value` is
acceptable because it is not in a public VtnPort method signature.

---

## Decision 4 — ObligationService retry behaviour

**Decision**: `ObligationService::check_and_report()` propagates VTN errors to the caller
with no internal retry.

**Rationale**: Confirmed by clarification Q2. Services should be stateless and free of timing
logic. The obligation task loop runs every 5 seconds; a failed submission is naturally retried
on the next interval. Keeping retry in the task loop keeps the service synchronously testable.

---

## Decision 5 — BDD test obligations for structural refactoring

**Decision**: No new BDD scenarios required. Existing BDD suite must remain green.

**Rationale**: Constitution Principle II requires BDD for "new behavior." This feature is a
pure structural refactoring — no HTTP endpoints added, no response shapes changed, no new
business rules introduced. The new test surface (unit tests for services, contract tests for
VtnPort) is covered by `cargo test`, not BDD.

---

## Decision 6 — Implementation order

**Decision**: Gap 3 → Gap 2 → Gap 1.

| Gap | Reason for order |
|-----|-----------------|
| Gap 3 (tick.rs) | Standalone 10-line extraction; ships immediately to unblock CLAUDE.md invariant |
| Gap 2 (VTN typing) | Standalone; must complete before ObligationService can use `VtnPort` |
| Gap 1 (services) | Depends on `VtnPort` existing for `ObligationService`; also largest work item |

Within Gap 1, service extraction order:
`ObligationService` → `UserRequestService` → `HvacService/EvSessionService` → `PlanningService`

`ObligationService` first — smallest (wraps 20 lines of task logic), fastest to verify the
service pattern before tackling the complex planning acceptance gate.

---

## Dependency inventory

| Item | Current state | Change needed |
|------|--------------|---------------|
| `async_trait` | Not in Cargo.toml | Add `async_trait = "0.1"` |
| `thiserror` | Already in Cargo.toml | No change (used by VtnPortError) |
| `openadr_interface.rs` | Takes `&[serde_json::Value]` | Change to `&[OadrEvent]` after Gap 2 |
| `detect_event_changes` | Takes `&[serde_json::Value]` | Change to `&[OadrEvent]` after Gap 2 |
| `reporter.rs` | Takes `&serde_json::Value` (event param) | Change to `&OadrEvent` after Gap 2 |
| `PollingState` | `Vec<serde_json::Value>` for events/reports | Change to typed after Gap 2 |
| `tasks/planning.rs` | 324 lines, acceptance gate inline | Slim to ≤80 lines after Gap 1 |
| `services/mod.rs` | Contains only `pub mod test_support` | Expand to re-export services |
