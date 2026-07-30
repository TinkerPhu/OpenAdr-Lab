import {
  Card,
  CardActions,
  CardContent,
  CardHeader,
  Divider,
  FormControlLabel,
  Stack,
  Switch,
  Typography,
} from "@mui/material";
import type {
  ArbiterDiagnostics,
  ArbiterSettings,
  UpdateArbiterSettingsBody,
} from "../../api/types";

export type ArbiterSettingsCardProps = {
  arbiterSettings: ArbiterSettings | undefined;
  putArbiterSettings: (body: UpdateArbiterSettingsBody) => void;
  diagnostics: ArbiterDiagnostics | undefined;
};

/** Runtime toggle for the deviation arbiter (§5.3,
 * openspec/changes/deviation-arbiter/). Default off in every profile —
 * enabling it here lets the arbiter's reactive levers (battery, EV, heater
 * pause/emergency-mode, PV curtailment) react live to injected PV/base-load
 * swings instead of only following the last MILP plan. */
function formatKw(kw: number | null | undefined): string {
  return kw === null || kw === undefined ? "—" : `${kw.toFixed(2)} kW`;
}

/** ui-transparency: last tick's arbiter reasoning — no backend-only decision
 * without an inspectable surface. `null`/`undefined` before the arbiter has
 * run this process, or during the no-plan-yet startup window. */
function DiagnosticsReadout({ diagnostics }: { diagnostics: ArbiterDiagnostics | undefined }) {
  if (!diagnostics || diagnostics.updated_at === null) {
    return (
      <Typography variant="body2" color="text.secondary" data-testid="arbiter-diagnostics-empty">
        No arbiter tick recorded yet.
      </Typography>
    );
  }
  return (
    <Stack spacing={0.5} data-testid="arbiter-diagnostics">
      <Typography variant="body2">
        Projected net site power: <strong>{formatKw(diagnostics.net_kw)}</strong>
      </Typography>
      <Typography variant="body2">
        Deviation from plan: <strong>{formatKw(diagnostics.dev_kw)}</strong>
      </Typography>
      <Typography variant="body2">
        Active lever: <strong>{diagnostics.active_lever ?? "none"}</strong>
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Updated {new Date(diagnostics.updated_at).toLocaleTimeString()}
      </Typography>
    </Stack>
  );
}

export function ArbiterSettingsCard(props: ArbiterSettingsCardProps) {
  const { arbiterSettings, putArbiterSettings, diagnostics } = props;
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
        {enabled && (
          <>
            <Divider sx={{ my: 1.5 }} />
            <DiagnosticsReadout diagnostics={diagnostics} />
          </>
        )}
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
