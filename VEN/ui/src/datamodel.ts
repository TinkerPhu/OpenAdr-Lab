export type Program = { id: string; name?: string | null };

export type Event = {
  id: string;
  program_id?: string | null;
  created_at?: string | null;
  status?: string | null;
  raw: any;
};

export type SensorSnapshot = {
  id: string;
  ts: string;
  temperature_c?: number | null;
  power_w?: number | null;
  voltage_v?: number | null;
  raw: any;
};
