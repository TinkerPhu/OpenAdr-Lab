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

export type EvSnapshot = {
  soc: number;
  plugged: boolean;
  current_kw: number;
  max_charge_kw: number;
  soc_target: number;
  battery_kwh: number;
};

export type HeaterSnapshot = {
  temp_c: number;
  current_kw: number;
  max_kw: number;
  temp_min_c: number;
  temp_max_c: number;
};

export type PvSnapshot = {
  irradiance: number;
  export_limit_kw: number | null; // active export cap (kW); null = no limit
  current_kw: number;
  rated_kw: number;
};

export type BatterySnapshot = {
  soc: number;
  current_kw: number;      // positive = charging (import), negative = discharging (export)
  capacity_kwh: number;
  max_charge_kw: number;
  max_discharge_kw: number;
  min_soc: number;
};

export type AssetSnapshot = {
  power_kw: number;
  [key: string]: number;
};

export type SimSnapshot = {
  ts: string;
  net_power_w: number;
  import_w: number;
  export_w: number;
  voltage_v: number;
  import_kwh: number;
  export_kwh: number;
  /** Generic per-asset map (new format). */
  assets: Record<string, AssetSnapshot>;
  /** Backward-compat derived fields. */
  base_load_w: number;
  ev?: EvSnapshot | null;
  heater?: HeaterSnapshot | null;
  pv?: PvSnapshot | null;
  battery?: BatterySnapshot | null;
};

export type Setpoints = {
  ev_charge_kw: number;
  heater_kw: number;
  pv_export_limit_kw: number | null; // active export cap (kW); null = no limit
  mode: string;
};

export type UserOverrides = {
  pv_irradiance?: number;
  ambient_temp_c?: number;
  ev_desired_kw?: number;
  ev_plugged?: boolean;
  ev_force_kw?: number;
  heater_force_kw?: number;
  battery_force_kw?: number;
  pv_force_export_limit_kw?: number;
  ev_max_charge_kw?: number;
  ev_soc_target?: number;
  heater_max_kw?: number;
  heater_temp_min_c?: number;
  heater_temp_max_c?: number;
  pv_rated_kw?: number;
  base_load_w?: number;
};

export type TraceEntry = {
  ts: string;
  mode: string;
  fsm_state: string;
  active_events: string[];
  winning_intent?: string | null;
  setpoints: Setpoints;
  constraints: string[];
  reason: string;
};

// ─── Sim schema types ─────────────────────────────────────────────────────────

export type ControlKind = "Slider" | "Switch" | "NumberInput";

/** Descriptor for one controllable parameter from GET /sim/schema. */
export type ControlDescriptor = {
  key: string;
  label: string;
  kind: ControlKind;
  min: number | null;
  max: number | null;
  unit: string;
};

// ─── HEMS Controller types ────────────────────────────────────────────────────

export type RateSnapshot = {
  interval_start: string;
  interval_end: string;
  import_price_eur_kwh: number | null;
  export_price_eur_kwh: number | null;
  co2_g_kwh: number | null;
  source_event_id: string | null;
  is_forecast: boolean;
};

export type PlannedRates = RateSnapshot[];

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
  import_price_eur_kwh: number;
  export_price_eur_kwh: number;
  co2_g_kwh: number;
  import_cap_kw: number;
  export_cap_kw: number;
  allocations: PacketAllocation[];
  net_import_kw: number;
  net_export_kw: number;
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

export type Plan = {
  id: string;
  created_at: string;
  trigger: string;
  firm_slots: PlanTimeSlot[];
  flexible_slots: PlanTimeSlot[];
  firm_summary: FirmSummary;
  warnings: Array<{ severity: string; message: string; packet_id: string | null }>;
};

export type AssetLedger = {
  asset_id: string;
  energy_kwh: number;
  cost_eur: number;
  co2_g: number;
  updated_at: string | null;
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
