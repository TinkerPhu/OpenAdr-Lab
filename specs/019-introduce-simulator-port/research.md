# research.md

## Unknowns extracted from spec

1. SimSnapshot shape: which fields are required vs optional; units and ranges.
2. Snapshot error semantics: what error details are needed (transient vs fatal).
3. Concurrency model for SimulatorPort: synchronous borrowing or interior mutability? How to handle multi-threaded callers.
4. Placement and interface of MockSimulatorPort: where to put shared test support and how to structure it for reuse.
5. Fast unit-test harness patterns used in repo (existing test support modules).

## Research tasks and decisions

- Task: Determine canonical SimSnapshot fields by inspecting simulator/mod.rs and assets types.
- Task: Decide on SnapshotError enum variants (e.g., PartialData, Uninitialized, IoError).
- Task: Review existing test_support modules for placement of mocks (repo uses services/test_support guidance in plan doc).
- Task: Decide on concurrency model: prefer Send+Sync trait with &dyn reference and interior mutability on the implementor where needed.

## Decisions (initial suggestions)

- Decision: Use Result<SimSnapshot, SnapshotError> for snapshot() (explicit error handling).
  - Rationale: Matches Rust idioms; allows propagating clear error reasons to callers.
  - Alternatives: Option<SimSnapshot> (loses error details); SimSnapshot with Option fields (forces consumers to check many fields).

- Decision: Place shared mocks under `VEN/src/services/test_support/` as described in the plan and docs; expose a `MockSimulatorPort` there for reuse.
  - Rationale: Matches existing repository conventions referenced in the plan doc.

- Decision: Keep SimulatorPort trait as Send+Sync; implementations may use interior mutability (Mutex/MutexGuard) internally to manage state safely.
  - Rationale: Encourages concurrent test harnesses and aligns with existing AppCtx sharing patterns.

## Next steps

- Inspect `VEN/src/simulator/mod.rs` and `VEN/src/assets/` to extract exact SimSnapshot fields and types.
- Draft `data-model.md` with SimSnapshot shape and SimInjectState fields.
