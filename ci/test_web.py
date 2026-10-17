"""Does the CI board web API (ci/web/ci_web.py) read the board's files correctly?

Runs the real module against a throwaway board directory; no server, no network.
Usage:  python3 ci/test_web.py   (or bash ci/test_web.sh)
"""
import os
import re
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "web"))
import ci_web  # noqa: E402

RESULT = """# CI result: T

- verdict: **red** — cargo test failed
- branch: fix/x   requested: aaaa   tested: bbbb
- tiers: quick,full   kind: merge   docs-only: no   attempt: 1
- finished: 2026-10-17T07:10:00Z

| step | exit | seconds |
|---|---|---|
| audit_file_sizes | 0 | 1 |
| cargo test | 101 | 124 |

## Failed: cargo test
"""
SHA = "0123456789abcdef0123456789abcdef01234567"


def make_ticket(board, name, status, extra=None, result=None, run_log=None):
    d = board / name
    d.mkdir(parents=True)
    (d / "request.env").write_text(
        f"SESSION_ID=09fb9c63-1\nBRANCH=fix/x\nSHA={SHA}\nTIERS=quick,full\nKIND=merge\n"
    )
    (d / "status").write_text(status + "\n")
    (d / "attempt").write_text("1\n")
    if result:
        (d / "result.md").write_text(result)
    if run_log:
        (d / "run.log").write_text(run_log)
    for k, v in (extra or {}).items():
        (d / k).write_text(v)
    return d


