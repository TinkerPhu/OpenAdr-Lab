// Generated from the Rust types (R-133): `VEN/src/ui_types.rs` lists them and
// `UPDATE_UI_TYPES=1 cargo test ui_types` regenerates them. Do not edit `./generated/`.
import type { ActivePenaltyRule } from "./generated/ActivePenaltyRule";
import type { AssetAllocation } from "./generated/AssetAllocation";
import type { AssetCapabilityView } from "./generated/AssetCapabilityView";
import type { AssetForecast } from "./generated/AssetForecast";
import type { AssetLedgerEntry } from "./generated/AssetLedgerEntry";
import type { AssetSnapshot } from "./generated/AssetSnapshot";
import type { ComfortCurveResponse } from "./generated/ComfortCurveResponse";
import type { ComfortCurveSource } from "./generated/ComfortCurveSource";
import type { ComfortRate } from "./generated/ComfortRate";
import type { ControlDescriptor } from "./generated/ControlDescriptor";
import type { ControlKind } from "./generated/ControlKind";
import type { CostBreakdown } from "./generated/CostBreakdown";
import type { EvSession } from "./generated/EvSession";
import type { EvSessionOrigin } from "./generated/EvSessionOrigin";
import type { FlexibilityEnvelope } from "./generated/FlexibilityEnvelope";
import type { ForecastSource } from "./generated/ForecastSource";
import type { GridSnapshot } from "./generated/GridSnapshot";
import type { HeaterTarget } from "./generated/HeaterTarget";
import type { KeyFeature } from "./generated/KeyFeature";
import type { Plan } from "./generated/Plan";
import type { PlanHistorySample } from "./generated/PlanHistorySample";
import type { PlanPhaseReport } from "./generated/PlanPhaseReport";
import type { PlanSummary } from "./generated/PlanSummary";
import type { PlanTimeSlot } from "./generated/PlanTimeSlot";
import type { PlanTrigger } from "./generated/PlanTrigger";
import type { PlanWarning } from "./generated/PlanWarning";
import type { PlanZone } from "./generated/PlanZone";
import type { PlannerEvent } from "./generated/PlannerEvent";
import type { PlannerObjective } from "./generated/PlannerObjective";
import type { PlanningHorizon } from "./generated/PlanningHorizon";
import type { PowerAdjustability } from "./generated/PowerAdjustability";
import type { RequestDeadline } from "./generated/RequestDeadline";
import type { SensorRaw } from "./generated/SensorRaw";
import type { SensorSnapshot } from "./generated/SensorSnapshot";
import type { SessionDetail } from "./generated/SessionDetail";
import type { SessionType } from "./generated/SessionType";
import type { ShiftableLoad } from "./generated/ShiftableLoad";
import type { SimInjectState } from "./generated/SimInjectState";
import type { SimSnapshot } from "./generated/SimSnapshot";
import type { SolveStatus } from "./generated/SolveStatus";
import type { StepRule } from "./generated/StepRule";
import type { TimeRange } from "./generated/TimeRange";
import type { UserRequest } from "./generated/UserRequest";
import type { UserRequestMode } from "./generated/UserRequestMode";
import type { UserRequestStatus } from "./generated/UserRequestStatus";
import type { UserRequestWithSession } from "./generated/UserRequestWithSession";
import type { WarningKind } from "./generated/WarningKind";
import type { WarningSeverity } from "./generated/WarningSeverity";
export type {
  ActivePenaltyRule,
  AssetAllocation,
  AssetCapabilityView,
  AssetForecast,
  AssetLedgerEntry,
  AssetSnapshot,
  ComfortCurveResponse,
  ComfortCurveSource,
  ComfortRate,
  ControlDescriptor,
  ControlKind,
  CostBreakdown,
  EvSession,
  EvSessionOrigin,
  FlexibilityEnvelope,
  ForecastSource,
  GridSnapshot,
  HeaterTarget,
  KeyFeature,
  Plan,
  PlanHistorySample,
  PlanPhaseReport,
  PlanSummary,
  PlanTimeSlot,
  PlanTrigger,
  PlanWarning,
  PlanZone,
  PlannerEvent,
  PlannerObjective,
  PlanningHorizon,
  PowerAdjustability,
  RequestDeadline,
  SensorRaw,
  SensorSnapshot,
  SessionDetail,
  SessionType,
  ShiftableLoad,
  SimInjectState,
  SimSnapshot,
  SolveStatus,
  StepRule,
  TimeRange,
  UserRequest,
  UserRequestMode,
  UserRequestStatus,
  UserRequestWithSession,
  WarningKind,
  WarningSeverity,
};

