export type Program = {
  id: string;
  programName?: string | null;
  createdDateTime?: string | null;
  [key: string]: unknown;
};

export type VtnEvent = {
  id: string;
  programID?: string | null;
  eventName?: string | null;
  createdDateTime?: string | null;
  intervals?: unknown;
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
