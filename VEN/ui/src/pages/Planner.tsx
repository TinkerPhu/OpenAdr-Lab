import { useCallback, useEffect, useState } from "react";
import {
  Box, Chip, CircularProgress, Divider, FormControl, InputLabel,
  LinearProgress, MenuItem, Select, Stack, Tooltip, Typography,
} from "@mui/material";
import { useQueryClient } from "@tanstack/react-query";
import { usePlan, usePlannerEvents, useSetObjective, useTrace, usePackets } from "../api/hooks";
import type { PlannerEvent, PlannerObjective } from "../api/types";
import { PlanHeaderBar } from "../components/planner/PlanHeaderBar";
import { PlanTriggerTimeline } from "../components/planner/PlanTriggerTimeline";
import { PlanDecisionMatrix } from "../components/planner/PlanDecisionMatrix";
import { PlanPowerStack } from "../components/planner/PlanPowerStack";
import { PacketProgressBoard } from "../components/planner/PacketProgressBoard";

const OBJECTIVE_OPTIONS: {
  value: PlannerObjective;
  label: string;
  tooltip: string;
}[] = [
  { value: "min_cost",    label: "Cost",    tooltip: "Minimize energy cost" },
  { value: "min_ghg",     label: "GHG",     tooltip: "Minimize CO₂ emissions" },
  { value: "min_grid",    label: "Grid",    tooltip: "Minimize total grid exchange (import + export)" },
  { value: "min_import",  label: "Autarky", tooltip: "Minimize grid import only (export OK)" },
  { value: "max_revenue", label: "Revenue", tooltip: "Maximize export revenue" },
];

// ─── Planner status (Plan E: SSE feedback) ────────────────────────────────────

type PlannerStatus =
  | { phase: "idle" }
  | { phase: "solving"; elapsed_ms: number; iteration: number; objective: PlannerObjective }
  | { phase: "updated"; solver_ms: number };

function PlannerStatusBar({ status }: { status: PlannerStatus }) {
  if (status.phase === "idle") return null;
  if (status.phase === "solving")
    return (
      <Box data-testid="planner-status-solving" sx={{ display: "flex", alignItems: "center", gap: 1, mb: 1 }}>
        <CircularProgress size={16} />
        <Typography variant="body2" color="text.secondary">
          Solving ({OBJECTIVE_OPTIONS.find((o) => o.value === status.objective)?.label ?? status.objective})
          — {(status.elapsed_ms / 1000).toFixed(0)} s, tick {status.iteration}
        </Typography>
        <LinearProgress sx={{ flex: 1 }} />
      </Box>
    );
  // phase === "updated"
  return (
    <Chip
      data-testid="planner-status-updated"
      size="small"
      color="success"
      label={`Plan updated — solved in ${(status.solver_ms / 1000).toFixed(1)} s`}
      sx={{ mb: 1 }}
    />
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export function PlannerPage() {
  const { data: plan } = usePlan();
  const { data: events } = useTrace(200);
  const { data: packets } = usePackets();
  const setObjectiveMutation = useSetObjective();
  const queryClient = useQueryClient();

  const [objective, setObjective] = useState<PlannerObjective>("min_cost");
  const [plannerStatus, setPlannerStatus] = useState<PlannerStatus>({ phase: "idle" });

  useEffect(() => {
    if (plan?.objective) setObjective(plan.objective);
  }, [plan?.objective]);

  usePlannerEvents(
    useCallback(
      (event: PlannerEvent) => {
        if (event.type === "solving_started") {
          setPlannerStatus({
            phase: "solving",
            elapsed_ms: 0,
            iteration: 0,
            objective: event.objective,
          });
        } else if (event.type === "solving_progress") {
          setPlannerStatus((prev) =>
            prev.phase === "solving"
              ? { ...prev, elapsed_ms: event.elapsed_ms, iteration: event.iteration }
              : prev,
          );
        } else if (event.type === "plan_ready") {
          setPlannerStatus({ phase: "updated", solver_ms: event.solver_ms });
          queryClient.invalidateQueries({ queryKey: ["plan"] });
          // Fade back to idle after 3 s
          setTimeout(() => setPlannerStatus({ phase: "idle" }), 3000);
        }
      },
      [queryClient],
    ),
  );

  return (
    <Box data-testid="planner-heading" sx={{ p: 2 }}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ mb: 2 }}>
        <Typography variant="h5">Planner</Typography>
        <FormControl size="small" sx={{ minWidth: 180 }}>
          <InputLabel>Optimization focus</InputLabel>
          <Select
            value={objective}
            label="Optimization focus"
            data-testid="objective-select"
            onChange={(e) => {
              const val = e.target.value as PlannerObjective;
              setObjective(val);
              setObjectiveMutation.mutate(val);
            }}
          >
            {OBJECTIVE_OPTIONS.map((opt) => (
              <MenuItem key={opt.value} value={opt.value}>
                <Tooltip title={opt.tooltip} placement="right">
                  <span>{opt.label}</span>
                </Tooltip>
              </MenuItem>
            ))}
          </Select>
        </FormControl>
      </Stack>

      <Stack spacing={3} divider={<Divider />}>
        {/* Planner Status (Plan E) */}
        <PlannerStatusBar status={plannerStatus} />

        {/* Plan Header */}
        <PlanHeaderBar plan={plan} />

        {/* Power Stack Chart */}
        <PlanPowerStack plan={plan} />

        {/* Trigger Timeline */}
        <Box>
          <Typography variant="subtitle2" color="text.secondary" gutterBottom>
            Trigger History
          </Typography>
          <PlanTriggerTimeline events={events ?? []} />
        </Box>

        {/* Decision Matrix */}
        <PlanDecisionMatrix plan={plan} />

        {/* Packet Progress Board */}
        <Box>
          <Typography variant="subtitle2" color="text.secondary" gutterBottom>
            Packet Progress
          </Typography>
          <PacketProgressBoard packets={packets ?? []} />
        </Box>
      </Stack>
    </Box>
  );
}