// ─── UI-side models derived from a wire type ──────────────────────────────────
// The client converts a few wire fields on arrival (ISO timestamps to epoch ms). Each model is
// stated as its wire type with those fields replaced, so the rest cannot drift.

/** `PlanHistorySample` with `created_at` in epoch ms (`api.historyPlans`). */
export type PlanHistoryRow = Omit<PlanHistorySample, "created_at"> & { created_at: number };

// WP-T1 (docs/history/project_journal.md, search "WP-T"): componentised health, replacing the
// previous plain "ok" string GET /health used to return.
export type HealthComponentStatus = { status: "ok" | "degraded"; detail?: string };

export type HealthResponse = {
  status: "ok" | "degraded";
  components: {
    ven_process: HealthComponentStatus;
    vtn_connection: HealthComponentStatus;
    storage: HealthComponentStatus;
    planner: HealthComponentStatus;
    /** Whether the VTN's last poll carried an object this VEN refused. */
    wire_conformance?: HealthComponentStatus;
    /**
     * Live fleet telemetry. Absent when this VEN does not publish at all,
     * which is a normal deployment rather than a fault — and the reason this
     * is optional rather than always present.
     */
    fleet_telemetry?: HealthComponentStatus;
  };
  /** Server clock (ISO 8601) — lets a client detect/correct for its own clock skew. */
  server_time: string;
  /**
   * Event payload types whose unit the latest poll read from the profile default because
   * neither the event nor its program declared one, with how many payloads (GB-50). Legal, so
   * it never makes `status` degraded; shown so the assumption is not silent.
   */
  wire_assumptions?: Record<string, number>;
};

export type VtnStatus = {
  connected: boolean;
  last_success_ts: string | null;
  last_error: string | null;
  current_backoff_s: number;
  token_expires_at: string | null;
};

// WP-T3 (docs/history/project_journal.md, search "WP-T"): per-task restart/outcome status.
export type TaskStatusEntry = {
  name: string;
  last_run_ts: string | null;
  last_success: boolean | null;
  restart_count: number;
};

// WP-T4 (docs/history/project_journal.md, search "WP-T"): VEN-operational Event Log entry,
// deliberately separate from UserNotification (see Notifications types).
export type EventLogEntry = {
  id: string;
  created_at: string;
  category: string;
  message: string;
};

export type Program = {
  id: string;
  programName?: string | null;
  /** valuesMap list; 3.1's replacement for the dropped programType. */
  attributes?: unknown[] | null;
  targets?: string[] | null;
  createdDateTime?: string | null;
  [key: string]: unknown;
};

export type IntervalPeriod = {
  start: string;
  duration?: string | null;
};

export type TargetEntry = {
  type: string;
  values: string[];
};

export type Interval = {
  id: number;
  intervalPeriod?: IntervalPeriod | null;
  payloads?: { type: string; values: number[] }[];
};

export type VtnEvent = {
  id: string;
  programID?: string | null;
  eventName?: string | null;
  priority?: number | null;
  /** New in 3.1: an event-level duration, independent of `intervals`. */
  duration?: string | null;
  intervalPeriod?: IntervalPeriod | null;
  /** 3.1: a flat list of target strings; empty means every VEN. */
  targets?: string[] | null;
  createdDateTime?: string | null;
  /** Optional in 3.1: an event may declare a duration and no intervals. */
  intervals?: Interval[];
  [key: string]: unknown;
};

