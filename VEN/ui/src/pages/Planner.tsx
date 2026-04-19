import { Box, Divider, Stack, Typography } from "@mui/material";
import { usePlan, useTrace, usePackets } from "../api/hooks";
import { PlanHeaderBar } from "../components/planner/PlanHeaderBar";
import { PlanTriggerTimeline } from "../components/planner/PlanTriggerTimeline";
import { PlanDecisionMatrix } from "../components/planner/PlanDecisionMatrix";
import { PlanPowerStack } from "../components/planner/PlanPowerStack";
import { PacketProgressBoard } from "../components/planner/PacketProgressBoard";

export function PlannerPage() {
  const { data: plan } = usePlan();
  const { data: events } = useTrace(200);
  const { data: packets } = usePackets();

  return (
    <Box data-testid="planner-heading" sx={{ p: 2 }}>
      <Typography variant="h5" gutterBottom>Planner</Typography>

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
