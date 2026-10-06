"""Step definitions for the asset key-features scenarios (asset_key_features.feature).

Uses context.last_response_json set by the shared "I GET {path} from the VEN" step.
"""

import re

from behave import then


def _key_feature_value(context, label):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    features = data.get("key_features")
    assert features is not None, f"'key_features' missing from response: {data}"
    values = [f["value"] for f in features if f["label"] == label]
    assert len(values) == 1, (
        f"expected exactly one key feature labelled {label!r}, got {features}"
    )
    return values[0]


@then('the capability key feature "{label}" is "{value}"')
def step_capability_key_feature_is(context, label, value):
    actual = _key_feature_value(context, label)
    assert actual == value, f"key feature {label!r}: expected {value!r}, got {actual!r}"


@then('the capability key feature "{label}" is "-" or a kW value')
def step_capability_key_feature_is_dash_or_kw(context, label):
    actual = _key_feature_value(context, label)
    assert actual == "-" or re.fullmatch(r"\d+\.\d{2} kW", actual), (
        f"key feature {label!r}: expected '-' or a value like '0.52 kW', got {actual!r}"
    )
