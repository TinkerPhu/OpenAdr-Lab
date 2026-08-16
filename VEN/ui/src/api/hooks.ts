import { useQuery, useQueries, useMutation, useQueryClient } from "@tanstack/react-query";
import { useRef, useEffect, useLayoutEffect } from "react";
import { useVenContext } from "../App";
import type {
  SensorSnapshot, SimInjectState, CreateUserRequestBody,
  UpdateEvSettingsBody, UpdateArbiterSettingsBody,
  CreateBaselineOverrideBody,
  PlannerObjective, PlannerEvent, ComfortRate, UserNotificationSeverity,
  EventLogEntry,
} from "./types";

// Mirrors the backend's own EVENT_LOG_RING_CAP (VEN/src/state/event_log.rs) —
// keeps the client-side list from growing unbounded over a long-lived SSE
// connection.
const EVENT_LOG_CLIENT_CAP = 200;

export function useHealth() {
  const { api } = useVenContext();
  console.log("[VEN-UI] useHealth hook called, baseUrl:", api.baseUrl);
  return useQuery({
    queryKey: ["health", api.baseUrl],
    queryFn: () => { console.log("[VEN-UI] useHealth queryFn firing"); return api.health(); },
    refetchInterval: 10_000,
  });
}

/** WP4.3 (BL-20): the notification feed, polled every 10 s. */
export function useNotifications() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["notifications", api.baseUrl],
    queryFn: () => api.notifications(),
    refetchInterval: 10_000,
  });
}

/** 030: persisted notification history with optional severity filter. */
export function useNotificationHistory(severity?: UserNotificationSeverity) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["notifications/history", api.baseUrl, severity ?? "ALL"],
    queryFn: () => api.notificationsHistory(severity ? { severity } : undefined),
    refetchInterval: 10_000,
  });
}

/** WP4.6: active grid signals for the status strip, polled every 10 s. */
export function useSignals() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["signals", api.baseUrl],
    queryFn: () => api.signals(),
    refetchInterval: 10_000,
  });
}

/** WP4.2 (BL-19): the effective comfort curve for one asset. */
export function useComfortCurve(assetId: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["comfort_curve", api.baseUrl, assetId],
    queryFn: () => api.comfortCurve(assetId),
  });
}

export function useSetComfortCurve() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ assetId, rates }: { assetId: string; rates: ComfortRate[] }) =>
      api.postComfortCurve(assetId, rates),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["comfort_curve"] });
    },
  });
}

export function useDeleteComfortCurve() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (assetId: string) => api.deleteComfortCurve(assetId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["comfort_curve"] });
    },
  });
}

export function usePrograms() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["programs", api.baseUrl],
    queryFn: () => api.programs(),
    refetchInterval: 300_000,
  });
}

export function useEvents() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["events", api.baseUrl],
    queryFn: () => api.events(200),
    refetchInterval: 30_000,
  });
}

export function useSensor() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["sensor", api.baseUrl],
    queryFn: () => api.sensors(),
    refetchInterval: 10_000,
  });
}

export function usePostSensor() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: Partial<SensorSnapshot>) => api.postSensors(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["sensor"] });
    },
  });
}

export function useReports() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["reports", api.baseUrl],
    queryFn: () => api.reports(),
    refetchInterval: 30_000,
  });
}

export function useSubmitReport() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: unknown) => api.submitReport(payload),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["reports"] });
      queryClient.invalidateQueries({ queryKey: ["reportSubmissions"] });
    },
  });
}

export function useUpdateReport() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, payload }: { id: string; payload: unknown }) =>
      api.updateReport(id, payload),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["reports"] });
      queryClient.invalidateQueries({ queryKey: ["reportSubmissions"] });
    },
  });
}

/** WP-T5 (G-5): recent report submission outcomes, for the per-row status chip. */
export function useReportSubmissions() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["reportSubmissions", api.baseUrl],
    queryFn: () => api.reportSubmissions(),
    refetchInterval: 30_000,
  });
}

export function useSim(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["sim", api.baseUrl],
    queryFn: () => api.sim(),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

export function useTrace(limit = 50) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["trace", api.baseUrl, limit],
    queryFn: () => api.trace(limit),
    refetchInterval: 10_000,
  });
}

export function useSimSchema() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["sim", "schema", api.baseUrl],
    queryFn: async () => {
      try {
        const data = await api.simSchema();
        console.warn("[simSchema] loaded keys:", Object.keys(data).join(","));
        return data;
      } catch (err) {
        console.error("[simSchema] fetch failed:", String(err));
        throw err;
      }
    },
    staleTime: Infinity, // schema doesn't change at runtime
    retry: 3,
  });
}

export function useSimInject() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["simInject", api.baseUrl],
    queryFn: () => api.getSimInject(),
    staleTime: Infinity, // only fetch on mount; user controls the state
  });
}

