# research.md

## Phase 0 — Unknowns from spec

| # | Unknown | Status |
|---|---------|--------|
| 1 | SimSnapshot shape: which fields are required vs optional; units and ranges | ✅ Resolved |
| 2 | Snapshot error semantics: what error details are needed (transient vs fatal) | ✅ Resolved |
| 3 | Concurrency model for SimulatorPort: synchronous borrowing or interior mutability? | ✅ Resolved |
| 4 | Placement and interface of MockSimulatorPort | ✅ Resolved |
| 5 | Fast unit-test harness patterns used in repo | ✅ Resolved |

---

## Decisions

### D-001: `snapshot()` return type

**Decision**: `Result<SimSnapshot, SnapshotError>`

**Rationale**: Allows callers to distinguish uninitialized (no tick yet), transient (sim locked), and fatal states. Rust idioms prefer `Result` over sentinel values. Plain `SimSnapshot` forces callers to check individual Option fields with no structured error path.

**Alternatives considered**:
- `Option<SimSnapshot>` — loses error detail; callers cannot distinguish "not ready" from "broken"
- `SimSnapshot` with all-Option fields — forces consumers to check many fields; no error path

---

### D-002: `inject()` return type

**Decision**: `()` — fire-and-forget

**Rationale**: Injection is a best-effort sim override (UI control). The caller has no recovery path if it fails — the simulator self-corrects on the next tick. If the simulator is uninitialized, the inject is silently dropped. A `Result` return would encourage callers to write error handling for a case that cannot be meaningfully handled.

**Alternatives considered**:
- `Result<(), InjectError>` — adds burden without benefit; callers cannot recover

---

### D-003: SimSnapshot field set

**Decision**: `ts: DateTime<Utc>`, `grid: GridSnapshot`, `assets: HashMap<String, AssetSnapshot>`. History buffers explicitly excluded.

**Rationale**: Derived from `VEN/src/simulator/mod.rs` (authoritative source). `AssetHistoryBuffer` has been moved to `VEN/src/assets/mod.rs` — history is a read-only query concern, not a snapshot concern. Including history in the snapshot would couple controller logic to history buffer lifetimes.

**Alternatives considered**:
- Include history in snapshot — rejected; violates clean port boundary and increases snapshot size

---

### D-004: SnapshotError variants

**Decision**: Three variants — `Uninitialized`, `Transient`, `Fatal`

**Rationale**: Covers all practical failure modes: not started yet (wait), temporarily blocked (retry), and broken (abort). Avoids over-engineering a detailed error hierarchy for a simple port.

**Alternatives considered**:
- Single `SnapshotFailed` variant — insufficient; callers need to distinguish retry vs abort

---

### D-005: Concurrency model

**Decision**: Trait is `Send + Sync`; implementations use interior mutability (`Mutex`/`RwLock`) internally; callers receive `&dyn SimulatorPort` or `Arc<dyn SimulatorPort>`

**Rationale**: Matches existing `AppCtx` sharing patterns in the VEN. Trait object-safety is preserved. Callers are not burdened with lock management — the implementation owns its synchronisation strategy.

**Alternatives considered**:
- `&mut self` trait methods — breaks object safety; not compatible with `Arc<dyn Trait>` sharing

---

### D-006: MockSimulatorPort placement

**Decision**: `VEN/src/services/test_support/mock_simulator_port.rs` (compiled in all builds, not `#[cfg(test)]`)

**Rationale**: Matches constitution Principle VI verbatim: "Mock adapters live in `VEN/src/services/test_support/`". Compiling outside `#[cfg(test)]` makes the mock shareable across service test modules, not just the crate defining it.

**Alternatives considered**:
- `#[cfg(test)]` module in `controller/simulator_port.rs` — not shareable across crates/modules

