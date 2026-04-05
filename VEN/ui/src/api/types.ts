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
  | { type: "PlanCycle";        ts: string; trigger_reason: string; firm_slots: number; flexible_slots: number }
  | { type: "PacketTransition"; ts: string; packet_id: string; asset_id: string; from_status: string; to_status: string }
  | { type: "RequestTransition"; ts: string; request_id: string; asset_id: string; from_status: string; to_status: string };

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
  import_limit_event_id: string | null;
  export_limit_event_id: string | null;
  last_updated: string | null;
};

export type PacketStatus =
  | "PENDING" | "SCHEDULED" | "ACTIVE" | "PAUSED"
  | "COMPLETED" | "PARTIAL_COMPLETED" | "ABANDONED" | "FAILED";

export type PacketAllocation = {
  packet_id: string;
  asset_id: string;
  power_kw: number;
  cost_eur: number;
  co2_g: number;
};

export type PlanTimeSlot = {
  slot_index: number;
  start: string;
  end: string;
  slot_type: "FIRM" | "FLEXIBLE";
  import_tariff_eur_kwh: number;
  export_tariff_eur_kwh: number;
  co2_g_kwh: number;
  import_cap_kw: number;
  export_cap_kw: number;
  allocations: PacketAllocation[];
  net_import_kw: number;
  net_export_kw: number;
  pv_forecast_kw: number;
  baseline_kw: number;
};

export type EnergyPacket = {
  id: string;
  asset_id: string;
  status: PacketStatus;
  target_energy_kwh: number;
  target_soc: number | null;
  desired_power_kw: number;
  estimated_cost_eur: number;
  estimated_co2_g: number;
  estimated_completion: number;
  accumulated_cost_eur: number;
  value_curve: {
    deadline_tiers: Array<{
      deadline: string;
      max_total_cost_eur: number | null;
      min_completion: number;
    }>;
    active_tier_index: number;
  };
  created_at: string;
  updated_at: string;
};

export type FirmSummary = {
  total_cost_eur: number;
  total_co2_g: number;
  total_import_kwh: number;
  total_export_kwh: number;
};

// ─── Planner decision audit trail ────────────────────────────────────────────

export type PlanReason =
  | { kind: "IDLE" }
  | { kind: "CHEAP_TARIFF";       tariff_eur_per_kwh: number; threshold_eur_per_kwh: number }
  | { kind: "EXPENSIVE_TARIFF";   tariff_eur_per_kwh: number; threshold_eur_per_kwh: number }
  | { kind: "FIRM_OBLIGATION";    source: unknown; required_kw: number }
  | { kind: "USER_OVERRIDE";      request_id: string; mode: string }
  | { kind: "SOC_CEILING";        soc_pct: number }
  | { kind: "SOC_FLOOR";          soc_pct: number }
  | { kind: "COMFORT_BOUND";      asset_id: string; bound_type: string }
  | { kind: "GRID_IMPORT_LIMIT";  limit_kw: number }
  | { kind: "GRID_EXPORT_LIMIT";  limit_kw: number }
  | { kind: "POLICY_RESERVE";     policy_id: string }
  | { kind: "OPPORTUNITY_MISSED"; reason: string }
  | { kind: "SURPLUS_ABSORPTION"; surplus_kw: number };

export type PlanStep = {
  ts: string;
  asset_id: string;
  setpoint_kw: number;
  actual_power_kw: number;
  reason: PlanReason;
  /** AssetState enum serialized as `{ asset_type: string; actual_power_kw: number; … }` */
  state_before: { asset_type: string; actual_power_kw: number; [key: string]: unknown };
  avail_max_import_kw: number;
  avail_max_export_kw: number;
};

export type Plan = {
  id: string;
  created_at: string;
  trigger: string;
  firm_boundary: string;
  firm_slots: PlanTimeSlot[];
  flexible_slots: PlanTimeSlot[];
  firm_summary: FirmSummary;
  warnings: Array<{ severity: string; message: string; packet_id: string | null; suggested_action: string | null }>;
  steps: PlanStep[];
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
    min_completion: number;
  }>;
  packet_id: string;
  status: UserRequestStatus;
  estimated_cost_eur: number;
  estimated_co2_g: number;
  created_at: string;
  updated_at: string;
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
};

export type FlexibilityEnvelope = {
  packet_id: string;
  asset_id: string;
  energy_needed_kwh: number;
  power_min_kw: number;
  power_max_kw: number;
  window_start: string;
  window_end: string;
  max_acceptable_rate: number;
  estimated_cost_eur: number;
};
