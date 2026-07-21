// WP-T1 (docs/plans/ven-ui-transparency.md): componentised health, replacing the
// previous plain "ok" string GET /health used to return.
export type HealthComponentStatus = { status: "ok" | "degraded"; detail?: string };

export type HealthResponse = {
  status: "ok" | "degraded";
  components: {
    ven_process: HealthComponentStatus;
    vtn_connection: HealthComponentStatus;
    storage: HealthComponentStatus;
    planner: HealthComponentStatus;
  };
};

export type VtnStatus = {
  connected: boolean;
  last_success_ts: string | null;
  last_error: string | null;
  current_backoff_s: number;
  token_expires_at: string | null;
};

// WP-T3 (docs/plans/ven-ui-transparency.md): per-task restart/outcome status.
export type TaskStatusEntry = {
  name: string;
  last_run_ts: string | null;
  last_success: boolean | null;
  restart_count: number;
};

// WP-T4 (docs/plans/ven-ui-transparency.md): VEN-operational Event Log entry,
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
  programLongName?: string | null;
  programType?: string | null;
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
  intervalPeriod?: IntervalPeriod | null;
  targets?: TargetEntry[] | null;
  createdDateTime?: string | null;
  intervals?: Interval[];
  [key: string]: unknown;
};

export type Report = {
  id: string;
  programID?: string | null;
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

export type SensorSnapshot = {
  id: string;
  ts: string;
  temperature_c?: number | null;
  power_w?: number | null;
  voltage_v?: number | null;
  raw: unknown;
};

export type AssetSnapshot = {
  power_kw: number;
  [key: string]: number | null;
};

export type GridSnapshot = {
  net_power_w: number;
  voltage_v: number;
  import_kwh: number;
  export_kwh: number;
};

export type SimSnapshot = {
  ts: string;
  grid: GridSnapshot;
  assets: Record<string, AssetSnapshot>;
};

export type Setpoints = {
  ev_charge_kw: number;
  heater_kw: number;
  pv_export_limit_kw: number | null; // active export cap (kW); null = no limit
  mode: string;
};

/** Simulation injection state — maps to /sim/inject backend endpoint. */
export type SimInjectState = {
  // Behaviour A: one-shot jumps (auto-cleared after application)
  battery_soc?: number | null;
  ev_soc?: number | null;
  heater_temp_c?: number | null;
  // Behaviour B: frozen + EMA blend-back on release
  pv_irradiance?: number | null;
  pv_irradiance_alpha?: number;
  base_load_kw?: number | null;
  base_load_alpha?: number;
  // Behaviour C: frozen while active, snap to profile on release
  ev_plugged?: boolean | null;
  ev_soc_target?: number | null;
  heater_setpoint_c?: number | null;
  heater_temp_min_c?: number | null;
  heater_temp_max_c?: number | null;
  ambient_temp_c?: number | null;
  grid_import_limit_kw?: number | null;
  grid_export_limit_kw?: number | null;
};

export type TraceEntry =
  | { type: "OpenAdrArrived";   ts: string; event_name: string; signal_type: string; value: number; interval: number }
  | { type: "OpenAdrExpired";   ts: string; event_name: string }
  | { type: "RateChange";       ts: string; interval_start: string; import_eur_kwh: number; export_eur_kwh: number }
  | { type: "CapacityChange";   ts: string; import_limit_kw: number | null; export_limit_kw: number | null }
  | { type: "PlanCycle";        ts: string; trigger_reason: string; total_slots: number }
  | { type: "RequestTransition"; ts: string; request_id: string; asset_id: string; from_status: string; to_status: string }
  | { type: "DispatchOverride";  ts: string; setpoint_kw: number | null; active: boolean };

// ─── Sim schema types ─────────────────────────────────────────────────────────

export type ControlKind = "slider" | "switch" | "number_input";

/** Descriptor for one controllable parameter from GET /sim/schema. */
export type ControlDescriptor = {
  key: string;
  label: string;
  kind: ControlKind;
  min: number | null;
  max: number | null;
  unit: string;
  /** Multiply raw value by this for display; divide on send. e.g. 100 renders 0.8 as "80 %" */
  display_scale?: number;
};

// ─── HEMS Controller types ────────────────────────────────────────────────────

export type TariffSnapshot = {
  interval_start: string;
  import_tariff_eur_kwh: number | null;
  export_tariff_eur_kwh: number | null;
  co2_g_kwh: number | null;
};

/** @deprecated Renamed to TariffSnapshot. Kept as alias for backward compat with Controller.tsx. */
export type RateSnapshot = TariffSnapshot;

export type PlannedRates = TariffSnapshot[];

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

export type AssetAllocation = {
  asset_id: string;
  power_kw: number;
  surplus_power_kw: number;
  grid_power_kw: number;
  marginal_value: number;
  cost_eur: number;
  co2_g: number;
};

export type PlanTimeSlot = {
  slot_index: number;
  start: string;
  end: string;
  import_tariff_eur_kwh: number;
  /** WP4.4: true when the rate was filled by the StaleRatePolicy (beyond tariff coverage). */
  rate_estimated?: boolean;
  export_tariff_eur_kwh: number;
  co2_g_kwh: number;
  import_cap_kw: number;
  export_cap_kw: number;
  allocations: AssetAllocation[];
  net_import_kw: number;
  net_export_kw: number;
  pv_forecast_kw: number;
  baseline_kw: number;
  planned_kw_by_asset?: Record<string, number>;
};

export type PlanSummary = {
  total_cost_eur: number;
  total_co2_g: number;
  total_import_kwh: number;
  total_export_kwh: number;
};

export type PlannerObjective =
  | "min_cost"
  | "min_ghg"
  | "min_grid"
  | "min_import"
  | "max_revenue";

export type SolveStatus = "OPTIMAL" | "INFEASIBLE";

export type Plan = {
  id: string;
  created_at: string;
  trigger: string;
  objective?: PlannerObjective;
  slots: PlanTimeSlot[];
  summary: PlanSummary;
  envelopes: FlexibilityEnvelope[];
  warnings: Array<{ severity: string; message: string; suggested_action: string | null }>;
  objective_eur: number;
  friction_eur: number;
  solve_status: SolveStatus;
};

export type AssetLedger = {
  asset_id: string;
  energy_kwh: number;
  cost_eur: number;
  co2_g: number;
  updated_at: string | null;
  started_at: string | null;
};

export type UserRequestStatus = "ACTIVE" | "COMPLETED" | "CANCELLED" | "FAILED";

/** WP4.2 (BL-19): one point of a comfort/value curve (domain passthrough). */
export type ComfortRate = {
  /** Task completion fraction 0.0–1.0. */
  fill: number;
  /** Max €/kWh the user bids at this fill level. */
  max_marginal_price: number;
  /** Max gCO2/kWh the user accepts at this fill level. */
  max_marginal_co2: number;
};

/** WP4.2: GET /assets/:id/comfort_curve response. */
export type ComfortCurveResponse = {
  source: "default" | "override";
  rates: ComfortRate[];
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

/** WP4.6: GET /signals — one-round-trip aggregate for the grid-signal strip. */
export type SignalsState = {
  alerts: AlertWindow[];
  simple: SimpleWindow[];
  dispatch: DispatchWindow[];
  capacity: OadrCapacityState;
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

/** How the user expressed the request (BL-28). Omitted = BY_DEADLINE (legacy). */
export type UserRequestMode =
  | "ASAP"
  | "ASAP_FREE"
  | "BY_DEADLINE"
  | "BY_DEADLINE_FREE"
  | "MAX_COST"
  | "OPPORTUNISTIC";

export type SessionType = "ev" | "heater" | "shiftable_load";

export type UserRequest = {
  id: string;
  asset_id: string;
  target_energy_kwh: number;
  target_soc: number | null;
  desired_power_kw: number;
  completion_policy: string;
  deadlines: Array<{
    latest_end: string;
    max_total_cost_eur: number | null;
    max_marginal_rate_eur_kwh: number | null;
    min_completion: number;
  }>;
  mode: UserRequestMode;
  max_total_cost_eur: number | null;
  tier_count: number;
  session_id: string | null;
  session_type: SessionType | null;
  status: UserRequestStatus;
  estimated_cost_eur: number;
  estimated_co2_g: number;
  interruptible: boolean;
  tolerance_min: number | null;
  budget_eur: number | null;
  created_at: string;
  updated_at: string;
};

export type SessionDetail =
  | { type: "ev" } & EvSession
  | { type: "heater" } & HeaterTarget
  | { type: "shiftable_load" } & ShiftableLoad;

export type UserRequestWithSession = UserRequest & {
  session: SessionDetail | null;
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
  // WM shiftable-load fields (optional)
  power_kw?: number;
  duration_min?: number;
  earliest_start?: string;
  latest_end?: string;
  // Per-device overrides (Plan D)
  soft_deadline?: boolean;
  target_temp_c?: number;
  // Request mode (BL-28); omitted = BY_DEADLINE
  mode?: UserRequestMode;
  // MAX_COST (WP4.1-c): total charging-cost ceiling in €
  budget_eur?: number;
};

// ─── Device Session types ─────────────────────────────────────────────────────

export type EvSession = {
  id: string;
  target_soc: number;
  departure_time: string;
  /** When true, MILP treats charging as a soft reward (best-effort). Default false = must reach target by departure. */
  soft_deadline: boolean;
  mode: UserRequestMode;
  /** MAX_COST (WP4.1-c): total charging-cost ceiling in €, null otherwise. */
  budget_eur: number | null;
  created_at: string;
  updated_at: string;
};

export type CreateEvSessionBody = {
  target_soc: number;
  departure_time: string;
  soft_deadline?: boolean;
  mode?: UserRequestMode;
  budget_eur?: number;
};

export type EvSettings = {
  opportunistic_charging_enabled: boolean;
  /** Derived: true while any EvSession is active (auto-pause). */
  paused_by_active_session: boolean;
};

export type UpdateEvSettingsBody = {
  opportunistic_charging_enabled: boolean;
};

export type HeaterTarget = {
  id: string;
  target_temp_c: number;
  ready_by: string;
  mode: UserRequestMode;
  created_at: string;
  updated_at: string;
};

export type CreateHeaterTargetBody = {
  target_temp_c: number;
  ready_by: string;
};

export type ShiftableLoad = {
  id: string;
  asset_id: string;
  power_kw: number;
  duration_min: number;
  earliest_start: string;
  latest_end: string;
  mode: UserRequestMode;
  created_at: string;
  updated_at: string;
};

export type CreateShiftableLoadBody = {
  asset_id: string;
  power_kw: number;
  duration_min: number;
  earliest_start: string;
  latest_end: string;
};

export type BaselineSlot = { slot_start: string; add_kw: number };

export type BaselineOverride = {
  id: string;
  slots: BaselineSlot[];
  created_at: string;
  updated_at: string;
};

export type CreateBaselineOverrideBody = { slots: BaselineSlot[] };

// ─── Flexibility Envelope ─────────────────────────────────────────────────────

/** Per-device schedulability metadata emitted at plan time (VEN/src/entities/plan.rs FlexibilityEnvelope). */
export type FlexibilityEnvelope = {
  asset_id: string;
  energy_needed_kwh: number;
  power_min_kw: number;
  power_max_kw: number;
  window_start: string;
  window_end: string;
  slots_available: number;
  max_acceptable_rate: number;
  min_acceptable_rate: number;
  budget_remaining_eur: number;
  estimated_cost_eur: number;
  estimated_co2_g: number;
};

// ─── Timeline zones ───────────────────────────────────────────────────────────

/** A planning zone returned by GET /timeline/all. */
export type ZoneDef = { from: string; to: string; step_s: number };

// ─── Planner SSE events (Plan E) ──────────────────────────────────────────────

export type PlannerEvent =
  | { type: "solving_started"; objective: PlannerObjective; num_slots: number; triggered_at: string }
  | { type: "solving_progress"; elapsed_ms: number; iteration: number }
  | { type: "plan_ready"; plan_id: string; objective: PlannerObjective; solver_ms: number; objective_eur: number; friction_eur: number; solve_status: SolveStatus; slot_count: number; trigger: string }
  | { type: "correction_active"; ts: string; asset_id: string; reason: string;
      planned_net_kw: number; actual_net_kw: number; deviation_kw: number;
      correction_kw: number; objective: PlannerObjective }
  | { type: "correction_cleared"; ts: string; reason: string };

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
};

export type HistoryGridSample = {
  ts: number;
  import_kw: number;
  export_kw: number;
  import_tariff_eur_kwh: number | null;
  export_tariff_eur_kwh: number | null;
  co2_g_kwh: number | null;
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

// WP-T6 (docs/plans/ven-ui-transparency.md): wiring previously-unused routes.

export type PlanSnapshot = {
  created_at: number;
  horizon_start: string;
  horizon_end: string;
  plan_json: string;
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

export type AssetCapability = {
  max_import_kw: number;
  max_export_kw: number;
  is_fixed: boolean;
};

export type ForecastSource =
  | "WEATHER_MODEL"
  | "DEVICE_CLOUD"
  | "PHYSICAL_MODEL"
  | "HEURISTIC"
  | "MANUAL"
  | "OPTIMIZATION"
  | "NONE";

export type AssetForecast = {
  asset_id: string;
  updated_at: string;
  source: ForecastSource;
  confidence: number;
  power_kw: number[];
  soc: number[] | null;
  availability_windows: Array<{ start: string; end: string }> | null;
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
};

export type WeatherStatus = "ok" | "stale" | "no_forecast";

export type WeatherResponse = {
  status: WeatherStatus;
  is_fresh: boolean;
  raw: WeatherForecast | null;
  derived: WeatherPvForecastSlot[] | null;
};
