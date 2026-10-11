// Generated from the Rust types (R-133): `VEN/src/ui_types.rs` lists them and
// `UPDATE_UI_TYPES=1 cargo test ui_types` regenerates them. Do not edit `./generated/`.
import type { ActivePenaltyRule } from "./generated/ActivePenaltyRule";
import type { AlertWindow } from "./generated/AlertWindow";
import type { ArbiterDiagnostics } from "./generated/ArbiterDiagnostics";
import type { ArbiterSettings } from "./generated/ArbiterSettings";
import type { AssetAllocation } from "./generated/AssetAllocation";
import type { AssetCapabilityView } from "./generated/AssetCapabilityView";
import type { AssetForecast } from "./generated/AssetForecast";
import type { AssetLedgerEntry } from "./generated/AssetLedgerEntry";
import type { AssetSnapshot } from "./generated/AssetSnapshot";
import type { BaselineOverride } from "./generated/BaselineOverride";
import type { BaselineSlot } from "./generated/BaselineSlot";
import type { CapacityCurve } from "./generated/CapacityCurve";
import type { CapacityCurveStep } from "./generated/CapacityCurveStep";
import type { CapacityCurves } from "./generated/CapacityCurves";
import type { CapacitySnapshot } from "./generated/CapacitySnapshot";
import type { ComfortCurveResponse } from "./generated/ComfortCurveResponse";
import type { ComfortCurveSource } from "./generated/ComfortCurveSource";
import type { ComfortRate } from "./generated/ComfortRate";
import type { CommitmentDirection } from "./generated/CommitmentDirection";
import type { ControlDescriptor } from "./generated/ControlDescriptor";
import type { ControlKind } from "./generated/ControlKind";
import type { ControllerEvent } from "./generated/ControllerEvent";
import type { CostBreakdown } from "./generated/CostBreakdown";
import type { DispatchWindow } from "./generated/DispatchWindow";
import type { EvSession } from "./generated/EvSession";
import type { EvSessionOrigin } from "./generated/EvSessionOrigin";
import type { EvSettings } from "./generated/EvSettings";
import type { EvUsageMode } from "./generated/EvUsageMode";
import type { EvUsageSimState } from "./generated/EvUsageSimState";
import type { EventLogEntry } from "./generated/EventLogEntry";
import type { EventReceived } from "./generated/EventReceived";
import type { FlexibilityEnvelope } from "./generated/FlexibilityEnvelope";
import type { ForecastAccuracySample } from "./generated/ForecastAccuracySample";
import type { ForecastLeadKind } from "./generated/ForecastLeadKind";
import type { ForecastSource } from "./generated/ForecastSource";
import type { GeoPosition } from "./generated/GeoPosition";
import type { GridSample } from "./generated/GridSample";
import type { GridSnapshot } from "./generated/GridSnapshot";
import type { HealthComponent } from "./generated/HealthComponent";
import type { HealthComponents } from "./generated/HealthComponents";
import type { HealthResponse } from "./generated/HealthResponse";
import type { HeaterTarget } from "./generated/HeaterTarget";
import type { HistoryPage } from "./generated/HistoryPage";
import type { KeyFeature } from "./generated/KeyFeature";
import type { LimitPassOutcome } from "./generated/LimitPassOutcome";
import type { MeasurementResponse } from "./generated/MeasurementResponse";
import type { MeasurementStatus } from "./generated/MeasurementStatus";
import type { NextTrip } from "./generated/NextTrip";
import type { OadrCapacityState } from "./generated/OadrCapacityState";
import type { OadrReportObligation } from "./generated/OadrReportObligation";
import type { Plan } from "./generated/Plan";
import type { PlanHistorySample } from "./generated/PlanHistorySample";
import type { PlanPhaseReport } from "./generated/PlanPhaseReport";
import type { PlanSummary } from "./generated/PlanSummary";
import type { PlanTimeSlot } from "./generated/PlanTimeSlot";
import type { PlanTrigger } from "./generated/PlanTrigger";
import type { PlanWarning } from "./generated/PlanWarning";
import type { PlanZone } from "./generated/PlanZone";
import type { PlannerEvent } from "./generated/PlannerEvent";
import type { PlannerObjective } from "./generated/PlannerObjective";
import type { PlanningHorizon } from "./generated/PlanningHorizon";
import type { PowerAdjustability } from "./generated/PowerAdjustability";
import type { ReportSent } from "./generated/ReportSent";
import type { ReportSubmissionRecord } from "./generated/ReportSubmissionRecord";
import type { ReportWindowSize } from "./generated/ReportWindowSize";
import type { RequestDeadline } from "./generated/RequestDeadline";
import type { ReservationRequest } from "./generated/ReservationRequest";
import type { SensorRaw } from "./generated/SensorRaw";
import type { SensorSnapshot } from "./generated/SensorSnapshot";
import type { SessionDetail } from "./generated/SessionDetail";
import type { SessionType } from "./generated/SessionType";
import type { ShiftableLoad } from "./generated/ShiftableLoad";
import type { SignalResponse } from "./generated/SignalResponse";
import type { SignalsState } from "./generated/SignalsState";
import type { SimInjectState } from "./generated/SimInjectState";
import type { SimSnapshot } from "./generated/SimSnapshot";
import type { SimpleWindow } from "./generated/SimpleWindow";
import type { SiteFlexibilityEnvelope } from "./generated/SiteFlexibilityEnvelope";
import type { SiteFlexibilityForecastSlot } from "./generated/SiteFlexibilityForecastSlot";
import type { SiteFlexibilitySample } from "./generated/SiteFlexibilitySample";
import type { SkyCondition } from "./generated/SkyCondition";
import type { SolveStatus } from "./generated/SolveStatus";
import type { StepRule } from "./generated/StepRule";
import type { TariffSnapshot } from "./generated/TariffSnapshot";
import type { TaskStatusEntry } from "./generated/TaskStatusEntry";
import type { TickSample } from "./generated/TickSample";
import type { TimeRange } from "./generated/TimeRange";
import type { UserNotification } from "./generated/UserNotification";
import type { UserNotificationSeverity } from "./generated/UserNotificationSeverity";
import type { UserRequest } from "./generated/UserRequest";
import type { UserRequestMode } from "./generated/UserRequestMode";
import type { UserRequestStatus } from "./generated/UserRequestStatus";
import type { UserRequestWithSession } from "./generated/UserRequestWithSession";
import type { VtnStatusResponse } from "./generated/VtnStatusResponse";
import type { WarningKind } from "./generated/WarningKind";
import type { WarningSeverity } from "./generated/WarningSeverity";
import type { WeatherForecast } from "./generated/WeatherForecast";
import type { WeatherForecastSample } from "./generated/WeatherForecastSample";
import type { WeatherPvForecastSlot } from "./generated/WeatherPvForecastSlot";
import type { WeatherResponse } from "./generated/WeatherResponse";
import type { WeatherStatus } from "./generated/WeatherStatus";
export type {
  ActivePenaltyRule,
  AlertWindow,
  ArbiterDiagnostics,
  ArbiterSettings,
  AssetAllocation,
  AssetCapabilityView,
  AssetForecast,
  AssetLedgerEntry,
  AssetSnapshot,
  BaselineOverride,
  BaselineSlot,
  CapacityCurve,
  CapacityCurveStep,
  CapacityCurves,
  CapacitySnapshot,
  ComfortCurveResponse,
  ComfortCurveSource,
  ComfortRate,
  CommitmentDirection,
  ControlDescriptor,
  ControlKind,
  ControllerEvent,
  CostBreakdown,
  DispatchWindow,
  EvSession,
  EvSessionOrigin,
  EvSettings,
  EvUsageMode,
  EvUsageSimState,
  EventLogEntry,
  EventReceived,
  FlexibilityEnvelope,
  ForecastAccuracySample,
  ForecastLeadKind,
  ForecastSource,
  GeoPosition,
  GridSample,
  GridSnapshot,
  HealthComponent,
  HealthComponents,
  HealthResponse,
  HeaterTarget,
  HistoryPage,
  KeyFeature,
  LimitPassOutcome,
  MeasurementResponse,
  MeasurementStatus,
  NextTrip,
  OadrCapacityState,
  OadrReportObligation,
  Plan,
  PlanHistorySample,
  PlanPhaseReport,
  PlanSummary,
  PlanTimeSlot,
  PlanTrigger,
  PlanWarning,
  PlanZone,
  PlannerEvent,
  PlannerObjective,
  PlanningHorizon,
  PowerAdjustability,
  ReportSent,
  ReportSubmissionRecord,
  ReportWindowSize,
  RequestDeadline,
  ReservationRequest,
  SensorRaw,
  SensorSnapshot,
  SessionDetail,
  SessionType,
  ShiftableLoad,
  SignalResponse,
  SignalsState,
  SimInjectState,
  SimSnapshot,
  SimpleWindow,
  SiteFlexibilityEnvelope,
  SiteFlexibilityForecastSlot,
  SiteFlexibilitySample,
  SkyCondition,
  SolveStatus,
  StepRule,
  TariffSnapshot,
  TaskStatusEntry,
  TickSample,
  TimeRange,
  UserNotification,
  UserNotificationSeverity,
  UserRequest,
  UserRequestMode,
  UserRequestStatus,
  UserRequestWithSession,
  VtnStatusResponse,
  WarningKind,
  WarningSeverity,
  WeatherForecast,
  WeatherForecastSample,
  WeatherPvForecastSlot,
  WeatherResponse,
  WeatherStatus,
};