export type Report = {
  id: string;
  /** VTN-provisioned in 3.1; 3.0's programID is gone. */
  clientID?: string | null;
  eventID?: string | null;
  clientName?: string | null;
  reportName?: string | null;
  resources?: unknown;
  createdDateTime?: string | null;
  [key: string]: unknown;
};

/** WP-T5 (G-5): outcome of a VEN-initiated report submission. */
export type ReportSubmission = {
  report_name: string | null;
  event_id: string | null;
  client_name: string;
  vtn_accepted: boolean;
  submitted_at: string;
  error: string | null;
};

/**
 * How many intervals one report currently carries (D-3).
 *
 * Visible because this window is what the VTN's copy of the report contains:
 * stuck at one means accumulation is not happening, pinned at `maxIntervals`
 * means it is trimming the oldest away.
 */
export type ReportWindow = {
  reportName: string;
  intervals: number;
  maxIntervals: number;
};

export type TraceEntry =
  // `event_id` and `modification_date_time` identify the event *version* the
  // VEN acted on: names are not unique and an edited event keeps its name.
  // Both are optional here because trace rows written before the fields
  // existed are still read back.
  | { type: "OpenAdrArrived";   ts: string; event_id?: string; modification_date_time?: string | null; event_name: string; signal_type: string; value: number; interval: number }
  | { type: "OpenAdrExpired";   ts: string; event_id?: string; event_name: string }
  | { type: "RateChange";       ts: string; interval_start: string; import_eur_kwh: number; export_eur_kwh: number }
  | { type: "CapacityChange";   ts: string; import_limit_kw: number | null; export_limit_kw: number | null }
  // `event_ids` names the VTN events that caused this replan (empty for a
  // periodic or local trigger). Optional here because rows written before the
  // field existed are still read back.
  | { type: "PlanCycle";        ts: string; trigger_reason: string; event_ids?: string[]; total_slots: number }
  // Statuses as `/user-requests` spells them; `from_status` is null for a created request.
  | { type: "RequestTransition"; ts: string; request_id: string; asset_id: string; from_status: UserRequestStatus | null; to_status: UserRequestStatus }
  | { type: "DispatchOverride";  ts: string; setpoint_kw: number | null; active: boolean }
  | { type: "ArbiterDecision";   ts: string; pass: string; active_lever: string | null; target_kw: number | null; excess_kw: number | null; unresolved_kw: number };

// ─── Sim schema types ─────────────────────────────────────────────────────────

// ─── HEMS Controller types ────────────────────────────────────────────────────

export type TariffSnapshot = {
  interval_start: string;
  import_tariff_eur_kwh: number | null;
  export_tariff_eur_kwh: number | null;
  co2_g_kwh: number | null;
};

export type PlannedRates = TariffSnapshot[];

/** Dynamic Operating Envelope schedule entry (OpenADR 3.1 User Guide §8.10.1)
 * — one per interval, mirroring TariffSnapshot's shape. Unlike OadrCapacityState
 * below (a single current-value scalar), this keeps the full forward/backward
 * schedule so it can be charted the same way tariffs are. */
export type CapacitySnapshot = {
  interval_start: string;
  import_limit_kw: number | null;
  export_limit_kw: number | null;
};

export type CapacitySchedule = CapacitySnapshot[];

export type OadrCapacityState = {
  import_limit_kw: number | null;
  export_limit_kw: number | null;
  import_subscription_kw: number | null;
  import_reservation_kw: number | null;
  export_subscription_kw: number | null;
  export_reservation_kw: number | null;
  import_limit_event_id: string | null;
  export_limit_event_id: string | null;
  last_updated: string | null;
};

