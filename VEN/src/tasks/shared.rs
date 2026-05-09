// Auto-generated shared helpers for tasks
//! Keep minimal to avoid lint noise
#![allow(dead_code)]

pub(crate) fn task_name(prefix: &str, name: &str) -> String {
    format!("{}::{}", prefix, name)
}
