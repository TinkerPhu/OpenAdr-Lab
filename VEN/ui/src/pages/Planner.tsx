import { useEffect, useState } from "react";
import {
  Box, Divider, FormControl, InputLabel, MenuItem,
  Select, Stack, Tooltip, Typography,
} from "@mui/material";
import { usePlan, useSetObjective, useTrace, usePackets } from "../api/hooks";
import type { PlannerObjective } from "../api/types";
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

export function PlannerPage() {
  const { data: plan } = usePlan();
  const { data: events } = useTrace(200);
  const { data: packets } = usePackets();
  const setObjectiveMutation = useSetObjective();

  const [objective, setObjective] = useState<PlannerObjective>("min_cost");

  useEffect(() => {
    if (plan?.objective) setObjective(plan.objective);
  }, [plan?.objective]);

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
