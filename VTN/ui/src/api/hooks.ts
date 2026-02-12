import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useBffContext } from "../App";
import type { EventInput, ProgramInput } from "./types";

export function useHealth() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["health"],
    queryFn: () => api.health(),
    refetchInterval: 10_000,
  });
}

export function usePrograms() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["programs"],
    queryFn: () => api.programs(),
    refetchInterval: 30_000,
  });
}

export function useCreateProgram() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: ProgramInput) => api.createProgram(input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["programs"] }),
  });
}

export function useUpdateProgram() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: ProgramInput }) => api.updateProgram(id, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["programs"] }),
  });
}

export function useDeleteProgram() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteProgram(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["programs"] }),
  });
}

export function useEvents() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["events"],
    queryFn: () => api.events(),
    refetchInterval: 10_000,
  });
}

export function useCreateEvent() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: EventInput) => api.createEvent(input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["events"] }),
  });
}

export function useUpdateEvent() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: EventInput }) => api.updateEvent(id, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["events"] }),
  });
}

export function useDeleteEvent() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteEvent(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["events"] }),
  });
}

export function useVens() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["vens"],
    queryFn: () => api.vens(),
    refetchInterval: 15_000,
  });
}

export function useDeleteVen() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteVen(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["vens"] }),
  });
}

export function useReports() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["reports"],
    queryFn: () => api.reports(),
    refetchInterval: 10_000,
  });
}

export function useDeleteReport() {
  const { api } = useBffContext();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteReport(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["reports"] }),
  });
}

export function useMetrics() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["metrics"],
    queryFn: () => api.metrics(),
    refetchInterval: 10_000,
  });
}
