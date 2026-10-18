"""Reading the VEN's `/tariffs` the way its contract and the VEN UI read it."""

from datetime import datetime


def parse_ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def price_at(rates, at):
    """Forward-fill: the API exposes each segment's start only, so the price at `at` is the
    import tariff of the last snapshot starting at or before it."""
    current = None
    for s in sorted(rates, key=lambda r: parse_ts(r["interval_start"])):
        if parse_ts(s["interval_start"]) <= at:
            current = s.get("import_tariff_eur_kwh")
        else:
            break
    return current
