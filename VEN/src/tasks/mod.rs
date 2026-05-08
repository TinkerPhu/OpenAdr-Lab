// Auto-generated tasks module for split loops

pub mod poll_events;
pub mod poll_programs;

pub(crate) use poll_events::{detect_event_changes, spawn_event_poll};
pub(crate) use poll_programs::spawn_program_poll;
