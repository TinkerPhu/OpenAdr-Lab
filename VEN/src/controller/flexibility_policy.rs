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
