"""Step definitions for UI-driven scenarios."""

import json
import subprocess
from behave import given, when, then, register_type, use_step_matcher


# -- cleanup --

@given("previous UI test programs are cleaned up")
def step_cleanup_ui_programs(context):
    """Delete any lingering ui-uc* events and programs before each UI scenario.

    Through the API, not SQL. The old raw-SQL version existed because 3.0 scoped
    programs to a business_id, so a program the VTN UI created could be
    invisible to the API token and pile up until it caused 409s. 3.1 has no
    business_id and one `read_all` credential sees everything, so the API can do
    it -- and the SQL could not: it deleted from `ven_program` and
    `report.program_id`, both dropped by the 3.1 migration, so every run logged
    `column "program_id" does not exist` and cleaned nothing.

    Events go first: `event.program_id` has no ON DELETE CASCADE, so a program
    with events cannot be deleted.
    """
    from features.helpers.api_client import vtn_get, vtn_delete, get_token_value
    try:
        token = get_token_value("bl-client", "bl-client")
        events = vtn_get("/events?limit=50", token).json()
        programs = vtn_get("/programs?limit=50", token).json()
        ui_program_ids = {p["id"] for p in programs
                          if (p.get("programName") or "").startswith("ui-uc")}
        for e in events:
            if e.get("programID") in ui_program_ids:
                vtn_delete(f"/events/{e['id']}", token)
        for pid in ui_program_ids:
            vtn_delete(f"/programs/{pid}", token)
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
    context.ui_program_targets = [ven1, ven2]
    context.ui.create_program(name, ven_targets=[ven1, ven2])


@when('I create a UI program "(?P<name>[^"]+)" targeting "(?P<ven>[^"]+)"')
def step_ui_create_program_targeted(context, name, ven):
    context.ui_program_targets = [ven]
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
    # The event inherits the program's targets. 3.1 filters each object by its
    # own targets with no program join, so an untargeted event is public --
    # visible even to a VEN its program does not name, which is what these
    # "targeted to VEN-N only" scenarios assert against.
    context.ui.create_event(
        name=name,
        program_name=prog,
        priority=pri,
        intervals_json=json.dumps(intervals),
        targets_json=json.dumps(getattr(context, "ui_program_targets", [])),
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
    targets = ["ven-2"]
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
