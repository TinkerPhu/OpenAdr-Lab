// ── OpenADR interface ─────────────────────────────────────────────────────────
pub mod openadr_interface;

// ── Planning & dispatch ───────────────────────────────────────────────────────
pub mod dispatcher;
pub mod envelope;
pub mod milp_interactions;
pub mod milp_planner;
pub mod timeline;

// ── Monitoring & reporting ────────────────────────────────────────────────────
pub mod monitor;
pub mod reporter;

// ── User requests ─────────────────────────────────────────────────────────────
pub mod user_request;

// ── Observability ─────────────────────────────────────────────────────────────
pub mod trace;
