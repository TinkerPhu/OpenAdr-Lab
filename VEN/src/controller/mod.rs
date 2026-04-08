// ── OpenADR interface ─────────────────────────────────────────────────────────
pub mod openadr_interface;

// ── Planning & dispatch ───────────────────────────────────────────────────────
pub mod dispatcher;
pub mod envelope;
pub mod flexibility_policy;
pub mod lp_planner;
pub mod planner;
pub mod reservation;
pub mod timeline;

// ── Monitoring & reporting ────────────────────────────────────────────────────
pub mod monitor;
pub mod reporter;

// ── User requests ─────────────────────────────────────────────────────────────
pub mod user_request;

// ── Shared numeric thresholds ─────────────────────────────────────────────────
pub mod thresholds;

// ── Observability ─────────────────────────────────────────────────────────────
pub mod trace;
