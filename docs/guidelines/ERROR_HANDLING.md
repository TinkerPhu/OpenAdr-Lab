# Error Handling — the DomainError pattern

Status: adopted (2026-07-17). Applies to VEN/src/ and, where a domain layer
exists, to the VTN BFF.

## The pattern in one paragraph

All failures that cross a layer boundary are expressed in **one domain-owned
error vocabulary**: the `DomainError` enum in `VEN/src/entities/error.rs`
(`thiserror`-derived). Outer rings translate their technical errors
(`reqwest::Error`, `rusqlite::Error`, solver failures, …) into a domain
variant **at the boundary where they occur**. Inner rings never see — and
never name — an infrastructure error type. This is a project design decision
(Clean Architecture / DDD), not a Rust language rule; the compiler only
enforces it indirectly via the ring-import rules in `VEN_ARCHITECTURE.md`.

## Rules

1. **One vocabulary.** New failure modes become a variant of `DomainError`
   (or enrich an existing one). Do not introduce parallel per-module error
   enums that cross ring boundaries.

2. **Translate at the boundary.** The adapter/infra code that receives a
   technical error converts it to a `DomainError` immediately, attaching the
   context only that boundary knows (URL, asset id, table name, …).
   Example: `vtn.rs` maps connect/timeout `reqwest::Error`s to
   `VtnUnreachable`; the history store maps `rusqlite::Error` to
   `StorageError`.

3. **Context over advice — and structured over stringified.** A variant
   should carry the facts a reader needs to understand the failure, as
   **typed fields** (with unit suffixes per the naming rule), not a
   pre-flattened `String`. Prefer
   `VtnUnreachable { url: String, kind: ConnectFailureKind }` over
   `VtnUnreachable(String)` when the context has structure. Structured
   fields let boundaries compute good log lines and UI messages, and let
   tests assert on causes.

4. **Remediation only within the component's own vocabulary.** A variant's
   message (or a `help()` hint) may suggest a fix **only if the fix is
   expressible in the erroring component's own domain terms** —
   "session conflict: another EV session is active; end it first" is fine.
   Anything referencing deployment, config files, docker, or other services
   is outer-ring knowledge: if such a hint is wanted, it is produced at the
   presentation boundary (route error-mapper, log site, UI), never hardcoded
   into `entities/error.rs`.

5. **Display stays terse.** The `#[error("...")]` string is one line:
   what failed + the identifying context. No multi-sentence advice — errors
   get chained, wrapped, and logged repeatedly; verbose Display pollutes
   every consumer.

6. **One error, several audiences — map per audience at the boundary:**
   - `tracing` logs (stdout → docker logs): the exhaustive developer/operator
     record. Every error boundary logs with structured fields.
   - UI notification feed (`services/notify.rs`): curated and deduplicated;
     only resident-relevant events, edge-triggered where the underlying
     condition is continuous (see `outage_transition`).
   - HTTP responses: status + terse message; no internals, no hints that
     leak deployment detail.

7. **Infallible-by-design ports stay infallible.** Where a port contract
   promises a usable result (e.g. `SolverPort::solve` always returns a
   fallback `Plan` + `PlanWarning`), the corresponding `DomainError` variant
   is logged at the boundary, not propagated. Don't "fix" that by threading
   `Result` through the port.

8. **No dead variants.** A variant must be constructed at a real error
   boundary. Reserved/speculative variants (like `ProfileInvalid` today)
   need a comment naming the backlog item that will use them, or they get
   removed.

## Related

- `docs/architecture/VEN_ARCHITECTURE.md` — ring map and dependency rule.
- `docs/guidelines/TESTING.md` — every variant needs a Display test and,
  where translated, a boundary-translation test.
- `docs/reference/TECHNICAL_DEBTS.md` — record deviations here immediately.
