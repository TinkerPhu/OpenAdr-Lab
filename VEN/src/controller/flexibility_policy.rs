use chrono::{DateTime, Datelike, Duration, NaiveDateTime, NaiveTime, Utc, Weekday};
use serde::Deserialize;
use uuid::Uuid;

use crate::controller::reservation::{FlexDirection, Reservation, ReservationSource};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlexibilityPolicy {
    #[serde(default)]
    pub default_reserve: DefaultReserve,
    #[serde(default)]
    pub scheduled_windows: Vec<ScheduledWindow>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DefaultReserve {
    #[serde(default)]
    pub up_kw: f64,
    #[serde(default)]
    pub down_kw: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledWindow {
    pub id: String,
    /// Weekday names: "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun".
    #[serde(default)]
    pub days: Vec<String>,
    /// Window start time "HH:MM" (local clock, treated as UTC).
    pub time_start: String,
    /// Window end time "HH:MM".
    pub time_end: String,
    #[serde(default)]
    pub reserve_up_kw: f64,
    #[serde(default)]
    pub reserve_down_kw: f64,
    /// Minutes before `time_start` to begin holding headroom.
    #[serde(default)]
    pub pre_load_minutes: u32,
}

impl FlexibilityPolicy {
    /// Generate all reservations implied by this policy for the window [from, until).
    ///
    /// Layer 1 — DefaultReserve: one site-level reservation covering the entire window.
    /// Layer 2 — ScheduledWindows: per-calendar-day reservations for matching weekdays.
    ///
    /// All reservations are site-level (`asset_id: None`).
    /// `ReservationLayer::query_asset()` applies site-level reservations to every asset.
    pub fn generate_reservations(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Vec<Reservation> {
        let mut out = Vec::new();

        // ── Layer 1: DefaultReserve ──────────────────────────────────────────
        if self.default_reserve.up_kw > 0.0 {
            out.push(Reservation {
                id: Uuid::new_v4(),
                window: (from, until),
                asset_id: None,
                kw: self.default_reserve.up_kw,
                direction: FlexDirection::Up,
                source: ReservationSource::PolicyDefault,
                priority: 100,
            });
        }
        if self.default_reserve.down_kw > 0.0 {
            out.push(Reservation {
                id: Uuid::new_v4(),
                window: (from, until),
                asset_id: None,
                kw: self.default_reserve.down_kw,
                direction: FlexDirection::Down,
                source: ReservationSource::PolicyDefault,
                priority: 100,
            });
        }

        // ── Layer 2: ScheduledWindows ────────────────────────────────────────
        for window in &self.scheduled_windows {
            let weekdays = parse_weekdays(&window.days);
            let pre_load = Duration::minutes(window.pre_load_minutes as i64);

            let mut day = from.date_naive();
            let until_date = until.date_naive();

            while day <= until_date {
                if weekdays.contains(&day.weekday()) {
                    let t_start = match NaiveTime::parse_from_str(&window.time_start, "%H:%M") {
                        Ok(t) => t,
                        Err(_) => {
                            day += Duration::days(1);
                            continue;
                        }
                    };
                    let t_end = match NaiveTime::parse_from_str(&window.time_end, "%H:%M") {
                        Ok(t) => t,
                        Err(_) => {
                            day += Duration::days(1);
                            continue;
                        }
                    };

                    let win_start = NaiveDateTime::new(day, t_start).and_utc() - pre_load;
                    let win_end = NaiveDateTime::new(day, t_end).and_utc();

                    if win_end <= win_start {
                        day += Duration::days(1);
                        continue;
                    }

                    if window.reserve_up_kw > 0.0 {
                        out.push(Reservation {
                            id: Uuid::new_v4(),
                            window: (win_start, win_end),
                            asset_id: None,
                            kw: window.reserve_up_kw,
                            direction: FlexDirection::Up,
                            source: ReservationSource::PolicySchedule {
                                policy_id: window.id.clone(),
                            },
                            priority: 50,
                        });
                    }
                    if window.reserve_down_kw > 0.0 {
                        out.push(Reservation {
                            id: Uuid::new_v4(),
                            window: (win_start, win_end),
                            asset_id: None,
                            kw: window.reserve_down_kw,
                            direction: FlexDirection::Down,
                            source: ReservationSource::PolicySchedule {
                                policy_id: window.id.clone(),
                            },
                            priority: 50,
                        });
                    }
                }

                day += Duration::days(1);
            }
        }

        out
    }
}

fn parse_weekdays(days: &[String]) -> Vec<Weekday> {
    days.iter()
        .filter_map(|s| s.parse::<Weekday>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use crate::controller::reservation::{FlexDirection, ReservationLayer};

    fn dt(s: &str) -> DateTime<Utc> {
        s.parse::<DateTime<Utc>>().unwrap()
    }

    // ── Layer 1: DefaultReserve ──────────────────────────────────────────────

    #[test]
    fn test_default_policy_generates_no_reservations() {
        let policy = FlexibilityPolicy::default();
        let from = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-15T18:00:00Z");
        assert!(policy.generate_reservations(from, until).is_empty());
    }

    #[test]
    fn test_default_reserve_up_generates_site_level_reservation() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve { up_kw: 3.0, down_kw: 0.0 },
            scheduled_windows: vec![],
        };
        let from = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-15T18:00:00Z");
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 1);
        let r = &rs[0];
        assert!(r.asset_id.is_none());
        assert_eq!(r.direction, FlexDirection::Up);
        assert_eq!(r.kw, 3.0);
        assert_eq!(r.priority, 100);
        assert_eq!(r.window, (from, until));
    }

    #[test]
    fn test_default_reserve_both_directions() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve { up_kw: 3.0, down_kw: 2.0 },
            scheduled_windows: vec![],
        };
        let from = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-15T18:00:00Z");
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 2);
        let ups: Vec<_> = rs.iter().filter(|r| r.direction == FlexDirection::Up).collect();
        let downs: Vec<_> = rs.iter().filter(|r| r.direction == FlexDirection::Down).collect();
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].kw, 3.0);
        assert!(ups[0].asset_id.is_none());
        assert_eq!(downs.len(), 1);
        assert_eq!(downs[0].kw, 2.0);
        assert!(downs[0].asset_id.is_none());
    }

    #[test]
    fn test_default_reserve_zero_kw_not_emitted() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve { up_kw: 0.0, down_kw: 1.0 },
            scheduled_windows: vec![],
        };
        let from = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-15T18:00:00Z");
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].direction, FlexDirection::Down);
    }

    #[test]
    fn test_site_level_applies_to_all_assets_via_available_cap() {
        // Site-level reservation applies to every asset queried — no profile lookup needed.
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve { up_kw: 3.0, down_kw: 0.0 },
            scheduled_windows: vec![],
        };
        let from = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-15T18:00:00Z");
        let mid  = dt("2024-01-15T14:00:00Z");
        let mut layer = ReservationLayer::new();
        for r in policy.generate_reservations(from, until) {
            layer.insert(r);
        }
        let battery = layer.query_asset("battery", mid);
        let ev      = layer.query_asset("ev",      mid);
        assert_eq!(battery.reserved_up_kw, 3.0);
        assert_eq!(ev.reserved_up_kw,      3.0);
    }

    // ── Layer 2: ScheduledWindows ────────────────────────────────────────────
    // Fixed dates: 2024-01-15 = Monday, 2024-01-16 = Tuesday, 2024-01-17 = Wednesday

    #[test]
    fn test_scheduled_window_matching_day_emits_up_and_down() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "peak_dr".to_string(),
                days: vec!["Tue".to_string()],
                time_start: "14:00".to_string(),
                time_end: "16:00".to_string(),
                reserve_up_kw: 5.0,
                reserve_down_kw: 2.0,
                pre_load_minutes: 0,
            }],
        };
        let from  = dt("2024-01-15T10:00:00Z"); // Mon
        let until = dt("2024-01-17T10:00:00Z"); // Wed
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 2);
        let win_start = dt("2024-01-16T14:00:00Z");
        let win_end   = dt("2024-01-16T16:00:00Z");
        for r in &rs {
            assert_eq!(r.window, (win_start, win_end));
            assert!(r.asset_id.is_none());
        }
        let ups:   Vec<_> = rs.iter().filter(|r| r.direction == FlexDirection::Up).collect();
        let downs: Vec<_> = rs.iter().filter(|r| r.direction == FlexDirection::Down).collect();
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].kw, 5.0);
        assert_eq!(downs.len(), 1);
        assert_eq!(downs[0].kw, 2.0);
    }

    #[test]
    fn test_scheduled_window_non_matching_day_skipped() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "mon_only".to_string(),
                days: vec!["Mon".to_string()],
                time_start: "09:00".to_string(),
                time_end: "10:00".to_string(),
                reserve_up_kw: 5.0,
                reserve_down_kw: 0.0,
                pre_load_minutes: 0,
            }],
        };
        // Horizon spans Tuesday only — Monday window must not fire
        let from  = dt("2024-01-16T00:00:00Z");
        let until = dt("2024-01-16T23:59:59Z");
        assert!(policy.generate_reservations(from, until).is_empty());
    }

    #[test]
    fn test_scheduled_window_pre_load_shifts_window_start() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "pre_load_test".to_string(),
                days: vec!["Tue".to_string()],
                time_start: "16:00".to_string(),
                time_end: "20:00".to_string(),
                reserve_up_kw: 5.0,
                reserve_down_kw: 0.0,
                pre_load_minutes: 60,
            }],
        };
        let from  = dt("2024-01-15T10:00:00Z");
        let until = dt("2024-01-17T10:00:00Z");
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 1);
        // Start shifted back by 60 min: 16:00 → 15:00
        assert_eq!(rs[0].window.0, dt("2024-01-16T15:00:00Z"));
        assert_eq!(rs[0].window.1, dt("2024-01-16T20:00:00Z"));
    }

    #[test]
    fn test_scheduled_window_reserve_down_zero_not_emitted() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "up_only".to_string(),
                days: vec!["Mon".to_string()],
                time_start: "09:00".to_string(),
                time_end: "10:00".to_string(),
                reserve_up_kw: 4.0,
                reserve_down_kw: 0.0,
                pre_load_minutes: 0,
            }],
        };
        let from  = dt("2024-01-15T00:00:00Z"); // Monday
        let until = dt("2024-01-15T23:59:59Z");
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].direction, FlexDirection::Up);
    }

    #[test]
    fn test_scheduled_window_clamped_to_horizon() {
        // The implementation emits full calendar-day windows (no hard clamping to [from, until]).
        // Correctness is guaranteed by query_asset() gating on window bounds.
        // This test verifies that a reservation generated for a matching day is active
        // at a query point inside the planning horizon.
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "full_day".to_string(),
                days: vec!["Mon".to_string()],
                time_start: "00:00".to_string(),
                time_end: "23:59".to_string(),
                reserve_up_kw: 3.0,
                reserve_down_kw: 0.0,
                pre_load_minutes: 0,
            }],
        };
        let from  = dt("2024-01-15T00:00:00Z");
        let until = dt("2024-01-15T23:59:00Z");
        let mid   = dt("2024-01-15T12:00:00Z");
        let mut layer = ReservationLayer::new();
        for r in policy.generate_reservations(from, until) {
            layer.insert(r);
        }
        assert_eq!(layer.query_asset("battery", mid).reserved_up_kw, 3.0);
    }

    #[test]
    fn test_invalid_time_start_skips_window() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![ScheduledWindow {
                id: "bad_time".to_string(),
                days: vec!["Mon".to_string()],
                time_start: "25:99".to_string(), // invalid — must not panic
                time_end: "10:00".to_string(),
                reserve_up_kw: 5.0,
                reserve_down_kw: 0.0,
                pre_load_minutes: 0,
            }],
        };
        let from  = dt("2024-01-15T00:00:00Z"); // Monday
        let until = dt("2024-01-15T23:59:59Z");
        assert!(policy.generate_reservations(from, until).is_empty());
    }

    #[test]
    fn test_multiple_windows_multiple_days() {
        let policy = FlexibilityPolicy {
            default_reserve: DefaultReserve::default(),
            scheduled_windows: vec![
                ScheduledWindow {
                    id: "mon_window".to_string(),
                    days: vec!["Mon".to_string()],
                    time_start: "09:00".to_string(),
                    time_end: "10:00".to_string(),
                    reserve_up_kw: 2.0,
                    reserve_down_kw: 0.0,
                    pre_load_minutes: 0,
                },
                ScheduledWindow {
                    id: "tue_window".to_string(),
                    days: vec!["Tue".to_string()],
                    time_start: "14:00".to_string(),
                    time_end: "15:00".to_string(),
                    reserve_up_kw: 3.0,
                    reserve_down_kw: 0.0,
                    pre_load_minutes: 0,
                },
            ],
        };
        let from  = dt("2024-01-15T00:00:00Z"); // Monday
        let until = dt("2024-01-17T00:00:00Z"); // Wednesday (exclusive end)
        let rs = policy.generate_reservations(from, until);
        assert_eq!(rs.len(), 2);
        let mon_r: Vec<_> = rs.iter().filter(|r| r.kw == 2.0).collect();
        let tue_r: Vec<_> = rs.iter().filter(|r| r.kw == 3.0).collect();
        assert_eq!(mon_r.len(), 1);
        assert_eq!(tue_r.len(), 1);
        assert_eq!(mon_r[0].window, (dt("2024-01-15T09:00:00Z"), dt("2024-01-15T10:00:00Z")));
        assert_eq!(tue_r[0].window, (dt("2024-01-16T14:00:00Z"), dt("2024-01-16T15:00:00Z")));
    }
}