export function useSetSimInject() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: Partial<SimInjectState>) => api.postSimInject(patch),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["simInject"] });
      queryClient.refetchQueries({ queryKey: ["sim"] });
      queryClient.refetchQueries({ queryKey: ["timeline/all"] });
    },
  });
}

export function useResetAssetSoc() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ assetId, soc }: { assetId: string; soc: number }) =>
      api.postSimReset(assetId, soc),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: ["sim"] });
    },
  });
}

export function useMetrics() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["metrics", api.baseUrl],
    queryFn: () => api.metrics(),
    refetchInterval: 10_000,
  });
}

// WP-T3 (docs/history/project_journal.md, search "WP-T"): background task restart status.
export function useTasksStatus() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["tasks-status", api.baseUrl],
    queryFn: () => api.tasksStatus(),
    refetchInterval: 10_000,
  });
}

// WP-T1 (docs/history/project_journal.md, search "WP-T"): VTN connection detail
// (token expiry, backoff, last error) — `/health` only carries a terse
// ok/degraded summary, this is the detail behind it for the Dashboard's
// VTN Connection status row (WP-T8).
export function useVtnStatus() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["vtn-status", api.baseUrl],
    queryFn: () => api.vtnStatus(),
    refetchInterval: 10_000,
  });
}

// WP-T4/GB-13: VEN-operational Event Log — seeded via an initial GET, kept
// live via the backend's /events/log/events SSE stream (live-forward-only,
// no replay) rather than polling.
export function useEventLog() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["event-log", api.baseUrl],
    queryFn: () => api.eventLog(),
  });

  useEffect(() => {
    const es = new EventSource(`${api.baseUrl}/events/log/events`);
    es.onmessage = (e) => {
      try {
        const entry = JSON.parse(e.data) as EventLogEntry;
        queryClient.setQueryData<EventLogEntry[]>(
          ["event-log", api.baseUrl],
          (old) => {
            const list = old ?? [];
            if (list.some((x) => x.id === entry.id)) return list;
            const next = [...list, entry];
            return next.length > EVENT_LOG_CLIENT_CAP
              ? next.slice(next.length - EVENT_LOG_CLIENT_CAP)
              : next;
          },
        );
      } catch {
        /* ignore malformed events */
      }
    };
    return () => es.close();
  }, [api.baseUrl, queryClient]);

  return query;
}

export function usePlan(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["plan", api.baseUrl],
    queryFn: () => api.plan(),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

export function useWeather(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["weather", api.baseUrl],
    queryFn: () => api.weather(),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

export function useMeasurement(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["measurement", api.baseUrl],
    queryFn: () => api.measurement(),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

export function useSetObjective() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (objective: PlannerObjective) => api.setObjective(objective),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["plan"] }),
  });
}

export function useTimeline(
  assetId: string,
  hoursBack = 1.0,
  hoursForward = 1.0
) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["timeline", api.baseUrl, assetId, hoursBack, hoursForward],
    queryFn: () => api.timeline(assetId, { hoursBack, hoursForward }),
    refetchInterval: 10_000,
  });
}

export function useAllTimelines(
  hoursBack = 1.0,
  hoursForward = 1.0,
  options?: { refetchInterval?: number | false; resolution?: number }
) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["timeline/all", api.baseUrl, hoursBack, hoursForward, options?.resolution],
    queryFn: () => api.allTimelines({ hoursBack, hoursForward, resolution: options?.resolution }),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

/** WP1.5 — persistent history reads. `from`/`to` are ISO strings; `refetchInterval`
 * is off since a past date range doesn't change once the window has fully elapsed. */
export function useHistoryTicks(from: string, to: string, assetId?: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/ticks", api.baseUrl, from, to, assetId],
    queryFn: () => api.historyTicks({ from, to, assetId }),
    refetchInterval: false,
  });
}

export function useHistoryGrid(from: string, to: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/grid", api.baseUrl, from, to],
    queryFn: () => api.historyGrid({ from, to }),
    refetchInterval: false,
  });
}

export function useHistoryEvents(from: string, to: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/events", api.baseUrl, from, to],
    queryFn: () => api.historyEvents({ from, to }),
    refetchInterval: false,
  });
}

export function useHistoryReports(from: string, to: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/reports", api.baseUrl, from, to],
    queryFn: () => api.historyReports({ from, to }),
    refetchInterval: false,
  });
}

/** forecast-accuracy-tracking: near/far forecast samples (predicted, and actual once
 * reconciled) for one asset, `[from, to)`. */
export function useHistoryForecastAccuracy(from: string, to: string, assetId?: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/forecast-accuracy", api.baseUrl, from, to, assetId],
    queryFn: () => api.historyForecastAccuracy({ from, to, assetId }),
    refetchInterval: false,
  });
}

