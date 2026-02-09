export type HealthStatus = {
  time: string;
  bff: { ok: boolean; version: string };
  vtn: { reachable: boolean; authOk: boolean };
};

export type TargetEntry = {
  type: string;
  values: string[];
};

export type ProgramDescription = {
  URL: string;
};

export type Program = {
  id: string;
  programName: string | null;
  programLongName?: string | null;
  programType?: string | null;
  programDescriptions?: ProgramDescription[] | null;
  targets?: TargetEntry[] | null;
  createdDateTime: string | null;
};

export type IntervalPeriod = {
  start: string;
  duration?: string | null;
};

export type VtnEvent = {
  id: string;
  programID: string | null;
  eventName: string | null;
  priority?: number | null;
  intervalPeriod?: IntervalPeriod | null;
  targets?: TargetEntry[] | null;
  createdDateTime: string | null;
  intervals: unknown;
};

export type Ven = {
  id: string;
  venName: string | null;
  createdDateTime: string | null;
};

export type ProgramInput = {
  programName: string;
  programLongName?: string | null;
  programType?: string | null;
  programDescriptions?: ProgramDescription[] | null;
  targets?: TargetEntry[] | null;
};

export type EventInput = {
  programID: string;
  eventName: string;
  priority?: number | null;
  intervalPeriod?: IntervalPeriod | null;
  targets?: TargetEntry[] | null;
  intervals: unknown[];
};

export type Report = {
  id: string;
  programID: string | null;
  eventID: string | null;
  clientName: string | null;
  reportName?: string | null;
  resources: unknown;
  createdDateTime: string | null;
};
