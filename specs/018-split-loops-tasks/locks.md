# Locking preservation guide — 018-split-loops-tasks

Purpose
-------
Document named locks and the verification steps required to satisfy NFR-001 (preserve existing runtime and locking semantics exactly) during the incremental migration.

Known locks (start here and expand after code inventory)
------------------------------------------------------
- SimState (e.g., `SimState: Arc<Mutex<...>>`) — held by the simulator loop and accessed by planner/tick logic. Search for `sim.lock` / `ctx.sim.lock()` / `SimState` in `VEN/src/`.
- AppState (e.g., `AppState: Arc<RwLock<...>>`) — application-level shared state held via an RwLock. Search for `RwLock` and `app_state` occurrences.

Inventory steps
---------------
1. Search for lock use-sites:
   - `rg "\.lock\(|RwLock|Mutex" VEN/src -n` (ripgrep recommended)
   - Record file, function, and the enclosing logical operation (read vs write).
2. For each lock, record:
   - Lock name and type (Mutex/RwLock)
   - Typical hold duration (short, medium, long)
   - Code paths that require the lock (spawn_* functions, HTTP handlers)

Verification checklist
----------------------
- Add an entry in `specs/018-split-loops-tasks/checklists/refactor.md` referencing this document (CHK005 already points to locking semantics).
- For each migrated file, run targeted unit tests and a small runtime smoke check that exercises the same lock sequences (see quickstart.md incremental BDD subset).
- Code review items: reviewer must confirm that any `ctx.sim.lock()` calls are unchanged in semantics (no new awaits while the lock is held, no moved locking boundaries).

If issues are found
-------------------
- Revert the single-file migration commit for quick rollback (see Quickstart step 2) and open an investigation task.
- If a lock-holding pattern must change, record the change in `docs/history/project_journal.md` and justify it in a follow-up plan (concurrency changes are out-of-scope for Phase 1).

Notes
-----
This file is a living inventory. Update it as each spawn_* function is migrated (T004..T010) to record any subtle changes discovered during testing.
