"""What a scenario changed on a VEN, and how to put it back (GB-35).

The one place that knows how to undo a scenario. `api_client`'s mutating VEN helpers report every
call here before sending it (`before_mutation`); `environment.after_scenario` calls `undo()`.

Every VEN route a step mutates is declared below, either with how to undo it (`RESETS`) or with why
nothing needs undoing (`NO_RESET`). A mutation matching neither is printed as
`[cleanup] no reset for ...`, so a new step cannot add state that leaks silently. Nothing here
swallows a failure: what cannot be undone is printed as `[cleanup] LEAK: ...`.

Physical state a scenario only caused (a heater's temperature, an EV's SoC drift) is deliberately
not forced back: writing a temperature under a running heater would itself latch it (R-94). Only
values a scenario set through an endpoint are restored.
"""

import re

import requests

HTTP_TIMEOUT = 60

# Path pattern -> the reset family that undoes it.
RESETS = [
    (re.compile(r"^/user-requests(/|$)"), "user_requests"),
    (re.compile(r"^/sim/inject(/|$)"), "injects"),
    (re.compile(r"^/sim/reset/[^/]+$"), "soc"),
    (re.compile(r"^/sim/config/battery$"), "battery_config"),
    (re.compile(r"^/assets/[^/]+/comfort_curve$"), "comfort"),
    (re.compile(r"^/baseline-override$"), "baseline_override"),
    (re.compile(r"^/arbiter-settings$"), "arbiter"),
]

# Path pattern -> why a mutation there needs no reset.
NO_RESET = [
    (re.compile(r"^/sensors$"), "overwritten by the next measurement tick"),
    (re.compile(r"^/reports$"), "lands on the VTN; the per-feature VTN cleanup owns it"),
    (re.compile(r"^/plan/trigger$"), "a replan leaves no setting behind"),
]

# Reachable from the VEN UI too, which bypasses api_client: always reset on the default VEN.
ALWAYS_ON_DEFAULT_VEN = ("user_requests", "injects", "arbiter", "baseline_override", "comfort")

# Families whose undo needs the value from before the scenario's first change.
NEEDS_SNAPSHOT = ("soc", "battery_config")

ARBITER_DEFAULTS = {"deviation_arbiter_enabled": False, "limit_enforcement_enabled": True}

_touched = {}  # base_url -> {family: [paths]}
_snapshots = {}  # base_url -> GET /sim assets, taken before the first change that needs it


def begin():
    """Forget the previous scenario."""
    _touched.clear()
    _snapshots.clear()


def family_of(path):
    """The reset family a mutated path belongs to; None for a declared no-reset path; raises
    LookupError for a path declared nowhere."""
    path = path.split("?", 1)[0]
    for pattern, family in RESETS:
        if pattern.match(path):
            return family
    for pattern, _why in NO_RESET:
        if pattern.match(path):
            return None
    raise LookupError(path)


def before_mutation(base_url, method, path):
    """Record that `method path` is about to change `base_url`'s state."""
    try:
        family = family_of(path)
    except LookupError:
        print(f"[cleanup] no reset for {method} {base_url}{path}: declare it in scenario_state.py")
        return
    if family is None:
        return
    if family in NEEDS_SNAPSHOT and base_url not in _snapshots:
        try:
            _snapshots[base_url] = _get(base_url, "/sim").get("assets", {})
        except requests.RequestException as exc:
            print(f"[cleanup] snapshot before {method} {base_url}{path} failed: {exc}")
    _touched.setdefault(base_url, {}).setdefault(family, []).append(path)


def undo(default_base_url):
    """Put every VEN this scenario touched back as it was. The default VEN always gets the
    UI-reachable resets."""
    families = dict(_touched)
    default = families.setdefault(default_base_url, {})
    for family in ALWAYS_ON_DEFAULT_VEN:
        default.setdefault(family, [])
    for base_url, by_family in families.items():
        for family, undo_family in UNDO:
            if family in by_family:
                try:
                    undo_family(base_url, by_family[family])
                except requests.RequestException as exc:
                    print(f"[cleanup] {family} reset failed on {base_url}: {exc}")
    begin()


