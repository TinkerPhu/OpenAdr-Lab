import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useVenContext } from "../App";
import type {
  SensorSnapshot, SimInjectState, CreateUserRequestBody,
  CreateEvSessionBody, CreateHeaterTargetBody, CreateShiftableLoadBody, CreateBaselineOverrideBody,
} from "./types";

export function useHealth() {
  const { api } = useVenContext();
  console.log("[VEN-UI] useHealth hook called, baseUrl:", api.baseUrl);
  return useQuery({
    queryKey: ["health", api.baseUrl],
    queryFn: () => { console.log("[VEN-UI] useHealth queryFn firing"); return api.health(); },
    refetchInterval: 10_000,
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["reports"] });
    },
  });
}

export function useUpdateReport() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, payload }: { id: string; payload: unknown }) =>
      api.updateReport(id, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["reports"] });
    },
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
    queryFn: () => api.simSchema(),
    staleTime: Infinity, // schema doesn't change at runtime
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

export function usePackets() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["packets", api.baseUrl],
    queryFn: () => api.packets(),
    refetchInterval: 10_000,
  });
}

export function usePlan(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["plan", api.baseUrl],
    queryFn: () => api.plan(),
    refetchInterval: options?.refetchInterval ?? 10_000,
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

export function useTariffs(options?: { refetchInterval?: number | false }) {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["rates", api.baseUrl],
    queryFn: () => api.rates(),
    refetchInterval: options?.refetchInterval ?? 30_000,
  });
}

/** @deprecated Use useTariffs instead. Alias kept for legacy Controller page. */
export const useRates = useTariffs;

export function useCapacity() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["capacity", api.baseUrl],
    queryFn: () => api.capacity(),
    refetchInterval: 10_000,
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
      queryClient.invalidateQueries({ queryKey: ["packets"] });
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

// ── Device Session hooks (Phase B) ──────────────────────────────────────────

export function useEvSession() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["ev_session", api.baseUrl],
    queryFn: () => api.evSession(),
    refetchInterval: 10_000,
  });
}

export function usePostEvSession() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateEvSessionBody) => api.postEvSession(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["ev_session"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useDeleteEvSession() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.deleteEvSession(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["ev_session"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useHeaterTarget() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["heater_target", api.baseUrl],
    queryFn: () => api.heaterTarget(),
    refetchInterval: 10_000,
  });
}

export function usePostHeaterTarget() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateHeaterTargetBody) => api.postHeaterTarget(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["heater_target"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useDeleteHeaterTarget() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.deleteHeaterTarget(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["heater_target"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useShiftableLoads() {
  const { api } = useVenContext();
  return useQuery({
    queryKey: ["shiftable_loads", api.baseUrl],
    queryFn: () => api.shiftableLoads(),
    refetchInterval: 10_000,
  });
}

export function usePostShiftableLoad() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateShiftableLoadBody) => api.postShiftableLoad(body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["shiftable_loads"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
  });
}

export function useDeleteShiftableLoad() {
  const { api } = useVenContext();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteShiftableLoad(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["shiftable_loads"] });
      queryClient.invalidateQueries({ queryKey: ["plan"] });
    },
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