// ─── UI-side models derived from a wire type ──────────────────────────────────
// The client converts a few wire fields on arrival (ISO timestamps to epoch ms). Each model is
// stated as its wire type with those fields replaced, so the rest cannot drift.

/** `PlanHistorySample` with `created_at` in epoch ms (`api.historyPlans`). */
export type PlanHistoryRow = Omit<PlanHistorySample, "created_at"> & { created_at: number };
/** `TickSample` with `ts` in epoch ms (`api.historyTicks`). */
export type HistoryTickSample = Omit<TickSample, "ts"> & { ts: number };
/** `GridSample` with `ts` in epoch ms (`api.historyGrid`). */
export type HistoryGridSample = Omit<GridSample, "ts"> & { ts: number };
/** `ForecastAccuracySample` with its three instants in epoch ms (`api.historyForecastAccuracy`). */
export type ForecastAccuracyRow = Omit<ForecastAccuracySample, "target_ts" | "predicted_at" | "actual_at"> & {
  target_ts: number;
  predicted_at: number;
  actual_at: number | null;
};
/** `EventReceived` with `received_at` in epoch ms (`api.historyEvents`). */
export type HistoryEventReceived = Omit<EventReceived, "received_at"> & { received_at: number };
/** `ReportSent` with `sent_at` in epoch ms (`api.historyReports`). */
export type HistoryReportSent = Omit<ReportSent, "sent_at"> & { sent_at: number };

