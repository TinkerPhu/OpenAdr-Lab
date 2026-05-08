// Auto-generated tasks module for split loops

pub mod poll_events;
pub mod poll_programs;
pub mod poll_reports;
pub mod obligation;
pub mod planning;

pub(crate) use poll_events::{detect_event_changes, spawn_event_poll};
pub(crate) use poll_programs::spawn_program_poll;
pub(crate) use poll_reports::spawn_report_poll;
pub(crate) use obligation::spawn_obligation_check;
pub(crate) use planning::spawn_planning;
