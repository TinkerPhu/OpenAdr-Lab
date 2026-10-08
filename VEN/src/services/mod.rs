/// Test support utilities for VEN controller unit tests.
#[cfg(test)]
pub mod test_support;

pub mod planning;
pub use planning::PlanningService;

pub mod request_submission;
pub mod user_request;

pub mod ev_usage_plan;
pub mod forecast;
pub mod heuristics;

pub mod comfort;
pub mod history_sampling;
pub mod notify;
pub mod obligation;
pub use obligation::ObligationService;

// `hems::HvacService` was deleted (BL-23). Its two methods only forwarded to
// `AppState::set_heater_target`, and the route BL-23 proposed wiring through
// them -- `post_heater_target` -- no longer exists: BL-41 retired the
// direct-CRUD session API in favour of `/user-requests`. Nothing could call it
// any more, so the "wire it or delete it" decision had only one branch left.
