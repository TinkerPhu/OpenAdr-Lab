import {
  Card,
  CardActions,
  CardContent,
  CardHeader,
  FormControlLabel,
  Switch,
  Typography,
} from "@mui/material";
import type { ArbiterSettings, UpdateArbiterSettingsBody } from "../../api/types";

export type ArbiterSettingsCardProps = {
  arbiterSettings: ArbiterSettings | undefined;
  putArbiterSettings: (body: UpdateArbiterSettingsBody) => void;
};

/** Runtime toggle for the deviation arbiter (§5.3,
 * openspec/changes/deviation-arbiter/). Default off in every profile —
 * enabling it here lets the arbiter's reactive levers (battery, EV, heater
 * pause/emergency-mode, PV curtailment) react live to injected PV/base-load
 * swings instead of only following the last MILP plan. */
export function ArbiterSettingsCard(props: ArbiterSettingsCardProps) {
  const { arbiterSettings, putArbiterSettings } = props;
  const enabled = arbiterSettings?.deviation_arbiter_enabled ?? false;

  return (
    <Card data-testid="arbiter-settings-card">
      <CardHeader title="Deviation Arbiter" />
      <CardContent>
        <Typography variant="body2" color="text.secondary">
          When enabled, the arbiter reacts to live PV/base-load deviations
          each tick — pausing or emergency-heating the tank, adjusting
          opportunistic EV charging, or discharging/charging the battery —
          ranked by marginal cost. Off by default; the plan's own allocations
          are unaffected either way.
        </Typography>
      </CardContent>
      <CardActions sx={{ px: 2 }}>
        <FormControlLabel
          label="Enable deviation arbiter"
          control={
            <Switch
              checked={enabled}
              data-testid="arbiter-enabled-switch"
              onChange={() =>
                putArbiterSettings({ deviation_arbiter_enabled: !enabled })
              }
            />
          }
        />
      </CardActions>
    </Card>
  );
}