/** WP3.1: an active grid-alert window (parsed from ALERT_* events). */
export type AlertWindow = {
  alert_type: string;
  start: string;
  end: string;
  event_id: string;
  message: string;
};

/** WP3.2: an active SIMPLE load-shed window (levels 1-3). */
export type SimpleWindow = {
  level: number;
  start: string;
  end: string;
  event_id: string;
};

/** WP3.4: an active DISPATCH_SETPOINT window. */
export type DispatchWindow = {
  setpoint_kw: number;
  start: string;
  end: string;
  event_id: string;
};

/**
 * Additional grid capacity this VEN is requesting beyond its contracted
 * allowance (R-76) — what goes out as the `IMPORT_RESERVATION_CAPACITY` /
 * `EXPORT_RESERVATION_CAPACITY` report payloads. Both are >= 0; zero means
 * nothing to ask for, which is the normal case while no VTN program issues
 * subscription or reservation events. `null` before the first headroom
 * computation.
 */
export type ReservationRequest = {
  import_kw: number;
  export_kw: number;
};

/** WP4.6: GET /signals — one-round-trip aggregate for the grid-signal strip. */
export type SignalsState = {
  alerts: AlertWindow[];
  simple: SimpleWindow[];
  dispatch: DispatchWindow[];
  capacity: OadrCapacityState;
  reservation_request: ReservationRequest | null;
};

/** WP4.3 (BL-20): user-facing notification severity. */
export type UserNotificationSeverity = "INFO" | "WARN" | "ALERT";

/** WP4.3 (BL-20): one entry in the notification feed. */
export type UserNotification = {
  id: string;
  created_at: string;
  severity: UserNotificationSeverity;
  message: string;
  asset_id: string | null;
  event_id: string | null;
  /** 030: repeats with this key inside the rolling window collapse into one row. */
  dedup_key: string | null;
  /** 030: occurrence count (dedup hits + 1). */
  count: number;
  /** 030: last occurrence; equals created_at until a dedup hit bumps it. */
  last_seen_at: string;
};

export type CreateUserRequestBody = {
  asset_id: string;
  target_soc: number | null;
  target_energy_kwh: number | null;
  desired_power_kw: number | null;
  completion_policy: string | null;
  deadlines: Array<{
    latest_end: string;
    max_total_cost_eur: number | null;
    max_marginal_rate_eur_kwh: number | null;
    min_completion: number | null;
  }>;
  comfort_rates: null;
  // WM shiftable-load fields (optional) — `earliest_start` is shared with the EV,
  // where it means the instant the vehicle becomes available for the session.
  power_kw?: number;
  duration_min?: number;
  earliest_start?: string;
  /** EV only: km expected after this session's departure. Omitted = use the EV's
   *  own configured default. */
  expected_trip_distance_km?: number;
  /**
   * EV only: when the car is expected back from that trip.
   *
   * Paired with `expected_trip_distance_km` — both or neither. The server refuses a
   * half-stated estimate: a distance with no return time is energy with no instant to
   * apply it to. With neither, the plan projects no drop and waits for the measured
   * return rather than inventing one.
   */
  expected_return_time?: string;
  latest_end?: string;
  // Per-device overrides (Plan D)
  soft_deadline?: boolean;
  /** Null = state none: the VEN applies the heater's declared default, or refuses. */
  target_temp_c?: number | null;
  // Request mode (BL-28); omitted = BY_DEADLINE
  mode?: UserRequestMode;
  // MAX_COST (WP4.1-c): total charging-cost ceiling in €
  budget_eur?: number;
  /**
   * EV only: the queued sessions this submission means to displace.
   *
   * Omitted = displace nothing, so a clash is refused — the default and the safe
   * answer. Sent, it must be exactly the ids the refusal quoted back as
   * `replaceable_session_ids`, so the confirmation refers to the plans the user was
   * actually shown rather than to whatever clashes by the time the server acts.
   */
  replace_session_ids?: string[];
};

