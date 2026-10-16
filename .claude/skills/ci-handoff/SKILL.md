---
name: ci-handoff
description: Hand a finished branch to the Node3 CI runner (tests it on the real stack and merges to main if green) and pick the result up later. Use when work on a branch is done and ready to merge, or when the user brings a CI result ("ci-next", "the CI came back").
---

# Skill: hand a branch to the Node3 CI runner

The runner (Node3) rebases your commit onto `origin/main`, runs the audits, `cargo fmt/clippy/test/audit`
and the full suite (`--rust --e2e --resilience`), and fast-forwards `main` when all of it is green.
You never merge to main yourself while it runs. Reference: `docs/guidelines/TESTING.md`, "CI on Node3".

## Handing off

Check first that the runner is alive: `ssh Node3 systemctl is-active ci-runner.timer` (`active`).
If it is not, say so and merge the old way (manual gate, own docker host run).

1. Quick local gate on the branch: the tests you touched, `cargo check`, the python audits
   (`audit_file_sizes`, `audit_duplication`, `audit_debt_gate`, `audit_ven_architecture`,
   `audit_step_definitions` when `tests/features` changed). The runner repeats them; this only
   spares an hour-long round trip for a typo.
2. Everything committed, **including the project journal and KEY_LEARNINGS entries** and any
   `Debt-deferred:` line the debt gate needs: they are tested and merged together with the code.
3. `git push -f origin HEAD:<branch>`.
4. `bash scripts/ci/ci-handoff.sh` (`--kind check` to test without merging).
5. Tell the user the ticket name and stop. Do not wait, poll or start the suite yourself. The user
   resumes you with `scripts/ci/ci-next.sh` when the result is there (a full run takes about an hour,
   queued tickets longer).

## When the result arrives

Your first message is the runner's `result.md`.

- **merged**: `origin/main` now holds the tested commit (the rebased one: its sha may differ from
  yours). Delete the branch (local and `git push origin --delete <branch>`), remove the worktree
  (`git worktree remove`, `git worktree prune`), return any docker host checkout you used to main,
  release locks you hold, report issues filed/fixed/deferred.
- **red**: the failing step and the failing scenarios or log tail are in `result.md`; full logs
  on Node3 under `/srv/ci/board/<ticket>/`. Fix the cause (not the test, unless the test is wrong
  and you explain it to the user), push, hand off again.
- **conflict**: the rebase onto `origin/main` conflicts. `git fetch && git rebase origin/main`,
  resolve, push, hand off again. (The project journal merges by union and never conflicts.)
- **stale**: `main` moved under the run three times; hand off again.
- **error**: read `result.md`; usually the commit was not pushed or the key was refused.
- **green**: tested but not merged (check-only hand-off): report it.

## Deploying

Only when the user asks: `bash scripts/ci/ci-handoff.sh --deploy <Node1|Node2> <VEN|VTN|VEN/scale_out/node2> "<services>"`
(`--rollback` returns to the previous images). It ships `origin/main`, so the change must have merged
first. The result status is `deployed`; verify the service is healthy and tell the user.

## Notes

- Start no build or test run that is not needed (docs/guidelines/TESTING.md, "Rules for runs"):
  nothing twice for the same commit, nothing while a run is in flight, warm caches first.

- One ticket at a time is tested; others queue oldest first. `bash scripts/ci/ci-next.sh --all` shows the board.
- Do not ask the runner to test an unpushed commit; `ci-handoff.sh` refuses it.
- Docs-only changes take the short path (audits only) and merge in minutes.