def _get(base_url, path):
    r = requests.get(f"{base_url}{path}", timeout=HTTP_TIMEOUT)
    return r.json() if r.ok and r.content else {}


def _expect(r, what, ok=(200, 201, 204)):
    if r.status_code not in ok:
        print(f"[cleanup] {what}: {r.status_code} {r.text[:200]}")


def _cancel_user_requests(base_url, _paths):
    for req in _get(base_url, "/user-requests") or []:
        # The list keeps finished requests too (R-137); only an active one can be cancelled.
        if req.get("status") != "ACTIVE":
            continue
        r = requests.delete(f"{base_url}/user-requests/{req['id']}", timeout=HTTP_TIMEOUT)
        if r.status_code == 409:
            # A started shiftable load runs to the end of its duration (`Asset::is_cancellable`).
            print(f"[cleanup] LEAK: {base_url} request {req['id']} on {req.get('asset_id')} "
                  f"cannot be cancelled and keeps running: {r.text[:200]}")
        else:
            _expect(r, f"cancel request {req['id']} on {base_url}", ok=(200, 204, 404))
    # An EV session that outlives every request blocks the next overlapping EV request with a 409.
    left = _get(base_url, "/ev-session")
    if left:
        print(f"[cleanup] LEAK: {base_url} EV sessions left after cancelling every request: {left}")


def _reset_injects(base_url, _paths):
    _expect(requests.post(f"{base_url}/sim/inject/reset", json={}, timeout=HTTP_TIMEOUT),
            f"inject reset on {base_url}")


def _reset_arbiter(base_url, _paths):
    _expect(requests.put(f"{base_url}/arbiter-settings", json=ARBITER_DEFAULTS, timeout=HTTP_TIMEOUT),
            f"arbiter reset on {base_url}")


def _clear_baseline_override(base_url, _paths):
    _expect(requests.delete(f"{base_url}/baseline-override", timeout=HTTP_TIMEOUT),
            f"baseline override clear on {base_url}", ok=(200, 204, 404))


def _clear_comfort_overrides(base_url, paths):
    # Every asset: the UI sets curves for the EV and the heater without going through api_client.
    asset_ids = {p.split("/")[2] for p in paths} | set(_get(base_url, "/sim").get("assets", {}))
    for aid in sorted(asset_ids):
        _expect(requests.delete(f"{base_url}/assets/{aid}/comfort_curve", timeout=HTTP_TIMEOUT),
                f"comfort override clear for {aid} on {base_url}", ok=(204, 404))


def _restore_battery_config(base_url, _paths):
    battery = _snapshots.get(base_url, {}).get("battery") or {}
    if "capacity_kwh" not in battery:
        print(f"[cleanup] LEAK: {base_url} battery config changed, no snapshot to restore it from")
        return
    body = {"capacity_kwh": battery["capacity_kwh"], "min_soc": battery.get("min_soc")}
    _expect(requests.put(f"{base_url}/sim/config/battery", json=body, timeout=HTTP_TIMEOUT),
            f"battery config restore on {base_url}")


def _restore_soc(base_url, paths):
    snapshot = _snapshots.get(base_url, {})
    for aid in sorted({p.rsplit("/", 1)[1] for p in paths}):
        soc = (snapshot.get(aid) or {}).get("soc")
        if soc is None:
            print(f"[cleanup] LEAK: {base_url} {aid} SoC was reset, no snapshot to restore it from")
            continue
        _expect(requests.post(f"{base_url}/sim/reset/{aid}", json={"soc": soc}, timeout=HTTP_TIMEOUT),
                f"SoC restore for {aid} on {base_url}")


# Family -> how to undo it, in the order undone: requests before injects (a cancelled request
# replans against the reset sim), battery config before SoC (min SoC bounds the SoC restored).
UNDO = [
    ("user_requests", _cancel_user_requests),
    ("injects", _reset_injects),
    ("arbiter", _reset_arbiter),
    ("baseline_override", _clear_baseline_override),
    ("comfort", _clear_comfort_overrides),
    ("battery_config", _restore_battery_config),
    ("soc", _restore_soc),
]