export type Program = {
  id: string;
  programName?: string | null;
  /** valuesMap list; 3.1's replacement for the dropped programType. */
  attributes?: unknown[] | null;
  targets?: string[] | null;
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
  /** New in 3.1: an event-level duration, independent of `intervals`. */
  duration?: string | null;
  intervalPeriod?: IntervalPeriod | null;
  /** 3.1: a flat list of target strings; empty means every VEN. */
  targets?: string[] | null;
  createdDateTime?: string | null;
  /** Optional in 3.1: an event may declare a duration and no intervals. */
  intervals?: Interval[];
  [key: string]: unknown;
};

export type Report = {
  id: string;
  /** VTN-provisioned in 3.1; 3.0's programID is gone. */
  clientID?: string | null;
  eventID?: string | null;
  clientName?: string | null;
  reportName?: string | null;
  resources?: unknown;
  createdDateTime?: string | null;
  [key: string]: unknown;
};

// ─── Sim schema types ─────────────────────────────────────────────────────────

// ─── HEMS Controller types ────────────────────────────────────────────────────

export type PlannedRates = TariffSnapshot[];

export type CapacitySchedule = CapacitySnapshot[];

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
  // WM shiftable-load fields (optional) — `earliest_start` is shared with the EV,
  // where it means the instant the vehicle becomes available for the session.
  power_kw?: number;
  duration_min?: number;
  earliest_start?: string;
  /** EV only: km expected after this session's departure. Omitted = use the EV's
   *  own configured default. */
  expected_trip_distance_km?: number;
  /**
   * EV only: when the car is expected back from that trip.
   *
   * Paired with `expected_trip_distance_km` — both or neither. The server refuses a
   * half-stated estimate: a distance with no return time is energy with no instant to
   * apply it to. With neither, the plan projects no drop and waits for the measured
   * return rather than inventing one.
   */
  expected_return_time?: string;
  latest_end?: string;
  // Per-device overrides (Plan D)
  soft_deadline?: boolean;
  /** Null = state none: the VEN applies the heater's declared default, or refuses. */
  target_temp_c?: number | null;
  // Request mode (BL-28); omitted = BY_DEADLINE
  mode?: UserRequestMode;
  // MAX_COST (WP4.1-c): total charging-cost ceiling in €
  budget_eur?: number;
  /**
   * EV only: the queued sessions this submission means to displace.
   *
   * Omitted = displace nothing, so a clash is refused — the default and the safe
   * answer. Sent, it must be exactly the ids the refusal quoted back as
   * `replaceable_session_ids`, so the confirmation refers to the plans the user was
   * actually shown rather than to whatever clashes by the time the server acts.
   */
  replace_session_ids?: string[];
};

