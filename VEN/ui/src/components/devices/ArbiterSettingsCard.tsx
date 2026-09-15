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
  LimitPassOutcome,
  UpdateArbiterSettingsBody,
} from "../../api/types";

export type ArbiterSettingsCardProps = {
  arbiterSettings: ArbiterSettings | undefined;
  putArbiterSettings: (body: UpdateArbiterSettingsBody) => void;
  diagnostics: ArbiterDiagnostics | undefined;
};

function formatKw(kw: number | null | undefined): string {
  return kw === null || kw === undefined ? "—" : `${kw.toFixed(2)} kW`;
}

/** ui-transparency: the deviation pass's last tick — no backend-only decision
 * without an inspectable surface. */
function DeviationReadout({ diagnostics }: { diagnostics: ArbiterDiagnostics }) {
  return (
    <Stack spacing={0.5} data-testid="arbiter-diagnostics">
      <Typography variant="subtitle2">Deviation correction</Typography>
      <Typography variant="body2">
        Projected net site power: <strong>{formatKw(diagnostics.net_kw)}</strong>
      </Typography>
      <Typography variant="body2">
        Deviation from plan: <strong>{formatKw(diagnostics.dev_kw)}</strong>
      </Typography>
      <Typography variant="body2">
        Active lever: <strong>{diagnostics.active_lever ?? "none"}</strong>
      </Typography>
    </Stack>
  );
}

/** The limit pass's last tick (GB-47): the import ceiling it steers to, what
 * the site would otherwise have drawn above it, the lever that shed it, and
 * any excess nothing could shed. */
function LimitReadout({ limit }: { limit: LimitPassOutcome | null }) {
  return (
    <Stack spacing={0.5} data-testid="arbiter-limit-diagnostics">
      <Typography variant="subtitle2">Limit enforcement</Typography>
      {limit === null ? (
        <Typography variant="body2" color="text.secondary">
          No hard import limit in force.
        </Typography>
      ) : (
        <>
          <Typography variant="body2">
            Import ceiling: <strong>{formatKw(limit.target_kw)}</strong>
          </Typography>
          <Typography variant="body2">
            Excess before shedding: <strong>{formatKw(Math.max(limit.excess_kw, 0))}</strong>
          </Typography>
          <Typography variant="body2">
            Lever: <strong>{limit.active_lever ?? "none"}</strong>
          </Typography>
          <Typography
            variant="body2"
            color={limit.unresolved_kw > 0 ? "error" : undefined}
          >
            Unresolved: <strong>{formatKw(limit.unresolved_kw)}</strong>
          </Typography>
        </>
      )}
    </Stack>
  );
}

function DiagnosticsReadout(props: {
  diagnostics: ArbiterDiagnostics | undefined;
  deviationEnabled: boolean;
  limitEnabled: boolean;
}) {
  const { diagnostics, deviationEnabled, limitEnabled } = props;
  if (!diagnostics || diagnostics.updated_at === null) {
    return (
      <Typography variant="body2" color="text.secondary" data-testid="arbiter-diagnostics-empty">
        No arbiter tick recorded yet.
      </Typography>
    );
  }
  return (
    <Stack spacing={1.5}>
      {deviationEnabled && <DeviationReadout diagnostics={diagnostics} />}
      {limitEnabled && <LimitReadout limit={diagnostics.limit} />}
      <Typography variant="body2">
        Measured net site power: <strong>{formatKw(diagnostics.measured_net_kw)}</strong>
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Updated {new Date(diagnostics.updated_at).toLocaleTimeString()}
      </Typography>
    </Stack>
  );
}

/** The arbiter's two runtime toggles. Deviation correction (off by default)
 * reacts to live PV/base-load deviations from the plan; limit enforcement (on
 * by default) keeps site import under an active capacity limit or alert
 * whatever the plan says — switch it off only to measure the planner alone.
 * Both passes use the same ranked levers; decisions also appear in the
 * Planner's Decision Trace. */
export function ArbiterSettingsCard(props: ArbiterSettingsCardProps) {
  const { arbiterSettings, putArbiterSettings, diagnostics } = props;
  const deviationEnabled = arbiterSettings?.deviation_arbiter_enabled ?? false;
  const limitEnabled = arbiterSettings?.limit_enforcement_enabled ?? true;

  return (
    <Card data-testid="arbiter-settings-card">
      <CardHeader title="Arbiter" />
      <CardContent>
        <Typography variant="body2" color="text.secondary">
          Each tick the arbiter adjusts the plan's setpoints with the cheapest
          levers — battery, EV charging, heater stages (and, only in a grid
          alert, the heater's emergency heat), PV curtailment. Deviation
          correction follows live PV/base-load swings; limit enforcement keeps
          import under an active capacity limit or alert.
        </Typography>
        {(deviationEnabled || limitEnabled) && (
          <>
            <Divider sx={{ my: 1.5 }} />
            <DiagnosticsReadout
              diagnostics={diagnostics}
              deviationEnabled={deviationEnabled}
              limitEnabled={limitEnabled}
            />
          </>
        )}
      </CardContent>
      <CardActions sx={{ px: 2, flexWrap: "wrap" }}>
        <FormControlLabel
          label="Deviation correction"
          control={
            <Switch
              checked={deviationEnabled}
              data-testid="arbiter-enabled-switch"
              onChange={() =>
                putArbiterSettings({ deviation_arbiter_enabled: !deviationEnabled })
              }
            />
          }
        />
        <FormControlLabel
          label="Limit enforcement"
          control={
            <Switch
              checked={limitEnabled}
              data-testid="limit-enforcement-switch"
              onChange={() =>
                putArbiterSettings({ limit_enforcement_enabled: !limitEnabled })
              }
            />
          }
        />
      </CardActions>
    </Card>
  );
}