// ─── Device Session types ─────────────────────────────────────────────────────

export type EvSettings = {
  opportunistic_charging_enabled: boolean;
  /** Derived: true while any EvSession is active (auto-pause). */
  paused_by_active_session: boolean;
};

// ─── EV usage simulation / forecast (ev-usage-simulation, ev-usage-forecast) ──

/** Which usage class the EV profile declared: `simulated` = physics only
 *  (`usage_sim`), `forecast` = the same schedule also fed to the planner as
 *  per-slot availability (`usage_forecast`). */
export type EvUsageMode = "simulated" | "forecast";

export type EvUsageSimNextTrip = {
  leave_at: string;
  return_at: string;
  expected_soc_drop_pct: number;
};

/** GET /ev-usage-sim response — undefined/204 when the EV has no usage
 * schedule configured (the vast majority of profiles; this is opt-in). Serves
 * both usage classes, distinguished by `mode`. */
export type EvUsageSimState = {
  mode: EvUsageMode;
  engage_charge_planning: boolean;
  next_trip: EvUsageSimNextTrip | null;
};

export type UpdateEvSettingsBody = {
  opportunistic_charging_enabled: boolean;
};

/** The arbiter's two passes (GET/PUT /arbiter-settings): deviation
 * correction (default off) and limit enforcement (GB-47, default on). */
export type ArbiterSettings = {
  deviation_arbiter_enabled: boolean;
  limit_enforcement_enabled: boolean;
};

/** Each toggle optional — one switch changes without restating the other. */
export type UpdateArbiterSettingsBody = {
  deviation_arbiter_enabled?: boolean;
  limit_enforcement_enabled?: boolean;
};

/** The limit-enforcement pass's last tick (controller::arbiter::limit). */
export type LimitPassOutcome = {
  target_kw: number;
  excess_kw: number;
  unresolved_kw: number;
  active_lever: string | null;
  heater_emergency_mode: [boolean, boolean] | null;
  adjusted_kw_by_asset: Record<string, number>;
  /** The battery's share that only bridges a lagging command (kW, R-104). */
  battery_bridge_kw: number;
};

/** Last tick's arbiter reasoning (GET /arbiter-diagnostics) — null fields
 * before the arbiter has run this process, or during the no-plan-yet
 * startup window. `net_kw`/`dev_kw`/`active_lever`/`unresolved_kw` are the
 * deviation pass's; `limit` is null while limit enforcement is off or no hard
 * import limit is in force. */
export type ArbiterDiagnostics = {
  net_kw: number | null;
  dev_kw: number | null;
  /** The deviation with battery/EV back at plan: a held correction is released once this is
   * inside the 0.1 kW dead band (R-88). */
  dev_without_correction_kw: number | null;
  active_lever: string | null;
  unresolved_kw: number;
  measured_net_kw: number | null;
  limit: LimitPassOutcome | null;
  /** The battery's share of its last command that only bridged a lagging command, e.g. the
   * EV charger applying a cut a tick late (kW, both passes) — never carried as a correction
   * into the next tick (R-104). */
  battery_bridge_kw: number;
  updated_at: string | null;
};

export type BaselineSlot = { slot_start: string; add_kw: number };

export type BaselineOverride = {
  id: string;
  slots: BaselineSlot[];
  created_at: string;
  updated_at: string;
};

export type CreateBaselineOverrideBody = { slots: BaselineSlot[] };

/** BL-43: live site-level headroom, `GET /flexibility` — instant-only, no forward
 * schedule (distinct from the per-device `FlexibilityEnvelope` below). `up_kw`/
 * `down_kw` are SIGNED (`site-capacity-seam-unification`, positive = import,
 * negative = export) — see `SiteHeadroomChart.tsx`'s own doc comment. */