class WebApi(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.board = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        ci_web.BOARD_DIR = self.board

    def test_finished_statuses_are_the_boards_own(self):
        text = (HERE / "board.sh").read_text()
        want = set(re.search(r'^BOARD_FINISHED="([^"]*)"', text, re.M).group(1).split())
        self.assertEqual(want, ci_web.FINISHED)

    def test_name_is_parsed(self):
        t = ci_web.parse_name("20261017T0413Z_09fb9c63_051-opportunistic-objective")
        self.assertEqual("2026-10-17T04:13:00Z", t["created"])
        self.assertEqual("09fb9c63", t["session"])

    def test_bad_names_are_rejected(self):
        for bad in ("..", "../etc", "a/b", "", "x" * 300):
            self.assertIsNone(ci_web.safe_name(bad))

    def test_result_steps_and_finish_are_read(self):
        r = ci_web.parse_result(RESULT)
        self.assertEqual("red", r["verdict"])
        self.assertEqual("cargo test failed", r["note"])
        self.assertEqual("2026-10-17T07:10:00Z", r["finished"])
        self.assertEqual(
            [{"name": "audit_file_sizes", "exit": 0, "seconds": 1},
             {"name": "cargo test", "exit": 101, "seconds": 124}],
            r["steps"],
        )

    def test_finished_ticket_has_duration(self):
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "red", result=RESULT,
                    run_log="[07:01:00Z] ticket T (attempt 1)\n[07:10:00Z] verdict: red\n")
        t = ci_web.read_ticket(name)
        self.assertEqual("red", t["status"])
        self.assertEqual("fix/x", t["branch"])
        self.assertEqual(540, t["duration_s"])  # 07:01:00 -> 07:10:00
        self.assertFalse(t["acked"])

    def test_running_ticket_shows_current_step(self):
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "running",
                    run_log="[07:01:00Z] ticket T (attempt 1)\n[07:02:00Z] === cargo fmt\n[07:03:30Z] === cargo clippy\n")
        t = ci_web.read_ticket(name)
        self.assertEqual("cargo clippy", t["current_step"])
        self.assertEqual("2026-10-17T07:03:30Z", t["current_step_since"])

    def test_summary_counts_and_queue(self):
        make_ticket(self.board, "20261017T0600Z_aaaaaaaa_a", "merged", result=RESULT)
        make_ticket(self.board, "20261017T0700Z_bbbbbbbb_b", "queued")
        make_ticket(self.board, "20261017T0710Z_cccccccc_c", "running")
        s = ci_web.summary()
        self.assertEqual({"merged": 1, "queued": 1, "running": 1}, s["counts"])
        self.assertEqual(1, s["queued"])
        self.assertEqual("20261017T0710Z_cccccccc_c", s["running"])

    def test_log_tail_is_limited_and_whitelisted(self):
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "red", extra={"quick.log": "\n".join(str(i) for i in range(100)) + "\n"})
        self.assertEqual("98\n99\n", ci_web.read_log(name, "quick", 2))
        self.assertIsNone(ci_web.read_log(name, "request.env", 2))

    def test_tickets_newest_first(self):
        make_ticket(self.board, "20261017T0600Z_aaaaaaaa_a", "merged")
        make_ticket(self.board, "20261017T0700Z_bbbbbbbb_b", "queued")
        names = [t["name"] for t in ci_web.list_tickets(limit=10)]
        self.assertEqual(["20261017T0700Z_bbbbbbbb_b", "20261017T0600Z_aaaaaaaa_a"], names)

    def _full_run(self, name, minutes):
        """A finished full-tier ticket that took `minutes` (run.log start, result.md finish)."""
        start = ci_web.parse_name(name)["created"]
        hh, mm = int(start[11:13]), int(start[14:16])
        end = hh * 60 + mm + minutes
        res = RESULT.replace("2026-10-17T07:10:00Z", f"2026-10-17T{end // 60:02d}:{end % 60:02d}:00Z").replace(
            "| cargo test | 101 | 124 |", "| run_all_tests.sh --rust --e2e --resilience | 0 | 3000 |")
        make_ticket(self.board, name, "merged", result=res,
                    run_log=f"[{hh:02d}:{mm:02d}:00Z] ticket T (attempt 1)\n")

    def test_expected_duration_is_median_of_same_class(self):
        self._full_run("20261017T0100Z_aaaaaaaa_a", 50)
        self._full_run("20261017T0200Z_bbbbbbbb_b", 60)
        self._full_run("20261017T0300Z_cccccccc_c", 100)
        self.assertEqual(60 * 60, ci_web.expected_duration_s("full"))
        self.assertIsNone(ci_web.expected_duration_s("audits"))

    def test_eta_running_and_queue_chain(self):
        self._full_run("20261017T0100Z_aaaaaaaa_a", 60)
        make_ticket(self.board, "20261017T0600Z_bbbbbbbb_b", "running",
                    run_log="[06:01:00Z] ticket T (attempt 1)\n")
        make_ticket(self.board, "20261017T0610Z_cccccccc_c", "queued")
        make_ticket(self.board, "20261017T0620Z_dddddddd_d", "queued")
        now = ci_web.parse_iso("2026-10-17T06:30:00Z")
        by = {t["name"][-1]: t for t in ci_web.list_tickets(limit=10, now=now)}
        self.assertEqual("2026-10-17T07:01:00Z", by["b"]["eta"])   # start + 60 min
        self.assertEqual("2026-10-17T08:01:00Z", by["c"]["eta"])   # after b, + 60 min
        self.assertEqual("2026-10-17T09:01:00Z", by["d"]["eta"])
        self.assertIsNone(by["a"]["eta"])                          # finished: no estimate

    def test_overdue_running_ticket_gets_a_floor_not_a_past_eta(self):
        self._full_run("20261017T0100Z_aaaaaaaa_a", 60)
        make_ticket(self.board, "20261017T0600Z_bbbbbbbb_b", "running",
                    run_log="[06:01:00Z] ticket T (attempt 1)\n")
        now = ci_web.parse_iso("2026-10-17T08:00:00Z")
        t = ci_web.list_tickets(limit=10, now=now)[0]
        self.assertTrue(t["overdue"])
        self.assertGreater(ci_web.parse_iso(t["eta"]), now)


if __name__ == "__main__":
    unittest.main(verbosity=1)
