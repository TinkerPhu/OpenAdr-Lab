import { useCallback, useEffect, useState } from "react";
import {
  Accordion, AccordionDetails, AccordionSummary,
  Box, Chip, CircularProgress, Divider, FormControl, InputLabel,
  LinearProgress, MenuItem, Select, Stack, Tooltip, Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { useQueryClient } from "@tanstack/react-query";
import { usePlan, usePlannerEvents, useSetObjective, useTrace, usePackets } from "../api/hooks";
import type { PlannerEvent, PlannerObjective } from "../api/types";
import { PlanHeaderBar } from "../components/planner/PlanHeaderBar";
import { PlanTriggerTimeline } from "../components/planner/PlanTriggerTimeline";
import { PlanDecisionMatrix } from "../components/planner/PlanDecisionMatrix";
import { PlanPowerStack } from "../components/planner/PlanPowerStack";
import { PacketProgressBoard } from "../components/planner/PacketProgressBoard";
import { TraceTable } from "../components/planner/TraceTable";

const OBJECTIVE_OPTIONS: {
  value: PlannerObjective;
  label: string;
  tooltip: string;
  detail: string;
  weights: { label: string; value: string }[];
}[] = [
  {
    value: "min_cost",
    label: "Cost",
    tooltip: "Minimize energy cost",
    detail: "Shift flexible loads to the cheapest tariff windows. A light CO₂ penalty acts as a tiebreaker; a tiny grid-exchange penalty discourages unnecessary round-trips.",
    weights: [
      { label: "Energy cost", value: "×1.0 (primary)" },
      { label: "CO₂ intensity", value: "×0.20 (nudge)" },
      { label: "Grid exchange", value: "×0.02 (rounding)" },
      { label: "Battery wear", value: "0.03 €/kWh" },
    ],
  },
  {
    value: "min_ghg",
    label: "GHG",
    tooltip: "Minimize CO₂ emissions",
    detail: "Carbon reduction takes absolute priority. Energy cost and grid flows are ignored — the planner will charge from renewable surplus even if it is financially suboptimal.",
    weights: [
      { label: "CO₂ intensity", value: "×10.0 (dominant)" },
      { label: "Energy cost", value: "0 (ignored)" },
      { label: "Grid exchange", value: "0 (ignored)" },
    ],
  },
  {
    value: "min_grid",
    label: "Grid",
    tooltip: "Minimize total grid exchange (import + export)",
    detail: "Maximise local self-consumption by penalising every kWh that crosses the meter — in either direction. Good for grid-congestion zones or flat-rate tariffs.",
    weights: [
      { label: "Grid exchange (import + export)", value: "×1.0 (primary)" },
      { label: "Energy cost", value: "0 (ignored)" },
      { label: "CO₂ intensity", value: "0 (ignored)" },
    ],
  },
  {
    value: "min_import",
    label: "Autarky",
    tooltip: "Minimize grid import only (export OK)",
    detail: "Reduce how much you draw from the grid. Exporting surplus PV or battery power is not penalised and can happen freely — ideal when export is revenue-neutral.",
    weights: [
      { label: "Grid import", value: "×1.0 (primary)" },
      { label: "Grid export", value: "0 (allowed)" },
      { label: "Energy cost / CO₂", value: "0 (ignored)" },
    ],
  },
  {
    value: "max_revenue",
    label: "Revenue",
    tooltip: "Maximize export revenue",
    detail: "Discharge battery and curtail loads to maximise export income at peak export prices and shifts flexible loads to the cheapest tariff windows. Battery wear cost is included so the planner avoids excessive cycling.",
    weights: [
      { label: "Energy cost", value: "×1.0 (primary)" },
      { label: "Battery wear", value: "0.03 €/kWh" },
      { label: "CO₂ / grid", value: "0 (ignored)" },
    ],
  },
];

// ─── Objective legend ─────────────────────────────────────────────────────────

function ObjectiveLegend() {
  return (
    <Accordion
      defaultExpanded={false}
      data-testid="objective-legend"
      disableGutters
      elevation={0}
      sx={{ border: 1, borderColor: "divider", borderRadius: 1, mb: 2 }}
    >
      <AccordionSummary expandIcon={<ExpandMoreIcon />}>
        <Typography variant="body2" color="text.secondary">
          Optimization Objective — weight reference
        </Typography>
      </AccordionSummary>
      <AccordionDetails>
        <Stack spacing={1.5}>
          {OBJECTIVE_OPTIONS.map((opt) => (
            <Box key={opt.value}>
              <Typography variant="body2" fontWeight="bold">{opt.label}</Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.5 }}>
                {opt.detail}
              </Typography>
              <Stack direction="row" flexWrap="wrap" gap={0.5}>
                {opt.weights.map((w) => (
                  <Chip key={w.label} size="small" label={`${w.label}: ${w.value}`} variant="outlined" />
                ))}
              </Stack>
            </Box>
          ))}
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}

// ─── Planner status (Plan E: SSE feedback) ────────────────────────────────────

type PlannerStatus =
  | { phase: "idle" }
  | { phase: "solving"; elapsed_ms: number; iteration: number; objective: PlannerObjective }
  | { phase: "updated"; solver_ms: number };

function PlannerStatusBar({ status }: { status: PlannerStatus }) {
  // Always render a fixed-height wrapper so that showing/hiding the status
  // bar doesn't cause layout shifts (which break Playwright click stability).
  return (
    <Box data-testid="planner-status" sx={{ minHeight: 32, mb: 1, display: "flex", alignItems: "center" }}>
      {status.phase === "solving" && (
        <Box data-testid="planner-status-solving" sx={{ display: "flex", alignItems: "center", gap: 1, width: "100%" }}>
          <CircularProgress size={16} />
          <Typography variant="body2" color="text.secondary">
            Solving ({OBJECTIVE_OPTIONS.find((o) => o.value === status.objective)?.label ?? status.objective})
            — {(status.elapsed_ms / 1000).toFixed(0)} s, tick {status.iteration}
          </Typography>
          <LinearProgress sx={{ flex: 1 }} />
        </Box>
      )}
      {status.phase === "updated" && (
        <Chip
          data-testid="planner-status-updated"
          size="small"
          color="success"
          label={`Plan updated — solved in ${(status.solver_ms / 1000).toFixed(1)} s`}
        />
      )}
    </Box>
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
          <InputLabel>Optimization objective</InputLabel>
          <Select
            value={objective}
            label="Optimization objective"
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

      <ObjectiveLegend />

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

        {/* Decision Trace (collapsible) */}
        <Accordion defaultExpanded={false} data-testid="trace-accordion">
          <AccordionSummary expandIcon={<ExpandMoreIcon />}>
            <Typography variant="subtitle2" color="text.secondary">
              Decision Trace ({events?.length ?? 0} events)
            </Typography>
          </AccordionSummary>
          <AccordionDetails sx={{ p: 0 }}>
            <TraceTable entries={events ?? []} />
          </AccordionDetails>
        </Accordion>
      </Stack>
    </Box>
  );
}
