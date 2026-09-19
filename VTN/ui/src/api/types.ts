export type RecorderStatus = {
  enabled: boolean;
  connected: boolean;
  lastPollAt: string | null;
  lastSuccessAt: string | null;
  consecutiveFailures: number;
  lastError: string | null;
};

export type HealthStatus = {
  time: string;
  bff: { ok: boolean; version: string };
  vtn: { reachable: boolean; authOk: boolean };
  recorder: RecorderStatus;
};

/**
 * OpenADR 3.1 targets: a flat list of target strings, replacing 3.0's
 * `{type, values}` entries. Non-optional on the wire and defaulting to `[]`,
 * where an empty list means "visible to every VEN".
 *
 * A VEN matches an object when its own targets (plus its resources') intersect
 * the object's — so the lab gives each VEN its own name as a target, and a
 * program targets `"ven-1"` to reach it.
 */
export type Targets = string[];

export type ProgramDescription = {
  URL: string;
};

export type Program = {
  id: string;
  programName: string | null;
  programDescriptions?: ProgramDescription[] | null;
  /** valuesMap list; carries the lab profile pointer (GB-50). */
  attributes?: unknown[] | null;
  targets?: Targets | null;
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
  /** New in 3.1: an event-level duration, independent of `intervals`. */
  duration?: string | null;
  intervalPeriod?: IntervalPeriod | null;
  targets?: Targets | null;
  createdDateTime: string | null;
  /** Optional in 3.1: an event may declare a duration and no intervals. */
  intervals?: unknown;
};

export type Ven = {
  id: string;
  venName: string | null;
  createdDateTime: string | null;
};

export type ProgramInput = {
  programName: string;
  programDescriptions?: ProgramDescription[] | null;
  attributes?: unknown[] | null;
  targets?: Targets | null;
};

export type EventInput = {
  programID: string;
  eventName: string;
  priority?: number | null;
  duration?: string | null;
  intervalPeriod?: IntervalPeriod | null;
  targets?: Targets | null;
  intervals?: unknown[];
};

export type Report = {
  id: string;
  /** VTN-provisioned in 3.1, identifying the VEN that submitted the report. */
  clientID?: string | null;
  eventID: string | null;
  clientName: string | null;
  reportName?: string | null;
  resources: unknown;
  createdDateTime: string | null;
};
