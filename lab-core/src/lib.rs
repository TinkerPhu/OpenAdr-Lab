//! The rules the VEN and the VTN's BFF must agree on.
//!
//! Both sides answer the same two questions about the same data: *when does
//! this interval run*, and *what is this series' value over that window*. When
//! each had its own answer they drifted — GB-48 is the record of what that
//! costs: "when does interval i of an OpenADR event run" decided in seven
//! parsers with four rules, and the differences between the copies were the
//! bugs.
//!
//! So the rules live here once, and both services call them. The crate is
//! deliberately thin — `chrono` and the OpenADR wire types — because a shared
//! crate only prevents divergence if depending on it stays cheaper than
//! re-implementing.
//!
//! What belongs here: rules that are *the same question* on both sides.
//! What does not: anything either side is the sole authority for — an asset's
//! own capability or forecast (`asset-competence-assurance`), the VEN's
//! planning, the BFF's aggregation policy.

pub mod event_timing;
pub mod test_fixtures;
pub mod time_series;
pub mod time_window;

pub use time_window::TimeWindow;