/** GB-25: per-plan-cycle solve-quality history (solve time, solver outcome, MIP-gap proxy,
 * cost/warning summary), `[from, to)`. */
export function useHistoryPlans(from: string, to: string) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["history/plans", api.baseUrl, from, to],
    queryFn: () => api.historyPlans({ from, to }),
    refetchInterval: false,
  });
}

export function useObligations() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["obligations", api.baseUrl],
    queryFn: () => api.obligations(),
    refetchInterval: 10_000,
  });
}

/** Per-asset feasible power range (Phase A), fetched in parallel for every
 * `assetIds` entry via `useQueries` — no bulk endpoint exists server-side. */
export function useAssetCapabilities(assetIds: string[]) {
  const { api } = useVenContext();
  return useQueries({
    queries: assetIds.map((assetId) => ({
      queryKey: ["capability", api.baseUrl, assetId],
      queryFn: () => api.assetCapability(assetId),
      refetchInterval: 10_000,
    })),
  });
}

/** Per-asset forecasts from the latest plan cycle (WP3.6, BL-15) — empty
 * array until the first plan has been adopted. */
export function useAssetForecasts() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["forecast", api.baseUrl],
    queryFn: () => api.assetForecasts(),
    refetchInterval: 10_000,
  });
}

export function useTariffs(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["rates", api.baseUrl],
    queryFn: () => api.rates(),
    refetchInterval: options?.refetchInterval ?? 30_000,
  });
}

export function useCapacity() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["capacity", api.baseUrl],
    queryFn: () => api.capacity(),
    refetchInterval: 10_000,
  });
}

export function useCapacitySchedule(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["capacity-schedule", api.baseUrl],
    queryFn: () => api.capacitySchedule(),
    refetchInterval: options?.refetchInterval ?? 30_000,
  });
}

export function useLedger() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["ledger", api.baseUrl],
    queryFn: () => api.ledger(),
    refetchInterval: 30_000,
  });
}

export function useRequests(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["user_requests", api.baseUrl],
    queryFn: () => api.userRequests(),
    refetchInterval: options?.refetchInterval ?? 10_000,
  });
}

export function usePostRequest() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateUserRequestBody) => api.postRequest(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["user_requests"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useDeleteRequest() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteRequest(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["user_requests"] });
    },
  });
}

export function useEvSettings() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["ev_settings", api.baseUrl],
    queryFn: () => api.evSettings(),
    refetchInterval: 10_000,
  });
}

export function usePutEvSettings() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateEvSettingsBody) => api.putEvSettings(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["ev_settings"] });
    },
  });
}

export function useArbiterSettings() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["arbiter_settings", api.baseUrl],
    queryFn: () => api.arbiterSettings(),
    refetchInterval: 10_000,
  });
}

export function usePutArbiterSettings() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateArbiterSettingsBody) => api.putArbiterSettings(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["arbiter_settings"] });
    },
  });
}

export function useArbiterDiagnostics(enabled: boolean) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["arbiter_diagnostics", api.baseUrl],
    queryFn: () => api.arbiterDiagnostics(),
    refetchInterval: 5_000,
    enabled,
  });
}

/** BL-43: live site-level headroom snapshot — a diagnostic value, not driven
 * by the Controller page's own unified 2s timer. */
export function useFlexibility() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["flexibility", api.baseUrl],
    queryFn: () => api.flexibility(),
    refetchInterval: 10_000,
  });
}

/** BL-43: the site-headroom ring, for the "Site Headroom" chart. */
export function useFlexibilityHistory() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["flexibility_history", api.baseUrl],
    queryFn: () => api.flexibilityHistory(),
    refetchInterval: 10_000,
  });
}

export function useBaselineOverride() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["baseline_override", api.baseUrl],
    queryFn: () => api.baselineOverride(),
    refetchInterval: 10_000,
  });
}

export function usePostBaselineOverride() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateBaselineOverrideBody) => api.postBaselineOverride(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["baseline_override"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useDeleteBaselineOverride() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.deleteBaselineOverride(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["baseline_override"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

// ── Planner SSE events (Plan E) ─────────────────────────────────────────────

/** Subscribe to planner progress via Server-Sent Events at GET /plan/events. */
export function usePlannerEvents(onEvent: (event: PlannerEvent) => void): void {
  const { api } = useVenContext();
  // Ref keeps callback stable so EventSource isn't re-created on every render
  const cbRef = useRef(onEvent);
  useLayoutEffect(() => { cbRef.current = onEvent; });

  useEffect(() => {
    const es = new EventSource(`${api.baseUrl}/plan/events`);
    es.onmessage = (e) => {
      try {
        cbRef.current(JSON.parse(e.data) as PlannerEvent);
      } catch {
        /* ignore malformed events */
      }
    };
    return () => es.close();
  }, [api.baseUrl]); // reconnect only when VEN URL changes
}