// ─── Device Session types ─────────────────────────────────────────────────────

// ─── EV usage simulation / forecast (ev-usage-simulation, ev-usage-forecast) ──

export type UpdateEvSettingsBody = {
  opportunistic_charging_enabled: boolean;
};

/** Each toggle optional — one switch changes without restating the other. */
export type UpdateArbiterSettingsBody = {
  deviation_arbiter_enabled?: boolean;
  limit_enforcement_enabled?: boolean;
};

export type CreateBaselineOverrideBody = { slots: BaselineSlot[] };

// ─── Flexibility Envelope ─────────────────────────────────────────────────────

// ─── Timeline zones ───────────────────────────────────────────────────────────

/** A planning zone returned by GET /timeline/all. */
// Lives in the shared chart package now (both UIs shade time bands the same
// way); re-exported here so every existing importer is unaffected.
export type { ZoneDef } from "@lab/charts/types";

// ─── Planner SSE events (Plan E) ──────────────────────────────────────────────

// ─── Persistent history (Phase 1, WP1.4/WP1.5) ────────────────────────────────
// Field names pass through the VEN's own snake_case wire format verbatim
// (no DTO renaming), except `ts`/`received_at`/`sent_at` which the client
// converts from ISO string to epoch ms, same as the /timeline/* client methods.

// ── Weather forecast plugin (weather-forecast-visibility) ────────────────────
// Field names pass through the Rust WeatherResponse/WeatherForecast wire shape
// verbatim (see VEN/src/routes/weather.rs, entities/weather.rs).

// ── Real-measurement MQTT feeds (real-measurement-mqtt) ───────────────────────
// Field names pass through the Rust MeasurementResponse wire shape verbatim
// (see VEN/src/routes/measurement.rs).
