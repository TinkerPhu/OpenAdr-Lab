#!/usr/bin/env python3
"""Recurring checks that no commit triggers.

Some work is due because time passed, not because anything changed: a
dependency audit, an architecture sweep, "has the tooling moved on". A
checklist cannot carry that — `docs/reference/SESSION_START.md` says "do this
~every 3 months" and nothing on earth knows when the last time was.

`jobs.json` knows. This reads it, says what is due, runs what can be run, and
records when it happened.

    python3 scripts/jobs.py due        # what is due now (quiet when nothing is)
    python3 scripts/jobs.py list       # every job and when it is next due
    python3 scripts/jobs.py run        # run every due job that has a command
    python3 scripts/jobs.py run <id>   # run one job whatever its schedule
    python3 scripts/jobs.py done <id>  # record a human checklist item as done

Exit codes for `due`: 0 = nothing due, 1 = something is due. That makes it
usable from a hook that only wants to speak up when there is something to say.
"""

import argparse
import json
import shlex
import shutil
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

JOBS_FILE = Path(__file__).resolve().parent.parent / "jobs.json"
ISO = "%Y-%m-%dT%H:%M:%SZ"


def now_utc() -> datetime:
    return datetime.now(timezone.utc).replace(microsecond=0)


def parse_ts(value: "str | None") -> "datetime | None":
    if not value:
        return None
    return datetime.strptime(value, ISO).replace(tzinfo=timezone.utc)


def fmt_ts(moment: datetime) -> str:
    return moment.strftime(ISO)


def load() -> dict:
    return json.loads(JOBS_FILE.read_text(encoding="utf-8"))


def save(data: dict) -> None:
    """Rewrite the file, refreshing `next_due` from the jobs themselves.

    `next_due` is derived, never edited: a cached answer that can disagree with
    the data it summarises is a second source of truth.
    """
    data["next_due"] = fmt_ts(min(due_at(job) for job in data["jobs"])) if data["jobs"] else None
    JOBS_FILE.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def due_at(job: dict) -> datetime:
    """When this job next comes due.

    A job that has never run is due now, not at some epoch date — the answer to
    "when should this first happen" is "as soon as anyone looks".
    """
    last = parse_ts(job.get("last_run"))
    if last is None:
        return now_utc()
    return last + timedelta(days=job["interval_days"])


def is_due(job: dict, at: datetime) -> bool:
    return due_at(job) <= at


def describe(job: dict, at: datetime) -> str:
    when = due_at(job)
    if when <= at:
        overdue = (at - when).days
        age = "never run" if job.get("last_run") is None else f"{overdue}d overdue"
        return f"  DUE   {job['id']:<26} {job['description']}  ({age})"
    return f"        {job['id']:<26} {job['description']}  (next {fmt_ts(when)[:10]})"


def cmd_list(data: dict) -> int:
    at = now_utc()
    for job in sorted(data["jobs"], key=due_at):
        print(describe(job, at))
    return 0


def cmd_due(data: dict, quiet: bool) -> int:
    at = now_utc()
    due = [job for job in data["jobs"] if is_due(job, at)]
    if not due:
        if not quiet:
            nxt = data.get("next_due")
            print(f"No recurring checks due{f' before {nxt[:10]}' if nxt else ''}.")
        return 0
    print(f"{len(due)} recurring check(s) due — see jobs.json:")
    for job in sorted(due, key=due_at):
        print(describe(job, at))
        if not job.get("command"):
            print("          (no command — this one is yours; `jobs.py done <id>` when handled)")
    print("Run them with: python3 scripts/jobs.py run")
    return 1


def executable_of(command: str) -> str:
    """The program a command line will actually invoke."""
    parts = shlex.split(command, posix=False)
    return parts[0].strip('"') if parts else ""


def missing_executable(command: str) -> "str | None":
    """The program this command needs and cannot find, or None if it is there.

    Checked *before* running rather than inferred from the exit code
    afterwards, because the exit code cannot carry that answer: cmd.exe returns
    1 for "not recognized", exactly what a real finding returns. The first
    version of this script trusted the exit code and recorded
    `finding (exit 1)` when `python3` was missing on Windows — a check that had
    never run, filed as a result.
    """
    program = executable_of(command)
    if not program or Path(program).exists() or shutil.which(program):
        return None
    return program


def run_one(job: dict) -> bool:
    """Run a job's command, reporting its own output. True when it succeeded."""
    print(f"\n=== {job['id']}: {job['description']}")
    if not job.get("command"):
        print("  no command — nothing to run automatically")
        return False
    # `{python}` rather than a hard-coded `python3`: the interpreter running
    # this script is by definition one that exists here, and the same jobs.json
    # has to work on Windows and on the lab hosts.
    command = job["command"].replace("{python}", f'"{sys.executable}"')

    absent = missing_executable(command)
    if absent:
        job["last_run"] = fmt_ts(now_utc())
        job["last_status"] = f"could not run ({absent} not found)"
        print(f"  -> {job['last_status']}")
        return False

    result = subprocess.run(command, shell=True, cwd=str(JOBS_FILE.parent))

    job["last_run"] = fmt_ts(now_utc())
    if result.returncode == 0:
        job["last_status"] = "ok"
    else:
        # Not a failure to check: the openspec job exits non-zero precisely
        # when it has something to tell you.
        job["last_status"] = f"finding (exit {result.returncode})"
    print(f"  -> {job['last_status']}")
    return result.returncode == 0


def cmd_run(data: dict, job_id: "str | None") -> int:
    at = now_utc()
    if job_id:
        targets = [job for job in data["jobs"] if job["id"] == job_id]
        if not targets:
            print(f"No job with id {job_id!r} in jobs.json", file=sys.stderr)
            return 2
    else:
        targets = [job for job in data["jobs"] if is_due(job, at) and job.get("command")]
        if not targets:
            print("Nothing due that can be run automatically.")
            return 0

    for job in targets:
        run_one(job)
    save(data)
    return 0


def cmd_done(data: dict, job_id: str) -> int:
    for job in data["jobs"]:
        if job["id"] == job_id:
            job["last_run"] = fmt_ts(now_utc())
            job["last_status"] = "done by hand"
            save(data)
            print(f"{job_id}: recorded as done, next due {fmt_ts(due_at(job))[:10]}")
            return 0
    print(f"No job with id {job_id!r} in jobs.json", file=sys.stderr)
    return 2


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=["due", "list", "run", "done"])
    parser.add_argument("job_id", nargs="?")
    parser.add_argument("--quiet", action="store_true",
                        help="say nothing when nothing is due (for hooks)")
    args = parser.parse_args()

    if not JOBS_FILE.exists():
        print(f"No {JOBS_FILE.name} found", file=sys.stderr)
        return 2
    data = load()

    if args.action == "list":
        return cmd_list(data)
    if args.action == "due":
        return cmd_due(data, args.quiet)
    if args.action == "run":
        return cmd_run(data, args.job_id)
    if args.action == "done":
        if not args.job_id:
            print("done needs a job id", file=sys.stderr)
            return 2
        return cmd_done(data, args.job_id)
    return 2


if __name__ == "__main__":
    sys.exit(main())
