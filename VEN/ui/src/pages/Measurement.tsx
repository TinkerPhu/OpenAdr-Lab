import { Alert, Card, CardContent, Chip, Stack, Typography } from "@mui/material";
import { useMeasurement } from "../api/hooks";
import { MeasurementSignal } from "../api/types";

function SignalCard({ title, testId, signal }: { title: string; testId: string; signal: MeasurementSignal }) {
  return (
    <Card data-testid={testId} variant="outlined">
      <CardContent>
        <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1 }}>
          <Typography variant="subtitle1">{title}</Typography>
          <Chip
            size="small"
            data-testid={`${testId}-source-alive`}
            label={signal.source_alive ? "Source: Live" : "Source: Offline"}
            color={signal.source_alive ? "success" : "default"}
          />
          <Chip
            size="small"
            data-testid={`${testId}-status`}
            label={signal.status}
            color={signal.status === "ok" ? "success" : signal.status === "stale" ? "warning" : "default"}
          />
        </Stack>
        {signal.status === "disabled" && (
          <Alert severity="info" data-testid={`${testId}-disabled`}>
            Not enabled for this VEN (profile <code>measurements</code> section).
          </Alert>
        )}
        {signal.status === "not_configured" && (
          <Alert severity="info" data-testid={`${testId}-not-configured`}>
            No measurement feed configured (env var not set) — nothing received yet.
          </Alert>
        )}
        {signal.raw_kw !== null && (
          <Typography data-testid={`${testId}-raw-kw`}>
            {signal.raw_kw.toFixed(2)} kW{signal.raw_at ? ` @ ${signal.raw_at}` : ""}
          </Typography>
        )}
      </CardContent>
    </Card>
  );
}

export function MeasurementPage() {
  const { data, isLoading } = useMeasurement();

  return (
    <div data-testid="measurement-page">
      <Typography variant="h5" sx={{ mb: 2 }}>
        Real Measurements
      </Typography>
      {isLoading && <Typography color="text.secondary">Loading…</Typography>}
      {!isLoading && data && (
        <Stack spacing={2}>
          <SignalCard title="PV Power" testId="measurement-pv" signal={data.pv} />
          <SignalCard title="Baseline Load Power" testId="measurement-base-load" signal={data.base_load} />
        </Stack>
      )}
    </div>
  );
}
