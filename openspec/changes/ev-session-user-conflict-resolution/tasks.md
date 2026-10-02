# Tasks

**Prerequisite: `ev-session-queue-foundation` must be implemented, tested and
merged first.** This change consumes its `EvSessionQueue::insert` /
`EvSessionConflict`; none of the tasks below re-implement overlap detection.

Branch: `050-ev-session-user-conflict-resolution`. Prerequisite:
`049-ev-session-queue-foundation` merged to main. Test-first throughout.

## 1. Request validation and the conflict error

- [ ] 1.1 Write failing unit tests in `VEN/src/controller/user_request.rs` for the new `RequestError` variants — a conflict carrying the clashing sessions, and an empty-window error when `earliest_start` is at or after the deadline; confirm they fail
- [ ] 1.2 Add those `RequestError` variants, carrying the clashing sessions as typed fields (id, window, target) per `docs/guidelines/ERROR_HANDLING.md` and design Decision 2, with a terse one-line `Display`; verify 1.1 passes and `cargo clippy --all-targets --all-features -- -D warnings` is clean
- [ ] 1.3 Add `replace_session_ids: Option<Vec<Uuid>>` to `CreateUserRequestParams` and verify it deserialises as absent on every existing request body (a test over today's EV, heater and shiftable-load payloads)


## 2. The route: refuse, and replace on instruction

- [ ] 2.2 Enrich the foundation's refusal: resolve its conflicting ids to full session detail (window, target) inside the same queue read, so the prompt can name the clashing plan without a second, independently-derived account of it (design Decision 2)
- [ ] 2.3 Write failing tests for the replace path: naming exactly the conflict set succeeds; naming a subset is refused; naming a non-conflicting queued session is refused; naming an id no longer queued is refused — and in every refusal nothing is removed (design Decision 1)
- [ ] 2.4 Implement replace-then-insert inside one `HemsState` write critical section with the checked `insert` still doing the enforcement, so a failing insert commits no removal (design Decision 3); verify 2.3 passes
- [ ] 2.5 Write a failing test that a successful replacement triggers a replan (`PlanTrigger::UserRequest`) exactly once, then verify it passes
- [ ] 2.6 Verify `python scripts/audit_file_sizes.py` still passes — `routes/hems/sessions.rs` is already large, so split the conflict-response construction out if it pushes the file over the cap

## 3. UI: the prompt and the confirm-and-resubmit

- [ ] 3.1 Add the conflict error kind and `replace_session_ids` to the UI API client (`VEN/ui/src/api/`) and verify a unit test asserts the conflict response parses into the clashing sessions
- [ ] 3.2 Write a failing UI test that a refused submission renders a prompt naming the clashing plan and its departure, then implement it; verify `cd VEN/ui && npm test` passes
- [ ] 3.3 Write failing UI tests for the two outcomes — declining leaves the queue untouched and keeps the draft amendable; confirming resubmits with exactly the named ids and the new session appears — then implement and verify
- [ ] 3.4 Verify the prompt lists all clashing plans when a submission conflicts with more than one, and confirming replaces all of them
- [ ] 3.6 Verify `npx eslint` reports zero errors in `VEN/ui`

## 4. Use-case coverage

- [ ] 4.1 Add a BDD scenario to `tests/features/ven_user_request.feature` for the user's own scenario: a standing plan exists, a spontaneous overlapping plan is submitted, the submission is refused naming the standing plan, and nothing is queued or removed
- [ ] 4.2 Add a BDD scenario for the confirmed replacement: resubmitting with the replace instruction removes the standing plan and queues the new one
- [ ] 4.3 Add a BDD scenario asserting a non-overlapping second plan is still accepted without any prompt, so the queue from the foundation change remains usable by hand
- [ ] 4.4 Run `DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` and verify all BDD suites pass

## 5. Verification and close-out

- [ ] 5.1 Run the full gate and verify green: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo audit`, `python scripts/audit_file_sizes.py`, the four VEN Rust pyramid layers, both UI suites
- [ ] 5.2 Verify in the running app (`ssh Node2` stack or local dev server) that submitting an overlapping plan shows the prompt, that declining changes nothing, and that confirming leaves exactly one plan for that window
- [ ] 5.3 Run `DOCKER_HOST=Node2 bash run_all_tests.sh` and verify every suite passes
- [ ] 5.4 Wave the implemented behaviour into current-state docs per `workflow` rule 3: the conflict-resolution flow into the relevant `docs/use-cases/*.md`, mechanism-level facts into `docs/architecture/VEN_ARCHITECTURE.md`, pointing at the test files by name rather than restating scenarios
- [ ] 5.5 Write the journal entry in `docs/history/project_journal.md` and add any durable lesson to `docs/reference/KEY_LEARNINGS.md`
- [ ] 5.6 Delete `openspec/changes/ev-session-user-conflict-resolution/` once implemented and tested
