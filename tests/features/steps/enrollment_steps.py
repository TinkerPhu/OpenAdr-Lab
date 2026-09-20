import requests
from behave import when, then
from features.helpers.api_client import vtn_post, VEN_BASE_URL, VEN2_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until


def _ven_programs(ven_url):
    return requests.get(f"{ven_url}/programs", timeout=HTTP_TIMEOUT).json()


@when('I create an open program named "{name}"')
def step_create_open_program(context, name):
    context.response = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "targets": None},
    )
    context.response.raise_for_status()


@when('I create a program named "{name}" targeting "{ven_name}"')
def step_create_targeted_program(context, name, ven_name):
    context.response = vtn_post(
        "/programs",
        context.vtn_token,
        json={
            "programName": name,
            "targets": [ven_name],
        },
    )
    context.response.raise_for_status()


@when('I wait for VEN-1 to show program "{name}"')
def step_wait_ven1_program(context, name):
    context.ven1_programs = poll_until(
        lambda: _ven_programs(VEN_BASE_URL),
        lambda progs: any(p.get("programName") == name for p in progs),
        timeout=60,
        description=f"VEN-1 shows program '{name}'",
    )


@when('I wait for VEN-2 to show program "{name}"')
def step_wait_ven2_program(context, name):
    context.ven2_programs = poll_until(
        lambda: _ven_programs(VEN2_BASE_URL),
        lambda progs: any(p.get("programName") == name for p in progs),
        timeout=60,
        description=f"VEN-2 shows program '{name}'",
    )


@then('VEN-1 has program "{name}"')
def step_ven1_has_program(context, name):
    progs = _ven_programs(VEN_BASE_URL)
    names = [p.get("programName") for p in progs]
    assert name in names, f"'{name}' not in VEN-1 programs: {names}"


@then('VEN-2 does not have program "{name}"')
def step_ven2_not_have_program(context, name):
    progs = _ven_programs(VEN2_BASE_URL)
    names = [p.get("programName") for p in progs]
    assert name not in names, f"'{name}' unexpectedly found in VEN-2 programs: {names}"


@then('VEN-2 has program "{name}"')
def step_ven2_has_program(context, name):
    progs = _ven_programs(VEN2_BASE_URL)
    names = [p.get("programName") for p in progs]
    assert name in names, f"'{name}' not in VEN-2 programs: {names}"


@when('I create a program named "{name}" targeting "{ven_a}" and "{ven_b}"')
def step_create_two_target_program(context, name, ven_a, ven_b):
    context.response = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "targets": [ven_a, ven_b]},
    )
    context.response.raise_for_status()
    context.shared_program_name = name


def _targets_seen_as(context, user):
    """The target list of `context.shared_program_name` as `user` sees it."""
    from features.helpers.api_client import get_token_value, vtn_get

    token = get_token_value(user, user)
    programs = vtn_get("/programs", token).json()
    match = [
        p for p in programs if p.get("programName") == context.shared_program_name
    ]
    assert match, (
        f"{user} cannot see program {context.shared_program_name!r} at all — "
        "target filtering, not target hiding, is what failed"
    )
    return sorted(match[0].get("targets") or [])


@then('reading it as "{user}" shows targets exactly "{expected}"')
def step_targets_exactly(context, user, expected):
    seen = _targets_seen_as(context, user)
    assert seen == [expected], (
        f"{user} sees targets {seen} — it must see only itself, or it learns "
        "which other VENs this object addresses"
    )


@then('reading it as "{user}" shows targets "{a}" and "{b}"')
def step_targets_both(context, user, a, b):
    seen = _targets_seen_as(context, user)
    assert seen == sorted([a, b]), (
        f"{user} holds read_all and should see the unredacted list, got {seen}"
    )
