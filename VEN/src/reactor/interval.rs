use chrono::{DateTime, Utc};

/// An active interval extracted from an event at a given point in time.
#[derive(Debug, Clone)]
pub struct ActiveInterval {
    pub event_id: String,
    pub event_name: String,
    pub priority: u32,
    pub payload_type: String,
    pub payload_value: f64,
    pub created: DateTime<Utc>,
}

/// Parse ISO 8601 duration (subset: PT{n}H, PT{n}M, PT{n}S, P{n}D, and combos).
fn parse_duration_secs(dur: &str) -> Option<f64> {
    if !dur.starts_with('P') {
        return None;
    }

    let s = &dur[1..]; // strip 'P'
    let mut total = 0.0;
    let mut num_buf = String::new();
    let mut in_time = false;

    for ch in s.chars() {
        match ch {
            'T' => {
                in_time = true;
            }
            '0'..='9' | '.' => {
                num_buf.push(ch);
            }
            'D' if !in_time => {
                total += num_buf.parse::<f64>().ok()? * 86400.0;
                num_buf.clear();
            }
            'H' if in_time => {
                total += num_buf.parse::<f64>().ok()? * 3600.0;
                num_buf.clear();
            }
            'M' if in_time => {
                total += num_buf.parse::<f64>().ok()? * 60.0;
                num_buf.clear();
            }
            'S' if in_time => {
                total += num_buf.parse::<f64>().ok()?;
                num_buf.clear();
            }
            _ => {}
        }
    }

    Some(total)
}

/// Find active intervals from a list of events at the given time.
pub fn find_active_intervals(
    events: &[serde_json::Value],
    now: DateTime<Utc>,
) -> Vec<ActiveInterval> {
    let mut result = Vec::new();

    for event in events {
        let event_id = event
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let event_name = event
            .get("eventName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let priority = event
            .get("priority")
            .and_then(|v| v.as_u64())
            .unwrap_or(999) as u32;
        let created = event
            .get("createdDateTime")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or(now);

        // Get event-level interval period for start/duration
        let event_start = event
            .get("intervalPeriod")
            .and_then(|ip| ip.get("start"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        let event_duration_s = event
            .get("intervalPeriod")
            .and_then(|ip| ip.get("duration"))
            .and_then(|v| v.as_str())
            .and_then(|s| parse_duration_secs(s));

        // Check if event is active (within its time window)
        if let Some(start) = event_start {
            if now < start {
                continue; // not started yet
            }
            if let Some(dur_s) = event_duration_s {
                let end = start + chrono::Duration::seconds(dur_s as i64);
                if now >= end {
                    continue; // already ended
                }
            }
        }

        // Extract payloads from intervals
        let intervals = event
            .get("intervals")
            .and_then(|v| v.as_array());

        if let Some(intervals) = intervals {
            for interval in intervals {
                // Check per-interval timing (overrides event-level timing)
                let iv_start = interval
                    .get("intervalPeriod")
                    .and_then(|ip| ip.get("start"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                let iv_duration_s = interval
                    .get("intervalPeriod")
                    .and_then(|ip| ip.get("duration"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| parse_duration_secs(s));

                if let Some(start) = iv_start {
                    if now < start {
                        continue; // interval not started yet
                    }
                    if let Some(dur_s) = iv_duration_s {
                        let end = start + chrono::Duration::seconds(dur_s as i64);
                        if now >= end {
                            continue; // interval already ended
                        }
                    }
                }

                let payloads = interval
                    .get("payloads")
                    .and_then(|v| v.as_array());

                if let Some(payloads) = payloads {
                    for payload in payloads {
                        let ptype = payload
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let pvalues = payload
                            .get("values")
                            .and_then(|v| v.as_array());

                        if let Some(values) = pvalues {
                            if let Some(val) = values.first().and_then(|v| {
                                v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                            }) {
                                result.push(ActiveInterval {
                                    event_id: event_id.clone(),
                                    event_name: event_name.clone(),
                                    priority,
                                    payload_type: ptype.to_string(),
                                    payload_value: val,
                                    created,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pt1h() {
        assert_eq!(parse_duration_secs("PT1H"), Some(3600.0));
    }

    #[test]
    fn parse_pt30m() {
        assert_eq!(parse_duration_secs("PT30M"), Some(1800.0));
    }

    #[test]
    fn parse_p1dt2h30m() {
        assert_eq!(parse_duration_secs("P1DT2H30M"), Some(95400.0));
    }
}
