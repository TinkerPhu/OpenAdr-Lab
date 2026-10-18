"""Does the CI board web API (ci/web/ci_web.py) read the board's files correctly?

Runs the real module against a throwaway board directory; no server, no network.
Usage:  python3 ci/test_web.py   (or bash ci/test_web.sh)
"""
import os
import re
import sys
import tempfile
import time
import json
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "web"))
import ci_web  # noqa: E402

RESULT = """# CI result: T

- verdict: **red** — cargo test failed
- branch: fix/x   requested: aaaa   tested: bbbb
- tiers: quick,full   kind: merge   attempt: 1
- finished: 2026-10-17T07:10:00Z

## Suites

- run: cargo e2e
- skipped: resilience rust-vtn
- decided by:
  - VEN/src/a.rs -> cargo e2e resilience

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

    def test_suites_run_and_skipped_are_read(self):
        r = ci_web.parse_result(RESULT)
        self.assertEqual(["cargo", "e2e"], r["suites_run"])
        self.assertEqual(["resilience", "rust-vtn"], r["suites_skipped"])

    def test_no_suites_block_means_unknown_and_none_means_empty(self):
        self.assertIsNone(ci_web.parse_result("# x\n")["suites_run"])
        r = ci_web.parse_result("- run: (none)\n- skipped: cargo e2e\n")
        self.assertEqual([], r["suites_run"])

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


class Clearing(unittest.TestCase):
    """Archiving finished tickets: the sweep, the token-guarded POST routes."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.board = Path(self.tmp.name) / "board"
        self.archive = Path(self.tmp.name) / "archive"
        self.board.mkdir()
        ci_web.BOARD_DIR, ci_web.ARCHIVE_DIR = self.board, self.archive
        ci_web.TOKEN, ci_web.KEEP_MERGED_H = "s3cret", 24.0
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), ci_web.Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"

    def post(self, path, token="s3cret"):
        req = urllib.request.Request(self.base + path, method="POST", data=b"{}")
        if token is not None:
            req.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(req) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read())

    def test_clear_moves_a_finished_ticket_to_the_archive(self):
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "red", result=RESULT)
        code, body = self.post(f"/api/tickets/{name}/clear")
        self.assertEqual(200, code)
        self.assertFalse((self.board / name).exists())
        self.assertTrue((self.archive / name / "result.md").exists())

    def test_running_and_queued_tickets_cannot_be_cleared(self):
        for status, name in (("running", "20261017T0700Z_aaaaaaaa_a"), ("queued", "20261017T0701Z_bbbbbbbb_b")):
            make_ticket(self.board, name, status)
            self.assertEqual(409, self.post(f"/api/tickets/{name}/clear")[0])
            self.assertTrue((self.board / name).exists())

    def test_wrong_or_missing_token_is_refused(self):
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "red")
        self.assertEqual(401, self.post(f"/api/tickets/{name}/clear", token="nope")[0])
        self.assertEqual(401, self.post(f"/api/tickets/{name}/clear", token=None)[0])
        self.assertTrue((self.board / name).exists())

    def test_clearing_is_off_without_a_configured_token(self):
        ci_web.TOKEN = ""
        name = "20261017T0700Z_09fb9c63_fix-x"
        make_ticket(self.board, name, "red")
        code, body = self.post(f"/api/tickets/{name}/clear", token="")
        self.assertEqual(403, code)
        self.assertIn("CI_WEB_TOKEN", body["error"])
        self.assertTrue((self.board / name).exists())

    def test_unknown_and_malformed_names(self):
        self.assertEqual(404, self.post("/api/tickets/20261017T0700Z_09fb9c63_nope/clear")[0])
        self.assertEqual(404, self.post("/api/tickets/..%2Fx/clear")[0])

    def test_clear_finished_leaves_open_work(self):
        make_ticket(self.board, "20261017T0600Z_aaaaaaaa_a", "red")
        make_ticket(self.board, "20261017T0601Z_bbbbbbbb_b", "merged")
        make_ticket(self.board, "20261017T0602Z_cccccccc_c", "running")
        make_ticket(self.board, "20261017T0603Z_dddddddd_d", "queued")
        code, body = self.post("/api/clear-finished")
        self.assertEqual(200, code)
        self.assertEqual(2, body["cleared"])
        self.assertEqual(["20261017T0602Z_cccccccc_c", "20261017T0603Z_dddddddd_d"],
                         sorted(p.name for p in self.board.iterdir()))

    def test_summary_tells_the_page_whether_clearing_is_on(self):
        self.assertTrue(ci_web.summary()["clear_enabled"])
        ci_web.TOKEN = ""
        self.assertFalse(ci_web.summary()["clear_enabled"])

    def test_sweep_archives_old_merged_but_keeps_unmerged_and_recent(self):
        now = ci_web.parse_iso("2026-10-18T12:00:00Z")
        for name, status in (("20261017T0700Z_aaaaaaaa_a", "merged"),   # 29h old
                             ("20261017T0701Z_bbbbbbbb_b", "deployed"),  # 29h old
                             ("20261017T0702Z_cccccccc_c", "red"),       # old, not merged
                             ("20261018T0900Z_dddddddd_d", "merged")):   # 3h old
            make_ticket(self.board, name, status)
        moved = ci_web.sweep(now)
        self.assertEqual(2, moved)
        self.assertEqual(["20261017T0702Z_cccccccc_c", "20261018T0900Z_dddddddd_d"],
                         sorted(p.name for p in self.board.iterdir()))

    def test_sweep_can_be_switched_off(self):
        ci_web.KEEP_MERGED_H = 0
        make_ticket(self.board, "20261017T0700Z_aaaaaaaa_a", "merged")
        self.assertEqual(0, ci_web.sweep(ci_web.parse_iso("2026-10-18T12:00:00Z")))


if __name__ == "__main__":
    unittest.main(verbosity=1)
