export type RecorderStatus = {
  enabled: boolean;
  connected: boolean;
  lastPollAt: string | null;
  lastSuccessAt: string | null;
  consecutiveFailures: number;
  lastError: string | null;
};

/**
 * The live fleet feed (fleet-monitor phase 0 §7).
 *
 * `enabled` and `connected` are deliberately separate: a deployment without a
 * broker is a configuration, a configured broker that cannot be reached is a
 * fault, and one boolean cannot tell them apart.
 *
 * `vensKnown` counts every VEN that has ever spoken; `vensOnline` excludes the
 * ones whose last-will has fired. A VEN that died is still known, so the two
 * numbers together say what the feed is worth.
 */
export type FleetStatus = {
  enabled: boolean;
  connected: boolean;
  lastMessageAt: string | null;
  lastError: string | null;
  vensKnown: number;
  vensOnline: number;
};

/**
 * One VEN's latest telemetry, as the BFF last received it.
 *
 * `netPowerW` is signed — import positive, export negative — and is `null`
 * when that VEN has not said. Null is not zero: a fleet total that treated
 * silence as "drawing nothing" would be wrong in the way hardest to notice.
 */
export type FleetVenLive = {
  venName: string;
  netPowerW: number | null;
  state: string | null;
  receivedAt: string;
};

export type FleetLive = {
  source: "live";
  vens: FleetVenLive[];
  fleet: {
    netPowerW: number;
    contributingVens: number;
    knownVens: number;
  };
};

export type FleetSample = { ts: string; netPowerW: number };

/**
 * A window of stored telemetry. `source` says which table answered: `raw` is
 * the published cadence, `rollup` is 1-minute means once the raw rows have
 * aged out. Both are true and they are not the same resolution.
 */
export type FleetHistory = {
  source: "raw" | "rollup";
  from: string;
  to: string;
  stepSeconds: number;
  vens: { venName: string; samples: FleetSample[] }[];
  fleet: (FleetSample & { contributingVens: number })[];
};

/**
 * One VEN's part of an event's reaction chain (§6.3).
 *
 * There is deliberately no "reacted" flag: the numbers are reported and the
 * judgement is the reader's. A threshold decided here would be a second
 * opinion about a site's own behaviour, and wrong differently for every asset
 * mix in the fleet.
 */
export type VenReaction = {
  venName: string;
  seenAt: string;
  seenReceivedAt: string;
  modificationDateTime: string | null;
  replannedAt: string | null;
  powerBeforeW: number | null;
  powerAfterW: number | null;
  deltaW: number | null;
};

export type FleetReactions = {
  eventID: string;
  vensSeen: number;
  vens: VenReaction[];
};

export type HealthStatus = {
  time: string;
  bff: { ok: boolean; version: string };
  vtn: { reachable: boolean; authOk: boolean };
  recorder: RecorderStatus;
  fleet: FleetStatus;
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
