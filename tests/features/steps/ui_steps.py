"""Step definitions for UI-driven scenarios."""

import json
import subprocess
from behave import given, when, then, register_type, use_step_matcher


# -- cleanup --

@given("previous UI test programs are cleaned up")
def step_cleanup_ui_programs(context):
    """Delete any lingering ui-uc* programs before each UI UC scenario.

    Belt-and-suspenders: _cleanup_all_programs() in before_feature handles
    bulk cleanup, but if the VTN UI created programs under a non-null business_id
    that is invisible to the any-business API token, they accumulate and cause
    409 Conflict errors on the next run.  This step runs a targeted SQL delete
    before each scenario so each scenario always starts with a clean slate.
    """
    dsn = "postgres://openadr:openadr@test-db:5432/openadr"
    sql = (
        "DELETE FROM report"
        "  WHERE program_id IN (SELECT id FROM program WHERE program_name LIKE 'ui-uc%');"
        "DELETE FROM event"
        "  WHERE program_id IN (SELECT id FROM program WHERE program_name LIKE 'ui-uc%');"
        "DELETE FROM ven_program"
        "  WHERE program_id IN (SELECT id FROM program WHERE program_name LIKE 'ui-uc%');"
        "DELETE FROM program WHERE program_name LIKE 'ui-uc%';"
    )
    try:
        result = subprocess.run(
            ["psql", dsn, "-c", sql],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode != 0 and result.stderr.strip():
            print(f"[ui-cleanup] SQL warning: {result.stderr[:200]}")
    except Exception as exc:
        print(f"[ui-cleanup] cleanup skipped: {exc}")


# -- navigation --

@given("I open the VTN UI")
def step_open_ui(context):
    # Already opened in before_scenario via context.ui.open()
    pass


@when("I navigate to the Programs page")
def step_nav_programs(context):
    context.ui.go_programs()


@when("I navigate to the Events page")
def step_nav_events(context):
    context.ui.go_events()


@when("I navigate to the Reports page")
def step_nav_reports(context):
    context.ui.go_reports()


# -- program creation via UI (use regex matcher to avoid ambiguity) --

use_step_matcher("re")


@when('I create a UI program "(?P<name>[^"]+)" targeting both "(?P<ven1>[^"]+)" and "(?P<ven2>[^"]+)"')
def step_ui_create_program_dual(context, name, ven1, ven2):
    context.ui.create_program(name, ven_targets=[ven1, ven2])


@when('I create a UI program "(?P<name>[^"]+)" targeting "(?P<ven>[^"]+)"')
def step_ui_create_program_targeted(context, name, ven):
    context.ui.create_program(name, ven_targets=[ven])


use_step_matcher("parse")


@when('I create an open program "{name}" via the UI')
def step_ui_create_open_program(context, name):
    # No VEN checkboxes checked = open program
    context.ui.create_program(name)


# -- event creation via UI --

@when('I create a UI event "{name}" for program "{prog}" with type "{ptype}" priority {pri:d} and {count:d} interval')
@when('I create a UI event "{name}" for program "{prog}" with type "{ptype}" priority {pri:d} and {count:d} intervals')
def step_ui_create_event(context, name, prog, ptype, pri, count):
    from features.steps.use_case_steps import _build_intervals
    intervals = _build_intervals(ptype, count)
    context.ui.create_event(
        name=name,
        program_name=prog,
        priority=pri,
        intervals_json=json.dumps(intervals),
    )


@when('I create a UI event "{name}" for program "{prog}" with type "{ptype}" priority {pri:d} and {count:d} interval with intervalPeriod')
def step_ui_create_event_with_ip(context, name, prog, ptype, pri, count):
    from features.steps.use_case_steps import _build_intervals
    intervals = _build_intervals(ptype, count)
    from datetime import datetime, timezone
    context.ui.create_event(
        name=name,
        program_name=prog,
        priority=pri,
        start=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        duration="PT4H",
        intervals_json=json.dumps(intervals),
    )


@when('I create a UI event "{name}" for program "{prog}" with type "{ptype}" priority {pri:d} and {count:d} interval with targets')
def step_ui_create_event_with_targets(context, name, prog, ptype, pri, count):
    from features.steps.use_case_steps import _build_intervals
    intervals = _build_intervals(ptype, count)
    targets = [{"type": "VEN_NAME", "values": ["ven-2"]}]
    context.ui.create_event(
        name=name,
        program_name=prog,
        priority=pri,
        intervals_json=json.dumps(intervals),
        targets_json=json.dumps(targets),
    )


# -- event deletion via UI --

@when('I delete event "{name}" via the UI')
def step_ui_delete_event(context, name):
    context.ui.delete_event_by_name(name)


# -- UI verification steps --

@then('the program "{name}" appears in the UI programs list')
def step_ui_program_visible(context, name):
    assert context.ui.program_visible(name), f"Program '{name}' not visible in UI"


@then('the event "{name}" appears in the UI events table')
def step_ui_event_visible(context, name):
    assert context.ui.event_visible(name), f"Event '{name}' not visible in UI"


@then('the event "{name}" is gone from the UI events table')
def step_ui_event_not_visible(context, name):
    assert context.ui.event_not_visible(name), f"Event '{name}' still visible in UI"


@then('the report from "{client}" appears in the UI reports table')
def step_ui_report_visible(context, client):
    assert context.ui.report_visible(client), f"Report from '{client}' not visible in UI"
