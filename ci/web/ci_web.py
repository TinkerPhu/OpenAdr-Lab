#!/usr/bin/env python3
"""ci_web - read-only HTTP API and web view over the CI handoff board (ci/board.sh).

Owns: turning the board's plain files (request.env, status, attempt, result.md, run.log) into
JSON, and serving the single-page view next to it. Reads: BOARD_DIR (default /srv/ci/board).
Must never: write to the board, run a ticket, or open a log that is not run/quick/full.log.
The set of finished statuses is the board's own (BOARD_FINISHED in board.sh), read, not copied.

Estimates (`eta`, `expected_s`) are the median duration of finished tickets of the same class
(audits-only / quick / full, told apart by the steps they ran), chained through the queue.

    python3 ci_web.py            # listens on 0.0.0.0:$CI_WEB_PORT (default 8480)
"""
import json
import os
import re
import statistics
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

HERE = Path(__file__).resolve().parent
BOARD_DIR = Path(os.environ.get("BOARD_DIR", "/srv/ci/board"))
BOARD_SH = HERE.parent / "board.sh"
NAME_RE = re.compile(r"^(\d{8}T\d{4}Z)_([A-Za-z0-9]{0,8})_([A-Za-z0-9._-]*)$")
LOGS = ("run", "quick", "full")
OVERDUE_FLOOR_S = 60
# Statuses where a run went through every step it was going to, so its duration is a fair sample.
COMPLETE = ("merged", "deployed", "green")


def _finished_set():
    m = re.search(r'^BOARD_FINISHED="([^"]*)"', BOARD_SH.read_text(), re.M)
    return set(m.group(1).split())


FINISHED = _finished_set()


# -- time ---------------------------------------------------------------------

def parse_iso(s):
    return datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def iso(dt):
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ") if dt else None


def utcnow():
    return datetime.now(timezone.utc)


# -- names, request, result, log ----------------------------------------------

def safe_name(name):
    return name if name and len(name) <= 120 and NAME_RE.match(name) else None


def parse_name(name):
    m = NAME_RE.match(name)
    created = datetime.strptime(m.group(1), "%Y%m%dT%H%MZ").replace(tzinfo=timezone.utc)
    return {"created": iso(created), "session": m.group(2), "slug": m.group(3)}


def read_request(d):
    out = {}
    try:
        for line in (d / "request.env").read_text().splitlines():
            k, _, v = line.partition("=")
            if k:
                out[k] = v
    except OSError:
        pass
    return out


def parse_result(text):
    r = {"verdict": None, "note": None, "finished": None, "steps": [],
         "suites_run": None, "suites_skipped": None}
    m = re.search(r"^- verdict: \*\*(\w+)\*\*(?: \u2014 (.*))?$", text, re.M)
    if m:
        r["verdict"], r["note"] = m.group(1), m.group(2)
    for key, label in (("suites_run", "run"), ("suites_skipped", "skipped")):
        m = re.search(rf"^- {label}: (.*)$", text, re.M)
        if m and m.group(1) != "(none)":
            r[key] = m.group(1).split()
        elif m:
            r[key] = []
    m = re.search(r"^- finished: (\S+)", text, re.M)
    if m:
        r["finished"] = m.group(1)
    for m in re.finditer(r"^\| (.+?) \| (\d+) \| (\d+) \|$", text, re.M):
        r["steps"].append({"name": m.group(1), "exit": int(m.group(2)), "seconds": int(m.group(3))})
    return r


def _log_clock(created, hms):
    """A run.log line carries only HH:MM:SSZ; the date is the ticket's, rolled over past midnight."""
    t = datetime.strptime(hms, "%H:%M:%S").time()
    dt = datetime.combine(created.date(), t, tzinfo=timezone.utc)
    return dt + timedelta(days=1) if dt < created - timedelta(minutes=1) else dt


def parse_run_log(text, created):
    """(start of the latest attempt, current step, when it began) from run.log."""
    start = step = since = None
    for m in re.finditer(r"^\[(\d\d:\d\d:\d\dZ)\] (ticket |=== )(.*)$", text, re.M):
        at = _log_clock(created, m.group(1)[:-1])
        if m.group(2) == "ticket ":
            start, step, since = at, None, None
        else:
            step, since = m.group(3), at
    return start, step, since


def step_class(steps):
    names = [s["name"] for s in steps]
    if any(n.startswith("run_all_tests") for n in names):
        return "full"
    return "quick" if "cargo test" in names else "audits"


def planned_class(request):
    """The class a ticket not yet run will most likely have (the suites it needs are known only once run)."""
    tiers = request.get("TIERS") or "quick,full"
    return "full" if "full" in tiers.split(",") else "quick"


def read_ticket(name):
    d = BOARD_DIR / name
    n = parse_name(name)
    req = read_request(d)
    try:
        status = (d / "status").read_text().strip()
    except OSError:
        status = "unknown"
    try:
        result = parse_result((d / "result.md").read_text())
    except OSError:
        result = parse_result("")
    try:
        log_text = (d / "run.log").read_text()
    except OSError:
        log_text = ""
    try:
        attempt = int((d / "attempt").read_text().strip() or 0)
    except (OSError, ValueError):
        attempt = 0
    created = parse_iso(n["created"])
    start, step, since = parse_run_log(log_text, created)
    finished = parse_iso(result["finished"]) if result["finished"] else None
    running = status == "running"
    return {
        "name": name, "status": status, "branch": req.get("BRANCH"), "sha": req.get("SHA"),
        "kind": req.get("KIND") or "merge", "tiers": req.get("TIERS") or "quick,full",
        "session": n["session"], "created": n["created"], "attempt": attempt,
        "acked": (d / "ack").exists(),
        "started": iso(start), "finished": iso(finished),
        "duration_s": int((finished - start).total_seconds()) if start and finished else None,
        "note": result["note"], "steps": result["steps"],
        "suites_run": result["suites_run"], "suites_skipped": result["suites_skipped"],
        "class": step_class(result["steps"]) if result["steps"] else planned_class(req),
        "current_step": step if running else None,
        "current_step_since": iso(since) if running else None,
        "eta": None, "expected_s": None, "overdue": False,
    }


