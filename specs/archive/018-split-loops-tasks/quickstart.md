# quickstart.md

Quickstart: How to implement "Split loops.rs into tasks/" (Phase 1)

Prerequisites
- Ensure working tree is clean and branch `018-split-loops-tasks` is checked out.
- Record baseline unit test count: `cargo test --no-run -- --list` or run `cargo test` and note passing count.
- Run a BDD subset that validates startup and a small set of scenarios (see steps below).

Step-by-step

1. Create the `VEN/src/tasks/` directory and add `mod.rs`.
   - `VEN/src/tasks/mod.rs` should declare submodules and re-export public spawn_* functions as re-exports matching prior `loops` public names.

2. Move one concern at a time (recommended order):
   - poll_events -> tasks/poll_events.rs
   - poll_programs -> tasks/poll_programs.rs
   - poll_reports -> tasks/poll_reports.rs
   - obligation -> tasks/obligation.rs
   - planning -> tasks/planning.rs
   - sim_tick -> tasks/sim_tick.rs
   - state_persist -> tasks/state_persist.rs

For each moved file:
- Move the spawn_* function and any helpers exclusively used by it into the new TaskFile.
- Move the corresponding #[cfg(test)] module into the same file (tests are excluded from the 200-line cap).
- Update `VEN/src/tasks/mod.rs` to `pub use` the spawn_* name so other modules see the same symbol (no other code changes required).
- Run `cargo test` and a small BDD subset. If tests fail, revert the move and debug.

3. Shared helpers
- If a helper is used by multiple spawn_* functions, add it to `VEN/src/tasks/shared.rs` with `pub(crate)` visibility and import it where needed.

4. File size rule
- Keep production code under 200 lines. If a file would exceed that threshold (excluding tests), extract sub-concerns into a subdirectory (e.g., `tasks/sim_tick/mod.rs` with helpers in `tasks/sim_tick/`).

5. Finalization
- After all concerns moved and verified, delete `VEN/src/loops.rs` and ensure no `mod loops` or `crate::loops` references remain.
- Update `VEN/src/main.rs` module path (if necessary) from `loops::` to `tasks::` — ideally only `use` statements change.
- Run full `cargo test` and the entire BDD suite on Pi4-Server.

Commands
- Baseline tests: `cargo test` (record passing count)
- Local verification after each move: `cargo test` && `pytest -k "ven_startup"` (or run a small behave tag set)
- Rebuild VEN image and run BDD full suite on Pi4-Server as final gate.

Notes
- Keep commits small and DCO-signed. Run `cargo fmt` and `cargo clippy` before pushing. Do not attempt concurrency/locking changes in Phase 1.
