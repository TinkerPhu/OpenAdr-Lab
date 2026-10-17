# ci/ - the CI/CD of OpenADR Lab

Everything that tests, gates, merges and deploys lives here. None of it is needed to build or
run the lab: a fresh clone runs its suites on the local docker (`bash run_all_tests.sh`).
Node1/Node2/Node3 are one lab's deployment, named only through environment variables.

| Path | What it is |
|---|---|
| `runner.sh`, `board.sh` | the runner (rebase, quick tier, full tier, fast-forward main) and the handoff board it works through |
| `ci-handoff.sh`, `ci-next.sh` | a session hands a branch over / picks the verdict up (`CI_HOST` required) |
| `deploy.sh` | build release images of `origin/main` and switch a production host over |
| `install.sh`, `systemd/` | install the runner timer and the web board on the CI host |
| `web/` | read-only JSON API and single-page view of the board (`ci_web.py`, stdlib only) |
| `locks/` | lease locks that serialise a docker host (`docker_host_lock.sh`) or WSL (`wsl_lock.sh`) |
| `audits/` | the merge gates: file sizes, duplication ratchet, debt gate, VEN architecture, step definitions |
| `test_*.sh`, `test_web.py` | tests of the CI mechanism itself |

Full flow, tickets and statuses: `docs/guidelines/TESTING.md` ("CI on Node3").

## On one machine, no lab hosts

```bash
bash run_all_tests.sh                      # suites on this machine's docker
python3 ci/audits/audit_file_sizes.py      # any gate, from the repo root
bash ci/test_board.sh                      # the CI mechanism's own tests
BOARD_DIR=/tmp/board python3 ci/web/ci_web.py   # the web board on http://localhost:8480
```

Nothing here needs a second host. Using one is opt-in:

| Variable | Meaning | Unset |
|---|---|---|
| `OPENADR_LAB_HOST` / `DOCKER_HOST` | ssh name of the docker host the suites and lock use | this machine |
| `CI_HOST` | ssh name of the CI host `ci-handoff.sh` / `ci-next.sh` talk to | the scripts refuse to run |
| `BOARD_DIR` | the board directory (`/srv/ci/board` on the CI host) | `/srv/ci/board` |
| `CI_WEB_PORT` | port of the web board | `8480` |

## CI host

On a machine with docker, git, python3 and an ssh key that may push to `origin/main`:
`bash ci/install.sh` (idempotent). It copies this directory to `/srv/ci/bin/ci/`, so a ticket that
changes the runner is tested by the previous version, enables the runner timer and `ci-web.service`.

## The web board

`ci/web/ci_web.py` serves, on port 8480:

| Route | Returns |
|---|---|
| `/` | the view: summary, timeline (solid = run, dashed = estimate), ticket table, step bars, log tail |
| `/api/summary` | counts, running ticket, queue length, estimated time the queue is empty, awaiting pickup |
| `/api/tickets?limit=N` | tickets newest first with status, timings, steps, `eta`, `expected_s`, `overdue` |
| `/api/tickets/<name>` | one ticket |
| `/api/tickets/<name>/log/<run\|quick\|full>?tail=N` | log tail (text) |

Estimates are the median duration of finished tickets of the same class (audits, quick, full),
chained through the queue. The board is read-only and has no authentication: keep it on the LAN.