def read_log(name, which, tail):
    if which not in LOGS or not safe_name(name):
        return None
    try:
        lines = (BOARD_DIR / name / f"{which}.log").read_text(errors="replace").splitlines(keepends=True)
    except OSError:
        return ""
    return "".join(lines[-max(1, min(tail, 5000)):])


# -- estimates ----------------------------------------------------------------

def expected_duration_s(cls, tickets=None):
    """Median seconds of finished, fully-run tickets of this class; None without a sample."""
    tickets = tickets if tickets is not None else [read_ticket(n) for n in board_names()]
    xs = [t["duration_s"] for t in tickets
          if t["status"] in COMPLETE and t["duration_s"] and t["class"] == cls]
    return int(statistics.median(xs)) if xs else None


def add_estimates(tickets, now):
    """Fill eta/expected_s/overdue: running = start + typical; queued = after the one before it."""
    cursor = now
    by_class = {}
    for t in tickets:
        if t["class"] not in by_class:
            by_class[t["class"]] = expected_duration_s(t["class"], tickets)
    ordered = sorted(tickets, key=lambda t: t["name"])
    for t in (x for x in ordered if x["status"] == "running"):
        exp = by_class[t["class"]]
        t["expected_s"] = exp
        if exp and t["started"]:
            eta = parse_iso(t["started"]) + timedelta(seconds=exp)
            if eta <= now:
                t["overdue"] = True
                eta = now + timedelta(seconds=max(OVERDUE_FLOOR_S, exp // 10))
            t["eta"] = iso(eta)
            cursor = max(cursor, eta)
        else:
            cursor = None
    for t in (x for x in ordered if x["status"] == "queued"):
        exp = by_class[t["class"]]
        t["expected_s"] = exp
        if cursor is None or not exp:
            cursor = None
            continue
        cursor = max(cursor, now) + timedelta(seconds=exp)
        t["eta"] = iso(cursor)


# -- board-wide views ---------------------------------------------------------

def board_names():
    try:
        return sorted(p.name for p in BOARD_DIR.iterdir() if p.is_dir() and safe_name(p.name))
    except OSError:
        return []


def list_tickets(limit=100, now=None):
    tickets = [read_ticket(n) for n in board_names()]
    add_estimates(tickets, now or utcnow())
    return sorted(tickets, key=lambda t: t["name"], reverse=True)[:limit]


def _systemctl(unit):
    try:
        out = subprocess.run(["systemctl", "is-active", unit], capture_output=True, text=True, timeout=3)
        return out.stdout.strip() or "unknown"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def summary(now=None):
    now = now or utcnow()
    tickets = list_tickets(limit=10**6, now=now)
    counts = {}
    for t in tickets:
        counts[t["status"]] = counts.get(t["status"], 0) + 1
    running = next((t for t in tickets if t["status"] == "running"), None)
    queued = [t for t in tickets if t["status"] == "queued"]
    etas = [t["eta"] for t in tickets if t["eta"]]
    return {
        "now": iso(now), "counts": counts, "queued": len(queued),
        "running": running["name"] if running else None,
        "queue_done_eta": max(etas) if etas else None,
        "unacked_finished": [t["name"] for t in tickets if t["status"] in FINISHED and not t["acked"]],
        "runner_timer": _systemctl("ci-runner.timer"),
        "expected_s": {c: expected_duration_s(c, tickets) for c in ("audits", "quick", "full")},
    }


# -- HTTP ---------------------------------------------------------------------

class Handler(BaseHTTPRequestHandler):
    server_version = "ci-web"

    def _send(self, code, body, ctype="application/json"):
        data = body.encode() if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", ctype + ("" if ctype == "image/png" else "; charset=utf-8"))
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def _json(self, obj, code=200):
        self._send(code, json.dumps(obj))

    def do_GET(self):  # noqa: N802
        u = urlparse(self.path)
        q = parse_qs(u.query)
        parts = [p for p in u.path.split("/") if p]
        try:
            if not parts:
                return self._send(200, (HERE / "index.html").read_bytes(), "text/html")
            if parts == ["api", "health"]:
                return self._json({"ok": True})
            if parts == ["api", "summary"]:
                return self._json(summary())
            if parts == ["api", "tickets"]:
                return self._json(list_tickets(int(q.get("limit", ["100"])[0])))
            if len(parts) >= 3 and parts[:2] == ["api", "tickets"] and safe_name(parts[2]):
                name = parts[2]
                if not (BOARD_DIR / name).is_dir():
                    return self._json({"error": "no such ticket"}, 404)
                if len(parts) == 3:
                    return self._json(next(t for t in list_tickets(10**6) if t["name"] == name))
                if len(parts) == 5 and parts[3] == "log":
                    text = read_log(name, parts[4], int(q.get("tail", ["200"])[0]))
                    if text is None:
                        return self._json({"error": "unknown log"}, 404)
                    return self._send(200, text, "text/plain")
        except (ValueError, StopIteration):
            return self._json({"error": "bad request"}, 400)
        return self._json({"error": "not found"}, 404)

    def log_message(self, *args):
        pass


def main():
    port = int(os.environ.get("CI_WEB_PORT", "8480"))
    print(f"ci-web: board {BOARD_DIR}, port {port}", flush=True)
    ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()


if __name__ == "__main__":
    sys.exit(main())