export type SiteFlexibilityEnvelope = {
  ts: string;
  up_kw: number;
  down_kw: number;
  up_duration_s: number | null;
  down_duration_s: number | null;
};

/** One historical sample of `SiteFlexibilityEnvelope`, `GET /flexibility/history`. */
export type SiteFlexibilitySample = {
  ts: string;
  up_kw: number;
  down_kw: number;
};

/** One forward-looking per-slot forecast sample, `GET /flexibility/forecast` —
 * re-derived fresh every dispatcher tick from the active plan's own setpoint
 * schedule, distinct from both `SiteFlexibilityEnvelope` (instant-only) and
 * `SiteFlexibilitySample` (the past ring). */
export type SiteFlexibilityForecastSlot = {
  ts: string;
  up_kw: number;
  down_kw: number;
};

/** One step in a `CapacityCurve` — power holds constant from `elapsed_s` until
 * the next step (or the curve's end), a step function, not interpolated. */
export type CapacityCurveStep = {
  elapsed_s: number;
  power_kw: number;
};

/** A closed-form all-in capability forecast — power/duration/energy — for one
 * direction, `GET /flexibility/capacity` — "if the site committed now to
 * sustained max import (or export), how does achievable power step down over
 * elapsed time." Distinct from `SiteFlexibilityForecastSlot` above (which is
 * an independent per-instant counterfactual, not a duration/energy curve).
 * Not cached or binding across ticks — re-derived fresh every dispatcher
 * tick, same as the other flexibility signals. */
export type CapacityCurve = {
  direction: "import" | "export";
  start: string;
  steps: CapacityCurveStep[];
};

/** `GET /flexibility/capacity` response body — both directions in one
 * response (mirrors `SiteHeadroomChart`'s up/down pairing convention).
 * `start` is the instant both curves are anchored at: the per-tick "now", or
 * for a `?start=` request the plan slot the requested start snapped down to
 * (or "now" again when there was no plan slot to anchor at). */
export type CapacityCurvesResponse = {
  start: string;
  import: CapacityCurve;
  export: CapacityCurve;
};

// ─── Flexibility Envelope ─────────────────────────────────────────────────────

// ─── Timeline zones ───────────────────────────────────────────────────────────

/** A planning zone returned by GET /timeline/all. */
// Lives in the shared chart package now (both UIs shade time bands the same
// way); re-exported here so every existing importer is unaffected.
export type { ZoneDef } from "@lab/charts/types";

// ─── Planner SSE events (Plan E) ──────────────────────────────────────────────

// ─── Persistent history (Phase 1, WP1.4/WP1.5) ────────────────────────────────
// Field names pass through the VEN's own snake_case wire format verbatim
// (no DTO renaming), except `ts`/`received_at`/`sent_at` which the client
// converts from ISO string to epoch ms, same as the /timeline/* client methods.

export type HistoryTickSample = {
  ts: number;
  asset_id: string;
  power_kw: number;
  soc_pct: number | null;
  temperature_c: number | null;
  /** Fraction of the minute the asset reported itself plugged in (0..1, 1 = plugged).
   * `null` for assets with no plugged state and for rows written before the column existed. */
  plugged: number | null;
  /** PV generation limit in force during the minute (kW, negative), null when none was. */
  generation_limit_kw: number | null;
  /** Source of that limit as the live timeline's code (1 plan, 2 capacity, 3 arbiter,
   * 4 manual, 5 comms-loss); null iff `generation_limit_kw` is. */
  curtailment_source: number | null;
};

export type HistoryGridSample = {
  ts: number;
  import_kw: number;
  export_kw: number;
  import_tariff_eur_kwh: number | null;
  export_tariff_eur_kwh: number | null;
  co2_g_kwh: number | null;
  /** Dynamic Operating Envelope import limit (kW) — tightest value observed in this window,
   * null if no capacity event was applicable during it (history-envelope-persistence). */
  import_limit_kw: number | null;
  export_limit_kw: number | null;
  /** site-headroom-forecast Piece 3: mean instant site-flexibility headroom over this
   * window (SCHEMA_V11), null before that migration recorded a dispatcher tick. */
  up_kw: number | null;
  down_kw: number | null;
};

