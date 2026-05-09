# contracts/simulator-port.md

## SimulatorPort contract

Trait: SimulatorPort (Rust)

- snapshot(&self) -> Result<SimSnapshot, SnapshotError)
  - Returns the most recent simulator state snapshot suitable for read-only controller logic.
  - Must not include historical buffers; history is accessed via assets read routes.

- inject(&self, state: SimInjectState)
  - Mutates simulator parameters for the next tick (e.g., overrides). Implementations may buffer injected state for the next tick.

## Snapshot shape (SimSnapshot)
- ts: DateTime<Utc]
- grid: GridSnapshot
- assets: HashMap<String, AssetSnapshot>

## Error semantics (SnapshotError)
- Uninitialized — simulator hasn't produced a snapshot yet
- Transient — temporary condition (e.g., sim locked); clients may retry
- Fatal — unrecoverable state; client should abort the operation and alert operator

## Concurrency
- Implementations must be Send + Sync; interior mutability is allowed (Mutex/RwLock) but implementations should minimize lock hold times.

## Mocking
- Provide MockSimulatorPort that implements the trait for unit tests; allow deterministic construction of SimSnapshot and capture of inject calls.


