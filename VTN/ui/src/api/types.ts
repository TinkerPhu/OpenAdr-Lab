export type HealthStatus = {
  time: string;
  bff: { ok: boolean; version: string };
  vtn: { reachable: boolean; authOk: boolean };
};

export type Program = {
  id: string;
  programName: string | null;
  createdDateTime: string | null;
};

export type VtnEvent = {
  id: string;
  programID: string | null;
  eventName: string | null;
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
};

export type EventInput = {
  programID: string;
  eventName: string;
  intervals: unknown[];
};