export type ForecastAccuracySample = {
  asset_id: string;
  lead_kind: "near" | "far";
  target_ts: number;
  predicted_kw: number;
  predicted_at: number;
  actual_kw: number | null;
  actual_at: number | null;
};

export type HistoryEventReceived = {
  received_at: number;
  event_id: string;
  event_type: string;
  payload_json: string;
};

export type HistoryReportSent = {
  sent_at: number;
  report_type: string;
  event_id: string;
  payload_json: string;
};

/** One page of a paginated history query: `rows` capped at the requested
 * `limit`, plus `total` — the full count matching the `[from, to)` filter
 * regardless of paging. Mirrors `HistoryPage<T>` in VEN/src/entities/history.rs. */
export type HistoryPage<T> = {
  rows: T[];
  total: number;
};

export type ReportObligation = {
  id: string;
  event_id: string;
  program_id: string | null;
  payload_type: string;
  reading_type: string;
  resource_name: string | null;
  due_at: string;
  interval_duration_s: number;
  fulfilled: boolean;
  created_at: string;
  historical: boolean;
};

// ── Weather forecast plugin (weather-forecast-visibility) ────────────────────
// Field names pass through the Rust WeatherResponse/WeatherForecast wire shape
// verbatim (see VEN/src/routes/weather.rs, entities/weather.rs).

export type SkyCondition =
  | "clear"
  | "mostly_clear"
  | "partly_cloudy"
  | "overcast"
  | "fog"
  | "rain"
  | "sleet"
  | "snow"
  | "thunderstorm"
  | "unknown";

export type GeoPosition = { latitude_deg: number; longitude_deg: number };

export type WeatherForecastSample = {
  valid_at: string;
  age_h: number;
  temperature_c: number;
  ghi_w_m2: number;
  wind_speed_kmh: number | null;
  rain_prob_pct: number | null;
  new_snowfall_cm: number | null;
  sky_condition: SkyCondition | null;
  irradiance_variability: number | null;
};

export type WeatherForecast = {
  source_id: string;
  location: GeoPosition;
  fetched_at: string;
  samples: WeatherForecastSample[];
};

export type WeatherPvForecastSlot = {
  valid_at: string;
  forecast_ac_kw: number;
  snow_covered: boolean;
  /** What the weather predicts with no snow on the panel: the reference the real output is compared with. */
  snow_free_ac_kw: number;
  /** Whether the air this hour is cold enough for snow to lie. */
  snow_possible: boolean;
};

export type WeatherStatus = "ok" | "stale" | "no_forecast";

export type WeatherResponse = {
  status: WeatherStatus;
  is_fresh: boolean;
  /** Transport heartbeat: whether the configured source has been heard from recently. */
  source_alive: boolean;
  raw: WeatherForecast | null;
  derived: WeatherPvForecastSlot[] | null;
  /** The PV asset concludes its panels are snow-covered right now (from its output vs the weather). */
  pv_snow_covered_now: boolean;
};

// ── Real-measurement MQTT feeds (real-measurement-mqtt) ───────────────────────
// Field names pass through the Rust MeasurementResponse wire shape verbatim
// (see VEN/src/routes/measurement.rs).

export type MeasurementStatus = "ok" | "stale" | "disabled" | "not_configured";

export type MeasurementSignal = {
  status: MeasurementStatus;
  is_fresh: boolean;
  /** Transport heartbeat: whether the configured source has been heard from recently. */
  source_alive: boolean;
  raw_kw: number | null;
  raw_at: string | null;
};

export type MeasurementResponse = {
  pv: MeasurementSignal;
  base_load: MeasurementSignal;
};
