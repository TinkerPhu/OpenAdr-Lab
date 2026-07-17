"""030 — steps for ven_notifications.feature (notification history endpoint).

Reuses the generic `I GET the VEN "{path}" endpoint` step from
ven_health_steps.py; only the history-specific assertions live here.
"""

from behave import then


@then("the VEN notification history response is a list of dedup-aware rows")
def step_history_is_dedup_aware_list(context):
    resp = context.ven_response
    assert resp.status_code == 200, f"expected 200, got {resp.status_code}: {resp.text}"
    rows = resp.json()
    assert isinstance(rows, list), f"expected a JSON list, got {type(rows)}"
    for row in rows:
        for field in ("id", "created_at", "severity", "message", "count", "last_seen_at"):
            assert field in row, f"row missing '{field}': {row}"
        assert row["count"] >= 1, f"count must be >= 1: {row}"
    # Oldest-first by last_seen_at (the endpoint contract).
    seen = [row["last_seen_at"] for row in rows]
    assert seen == sorted(seen), f"rows must be oldest-first by last_seen_at: {seen}"


@then('every VEN notification history row has severity "{severity}"')
def step_history_rows_have_severity(context, severity):
    resp = context.ven_response
    assert resp.status_code == 200, f"expected 200, got {resp.status_code}: {resp.text}"
    rows = resp.json()
    mismatched = [row for row in rows if row["severity"] != severity]
    assert not mismatched, f"rows with other severities: {mismatched}"


@then('the VEN notification history response is 400 with a JSON error mentioning "{text}"')
def step_history_bad_request(context, text):
    resp = context.ven_response
    assert resp.status_code == 400, f"expected 400, got {resp.status_code}: {resp.text}"
    body = resp.json()
    assert text in body.get("error", ""), f"error body missing '{text}': {body}"
