//! One rule for "the VTN sent an object we cannot parse".
//!
//! The two fetchers disagreed about what that means. `fetch_events` and
//! `fetch_programs` ended in `collect::<Result<Vec<_>>>()`, so the *first*
//! unparseable object failed the whole poll and the VEN went on acting on stale
//! capacity limits, prices and dispatch windows — one bad event blinding it to
//! every good one. `fetch_reports` did the opposite, `filter_map(..ok())`,
//! dropping bad objects without a word. Neither surfaces anything.
//!
//! This is the single answer both call sites use: parse each object
//! independently, keep every one that parses, and hand the caller a record of
//! each one that did not, so it can be surfaced. Rejecting silently is the same
//! failure as accepting silently.

use serde::de::DeserializeOwned;

/// One object the VTN sent that we could not parse.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedObject {
    /// The object's `id`, when the JSON carried a string one. It is the only
    /// field we can rely on before the object is typed, and it is what makes a
    /// rejection actionable — without it the operator cannot find the object on
    /// the VTN. `None` when even that is absent or not a string.
    pub id: Option<String>,
    /// What serde objected to, for the log and the notification detail.
    pub reason: String,
}

impl RejectedObject {
    /// How this rejection reads in a notification or a `/health` detail.
    pub fn describe(&self) -> String {
        match &self.id {
            Some(id) => format!("{id}: {}", self.reason),
            None => format!("<no id>: {}", self.reason),
        }
    }
}

/// What one page of wire objects parsed into: the usable ones, and a record of
/// the ones that were refused.
#[derive(Debug, Clone)]
pub struct FetchOutcome<T> {
    pub items: Vec<T>,
    pub rejected: Vec<RejectedObject>,
}

impl<T> FetchOutcome<T> {
    /// A one-line summary of what was refused, for a notification or a log.
    /// `None` when nothing was refused.
    pub fn rejection_summary(&self, resource: &str) -> Option<String> {
        if self.rejected.is_empty() {
            return None;
        }
        let detail = self
            .rejected
            .iter()
            .map(|r| r.describe())
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "VTN sent {} malformed {resource} object(s), ignored: {detail}",
            self.rejected.len()
        ))
    }
}

/// Parse each object on its own. One malformed object costs exactly itself.
pub fn partition_valid<T: DeserializeOwned>(items: &[serde_json::Value]) -> FetchOutcome<T> {
    let mut parsed = Vec::with_capacity(items.len());
    let mut rejected = Vec::new();
    for value in items {
        match serde_json::from_value::<T>(value.clone()) {
            Ok(item) => parsed.push(item),
            Err(e) => rejected.push(RejectedObject {
                id: value.get("id").and_then(|v| v.as_str()).map(str::to_string),
                reason: e.to_string(),
            }),
        }
    }
    FetchOutcome {
        items: parsed,
        rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Thing {
        id: String,
        size: i64,
    }

    fn parse(vals: Vec<serde_json::Value>) -> FetchOutcome<Thing> {
        partition_valid::<Thing>(&vals)
    }

    #[test]
    fn partition_valid_keeps_every_object_that_parses() {
        let out = parse(vec![
            json!({"id": "a", "size": 1}),
            json!({"id": "b", "size": 2}),
        ]);
        assert_eq!(out.items.len(), 2);
        assert!(out.rejected.is_empty());
    }

    /// The bug this module exists for: one malformed object used to fail the
    /// whole poll, leaving the VEN on stale state. It must now cost only itself.
    #[test]
    fn partition_valid_keeps_the_good_objects_when_one_is_malformed() {
        let out = parse(vec![
            json!({"id": "good-1", "size": 1}),
            json!({"id": "bad", "size": "not-a-number"}),
            json!({"id": "good-2", "size": 2}),
        ]);
        assert_eq!(
            out.items,
            vec![
                Thing {
                    id: "good-1".into(),
                    size: 1
                },
                Thing {
                    id: "good-2".into(),
                    size: 2
                },
            ],
            "a malformed object must not cost the objects around it"
        );
        assert_eq!(out.rejected.len(), 1);
    }

    /// Rejecting silently is the same failure as accepting silently: the
    /// rejection has to name the object so an operator can find it.
    #[test]
    fn partition_valid_records_the_id_of_a_rejected_object() {
        let out = parse(vec![json!({"id": "bad-event", "size": "nope"})]);
        assert_eq!(out.rejected[0].id.as_deref(), Some("bad-event"));
        assert!(
            out.rejected[0].describe().starts_with("bad-event:"),
            "got {:?}",
            out.rejected[0].describe()
        );
    }

    #[test]
    fn partition_valid_records_a_rejection_even_without_an_id() {
        let out = parse(vec![json!({"size": 1})]);
        assert_eq!(out.rejected.len(), 1);
        assert_eq!(out.rejected[0].id, None);
        assert!(out.rejected[0].describe().starts_with("<no id>:"));
    }

    #[test]
    fn rejection_summary_is_none_when_nothing_was_refused() {
        let out = parse(vec![json!({"id": "a", "size": 1})]);
        assert_eq!(out.rejection_summary("event"), None);
    }

    #[test]
    fn rejection_summary_names_the_count_and_the_objects() {
        let out = parse(vec![
            json!({"id": "x", "size": "bad"}),
            json!({"id": "y", "size": "bad"}),
        ]);
        let summary = out.rejection_summary("event").expect("some rejections");
        assert!(summary.contains('2'), "got {summary}");
        assert!(summary.contains("x:"), "got {summary}");
        assert!(summary.contains("y:"), "got {summary}");
    }
}
