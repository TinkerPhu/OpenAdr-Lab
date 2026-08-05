mod arbiter;
mod baseline_override;
mod comfort;
mod ev;
mod history;
mod misc;
mod sessions;

pub use arbiter::*;
pub use baseline_override::*;
pub use comfort::*;
pub use ev::*;
pub use history::*;
pub use misc::*;
pub use sessions::*;

use serde::Serialize;

use crate::entities::device_session::{EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::user_request::UserRequest;

/// Embedded session detail for GET /user-requests response.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionDetail {
    Ev(EvSession),
    Heater(HeaterTarget),
    ShiftableLoad(ShiftableLoad),
}

/// Enriched user request with embedded session details.
#[derive(Debug, Clone, Serialize)]
pub struct UserRequestWithSession {
    #[serde(flatten)]
    pub request: UserRequest,
    pub session: Option<SessionDetail>,
}
