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
 * `netPowerKw` is signed — import positive, export negative — and is `null`
 * when that VEN has not said. Null is not zero: a fleet total that treated
 * silence as "drawing nothing" would be wrong in the way hardest to notice.
 */
export type FleetVenLive = {
  venName: string;
  netPowerKw: number | null;
  state: string | null;
  receivedAt: string;
  /** The VEN's own `objective` leaf (its `GET /plan/objective` body), passed through. */
  objective?: VenObjective | null;
  /** The fleet's command for this VEN, as the broker holds it; null once released. */
  command?: ObjectiveCommand | null;
  /** A command is in force that the VEN does not report applied yet. */
  commandPending?: boolean;
  /** When the BFF saw this VEN's objective last change (not set by the first one seen). */
  objectiveChangedAt?: string | null;
};

/** What a VEN says about its objective (VEN `ObjectiveSetting`, wire names kept). */
export type VenObjective = {
  objective: string;
  source: "profile" | "ven_ui" | "fleet";
  set_at: string | null;
  profile_objective: string;
  available_objectives: string[];
};

/** The body of a VEN's `command/objective` leaf. */
export type ObjectiveCommand = { objective: string; requested_at: string };

/** The part of a VEN's adopted plan the drill-down draws: its `plan` leaf (VEN
 *  `telemetry_port::plan_body`, wire names kept), as the BFF stored it. */
export type VenPlan = {
  objective: string;
  solve_status: string;
  slots: {
    start: string;
    end: string;
    net_import_kw: number;
    net_export_kw: number;
    planned_kw_by_asset: Record<string, number>;
  }[];
};

export type FleetLive = {
  source: "live";
  vens: FleetVenLive[];
  fleet: {
    netPowerKw: number;
    contributingVens: number;
    knownVens: number;
  };
};

export type FleetSample = { ts: string; netPowerKw: number };

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
  /** True when the VEN's own PlanCycle named this event; false when the time
   *  window inferred it. An operator judging whether an event worked needs to
   *  know which of the two they are looking at. */
  replanAttributed: boolean;
  powerBeforeKw: number | null;
  powerAfterKw: number | null;
  deltaKw: number | null;
};

export type FleetReactions = {
  eventID: string;
  vensSeen: number;
  vens: VenReaction[];
};

/**
 * One interval of one event, as it applied to one VEN.
 *
 * `payloadType` is spelled as OpenADR spells it, so a reader can grep from a
 * chart label straight to the wire (`dto`). `value` is null for payload types
 * that carry no number — never 0, which would be a claim about the signal.
 */
export type SignalBand = {
  from: string;
  to: string;
  eventID: string;
  eventName: string | null;
  payloadType: string;
  value: number | null;
  /**
   * Who said what unit `value` is in: the event, its program, or nobody (`PROFILE_DEFAULT`:
   * the lab profile's default was assumed). Null when the type has no unit to declare.
   */
  unitsDeclaredBy?: "EVENT" | "PROGRAM" | "PROFILE_DEFAULT" | null;
  /** Why `value` was not read: the payload declares a unit the profile does not read, so a
   *  VEN drops it too. Null otherwise. */
  refused?: string | null;
};

export type FleetSignals = {
  from: string;
  to: string;
  vens: { venName: string; bands: SignalBand[] }[];
  /** Events the BFF could not read. Surfaced because fewer bands than expected
   *  should not be indistinguishable from a quiet grid. */
  rejectedEvents: number;
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

/** `GET /api/signals/registry`: what a number of this payload type means on the wire. */
export type WireSignalFacts = {
  payloadType: string;
  units: string | null;
  currency: string | null;
};

/** `GET /api/fleet/plan?at=`: what the fleet planned to import and export, as of an instant. */
export type FleetPlanPoint = {
  ts: string;
  netImportKw: number;
  netExportKw: number;
  /** How many VENs this total is built from; a VEN past its plan's horizon is absent, not zero. */
  contributingVens: number;
};

export type FleetPlan = {
  at: string;
  stepSeconds: number;
  points: FleetPlanPoint[];
  /** The plan each contributing VEN was working to at `at`. */
  plans: { venName: string; planId: string; createdAt: string }[];
  /** Known VENs with no plan at `at`. */
  vensWithoutPlan: string[];
};

/** A sum of declared energies [kWh] over the VENs that declared them (`null`: none did). */
export type ServiceTotal = { kwh: number | null; contributingVens: number };

/** `GET /api/fleet/service`: what each asset declared over a window, per asset, per VEN and for the fleet. */
export type FleetServiceReport = {
  unservedEnergyKwh: ServiceTotal;
  carryOverKwh: ServiceTotal;
  vens: {
    venName: string;
    unservedEnergyKwh: number | null;
    carryOverKwh: number | null;
    assets: { assetId: string; unservedEnergyKwh: number | null; carryOverKwh: number | null }[];
  }[];
};

export type FleetService = {
  from: string;
  to: string;
  /** False when the window starts before the raw telemetry horizon: no figure, never a zero. */
  available: boolean;
  reason?: string;
  report?: FleetServiceReport;
};

/** One declared interval of an event, timed but not repeated (`POST /api/signals/preview`, `pattern`). */
export type PatternInterval = {
  start: string;
  end: string;
  payloads: { type: string; value: number | null }[];
};

/** A draft event expanded the way receivers expand it (`POST /api/signals/preview`). */
export type SignalPreview = {
  bands: SignalBand[];
  /** The receivers' expansion cap falls inside the asked-for window. */
  truncated: boolean;
  maxLoopedIntervals: number;
};

export type EventInput = {
  programID: string;
  eventName: string;
  priority?: number | null;
  duration?: string | null;
  intervalPeriod?: IntervalPeriod | null;
  targets?: Targets | null;
  intervals?: unknown[];
  /** What each payload type's numbers mean (`wire-contracts`). */
  payloadDescriptors?: EventPayloadDescriptor[];
};

export type EventPayloadDescriptor = {
  payloadType: string;
  units?: string | null;
  currency?: string | null;
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
