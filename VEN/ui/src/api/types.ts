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
};

export type HeaterSnapshot = {
  temp_c: number;
  current_kw: number;
  max_kw: number;
};

export type PvSnapshot = {
  irradiance: number;
  curtailment: number;
  current_kw: number;
  rated_kw: number;
};

export type SimSnapshot = {
  ts: string;
  net_power_w: number;
  import_w: number;
  export_w: number;
  voltage_v: number;
  base_load_w: number;
  import_kwh: number;
  export_kwh: number;
  ev?: EvSnapshot | null;
  heater?: HeaterSnapshot | null;
  pv?: PvSnapshot | null;
};

export type Setpoints = {
  ev_charge_kw: number;
  heater_kw: number;
  pv_curtailment: number;
  mode: string;
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
