//! Building wire objects for tests, so a fixture states only what it is about.
//!
//! Not `#[cfg(test)]`: `cfg(test)` applies to the crate being tested, so a
//! helper gated that way here would be invisible to the VEN's tests, which are
//! most of its users. The cost is a function in the release binary that
//! nothing calls; the alternative is each crate keeping its own copy of the
//! defaults, which is the divergence this crate exists to prevent.

use serde_json::{json, Value};

use crate::event_timing::OadrEvent;

/// Build events for tests from the JSON a fixture actually cares about.
///
/// The wire `Event` requires `id`, `createdDateTime` and `modificationDateTime`
/// -- fields no test is about, and which the lenient DTO did not have. Rather
/// than spell them out in ~30 fixtures (and in every fixture written after
/// this), they are merged in where absent. A fixture that *does* care about one
/// states it and keeps it: this fills gaps, it does not overwrite.
///
/// `createdDateTime` defaults to `MIN_UTC` deliberately. That is the value the
/// old DTO path fell back to when the field was absent, so priority
/// tie-breaking in `rate_schedule` behaves as it always did for fixtures that
/// do not set it.
pub fn events_from_json(value: serde_json::Value) -> Vec<OadrEvent> {
    let epoch = "0001-01-01T00:00:00Z";
    let mut arr = match value {
        Value::Array(a) => a,
        one => vec![one],
    };
    for (i, ev) in arr.iter_mut().enumerate() {
        let Some(obj) = ev.as_object_mut() else {
            continue;
        };
        obj.entry("id").or_insert_with(|| json!(format!("evt-{i}")));
        obj.entry("createdDateTime").or_insert_with(|| json!(epoch));
        obj.entry("modificationDateTime")
            .or_insert_with(|| json!(epoch));
        obj.entry("programID").or_insert_with(|| json!("prog-test"));
        // `interval.id` is required by the schema and is never what a timing
        // or payload fixture is about; number them in declaration order.
        if let Some(Value::Array(intervals)) = obj.get_mut("intervals") {
            for (n, iv) in intervals.iter_mut().enumerate() {
                if let Some(io_) = iv.as_object_mut() {
                    io_.entry("id").or_insert_with(|| json!(n));
                }
            }
        }
    }
    serde_json::from_value(Value::Array(arr)).expect("test fixture is not a valid 3.1 event")
}
