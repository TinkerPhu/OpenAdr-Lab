import { useQuery } from "@tanstack/react-query";
import { useBffContext } from "../App";

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

export function useEvents() {
  const { api } = useBffContext();
  return useQuery({
    queryKey: ["events"],
    queryFn: () => api.events(),
    refetchInterval: 10_000,
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
