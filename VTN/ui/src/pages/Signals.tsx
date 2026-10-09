import { useMemo, useState } from "react";
import {
  Alert, MenuItem, Paper, Stack, TextField, Tooltip, Typography,
} from "@mui/material";
import { useFleetSignals } from "../api/hooks";
import { venColor } from "../utils/venColor";
import { byVenName } from "../utils/venOrder";
import { bandGeometry } from "../utils/signalBandGeometry";

const WINDOWS = [
  { minutes: 60, label: "1 hour" },
  { minutes: 360, label: "6 hours" },
  { minutes: 1440, label: "24 hours" },
];

/** One colour per payload type, so a PRICE band never reads as a limit. */
const TYPE_COLORS: Record<string, string> = {
  IMPORT_CAPACITY_LIMIT: "#c62828",
  EXPORT_CAPACITY_LIMIT: "#ad1457",
  PRICE: "#1565c0",
  EXPORT_PRICE: "#0277bd",
  GHG: "#2e7d32",
  SIMPLE: "#ef6c00",
  ALERT_GRID_EMERGENCY: "#b71c1c",
  DISPATCH_SETPOINT: "#4527a0",
};
const TYPE_FALLBACK = "#546e7a";

const colorForType = (t: string) => TYPE_COLORS[t] ?? TYPE_FALLBACK;

/**
 * §1: every signal the fleet was sent, resolved per VEN.
 *
 * The Fleet page answers "did the fleet react"; this answers "what was it
 * actually told, and which sites did it reach" — the question that comes first
 * when a targeted VEN turns out not to have been targeted at all.
 */
export function SignalsPage() {
  const [windowIndex, setWindowIndex] = useState(0);
  const chosen = WINDOWS[windowIndex];
  const signals = useFleetSignals(chosen.minutes);

  const window = useMemo(() => {
    if (!signals.data) return null;
    return { fromMs: Date.parse(signals.data.from), toMs: Date.parse(signals.data.to) };
  }, [signals.data]);

  // Same counting order as the Fleet tab: a reader scanning for ven-2 should
  // find it second in both places, not second here and thirteenth there.
  const venRows = byVenName(
    (signals.data?.vens ?? []).filter((v) => v.bands.length > 0),
    (v) => v.venName,
  );

  return (
    <Stack spacing={2}>
      <Paper sx={{ p: 2 }} data-testid="signals-card">
        <Stack direction="row" spacing={2} alignItems="center" mb={1}>
          <Typography variant="h6" data-testid="signals-heading">
            Signals sent to the fleet
          </Typography>
          <TextField
            select
            size="small"
            value={windowIndex}
            onChange={(e) => setWindowIndex(Number(e.target.value))}
            inputProps={{ "data-testid": "signals-window-select" }}
            sx={{ minWidth: 120 }}
          >
            {WINDOWS.map((w, i) => (
              <MenuItem key={w.minutes} value={i}>
                {w.label}
              </MenuItem>
            ))}
          </TextField>
        </Stack>

        {signals.isError && (
          <Alert severity="error" data-testid="signals-error">
            The signal feed could not be read.
          </Alert>
        )}

        {/* An event the BFF could not parse means this page is showing less
            than the VTN sent. Saying so beats a quietly shorter list. */}
        {signals.data && signals.data.rejectedEvents > 0 && (
          <Alert severity="warning" data-testid="signals-rejected">
            {signals.data.rejectedEvents} event(s) could not be read — this view is incomplete.
          </Alert>
        )}

        {signals.data && venRows.length === 0 && (
          <Typography variant="body2" data-testid="signals-empty">
            No VEN was under any signal in this window.
          </Typography>
        )}

        {window &&
          venRows.map((ven) => (
            <Stack
              key={ven.venName}
              direction="row"
              alignItems="center"
              spacing={1}
              sx={{ mb: 0.5 }}
              data-testid={`signals-row-${ven.venName}`}
            >
              <Typography
                variant="body2"
                sx={{ width: 90, color: venColor(ven.venName), fontFamily: "monospace" }}
              >
                {ven.venName}
              </Typography>
              <div
                style={{
                  position: "relative",
                  flex: 1,
                  height: 18,
                  background: "#f3f3f3",
                  borderRadius: 3,
                }}
              >
                {ven.bands.map((band, i) => {
                  const { leftPct, widthPct } = bandGeometry(band, window.fromMs, window.toMs);
                  const label =
                    `${band.payloadType}` +
                    (band.value !== null ? ` ${band.value}` : "") +
                    (band.unitsDeclaredBy === "PROFILE_DEFAULT" ? " (unit not declared, profile default assumed)" : "") +
                    (band.refused ? ` · not read: ${band.refused}` : "") +
                    ` · ${new Date(band.from).toLocaleTimeString()}–${new Date(band.to).toLocaleTimeString()}` +
                    (band.eventName ? ` · ${band.eventName}` : "");
                  return (
                    <Tooltip key={`${band.eventID}-${i}`} title={label}>
                      <div
                        data-testid={`signals-band-${ven.venName}-${i}`}
                        style={{
                          position: "absolute",
                          left: `${leftPct}%`,
                          width: `${widthPct}%`,
                          top: 0,
                          bottom: 0,
                          background: colorForType(band.payloadType),
                          opacity: 0.75,
                          borderRadius: 2,
                        }}
                      />
                    </Tooltip>
                  );
                })}
              </div>
            </Stack>
          ))}

        {window && venRows.length > 0 && (
          <Typography variant="caption" color="text.secondary" data-testid="signals-window-label">
            {new Date(window.fromMs).toLocaleTimeString()} –{" "}
            {new Date(window.toMs).toLocaleTimeString()}
          </Typography>
        )}
      </Paper>
    </Stack>
  );
}
