import { Chip, Stack, Tooltip } from "@mui/material";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import BoltIcon from "@mui/icons-material/Bolt";
import SpeedIcon from "@mui/icons-material/Speed";
import CompressIcon from "@mui/icons-material/Compress";
import { useSignals } from "../../api/hooks";

function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** "until HH:MM" for an active window, "from HH:MM" for an upcoming one —
 * the /signals aggregate already drops ended windows server-side. */
function windowLabel(startIso: string, endIso: string): string {
  return new Date(startIso).getTime() > Date.now()
    ? `from ${fmtTime(startIso)}`
    : `until ${fmtTime(endIso)}`;
}

/** WP4.6: at-a-glance strip of the active grid signals (alert / SIMPLE /
 *  dispatch / capacity). Renders nothing when no signal is active, so the
 *  page stays uncluttered in normal operation. */
export function GridSignalStrip() {
  const { data } = useSignals();
  if (!data) return null;

  const { alerts, simple, dispatch, capacity } = data;
  const maxSimple = simple.reduce((m, w) => Math.max(m, w.level), 0);
  const activeDispatch = dispatch[0];
  const capParts: string[] = [];
  if (capacity.import_limit_kw != null) capParts.push(`limit ${capacity.import_limit_kw} kW`);
  if (capacity.import_subscription_kw != null)
    capParts.push(`subscription ${capacity.import_subscription_kw} kW`);
  if (capacity.import_reservation_kw != null)
    capParts.push(`reservation ${capacity.import_reservation_kw} kW`);

  const anyActive =
    alerts.length > 0 || maxSimple > 0 || activeDispatch != null || capParts.length > 0;
  if (!anyActive) return null;

  return (
    <Stack direction="row" spacing={1} sx={{ mb: 1, flexWrap: "wrap" }} data-testid="signal-strip">
      {alerts.map((a) => (
        <Tooltip key={a.event_id} title={a.message}>
          <Chip
            icon={<WarningAmberIcon />}
            color="error"
            size="small"
            data-testid="signal-chip-alert"
            label={`Alert ${a.alert_type} ${windowLabel(a.start, a.end)}`}
          />
        </Tooltip>
      ))}
      {maxSimple > 0 && (
        <Chip
          icon={<CompressIcon />}
          color="warning"
          size="small"
          data-testid="signal-chip-simple"
          label={`SIMPLE L${maxSimple} ${windowLabel(
            simple.filter((w) => w.level === maxSimple)[0].start,
            simple.filter((w) => w.level === maxSimple)[0].end,
          )}`}
        />
      )}
      {activeDispatch && (
        <Chip
          icon={<SpeedIcon />}
          color="info"
          size="small"
          data-testid="signal-chip-dispatch"
          label={`Dispatch ${activeDispatch.setpoint_kw} kW ${windowLabel(activeDispatch.start, activeDispatch.end)}`}
        />
      )}
      {capParts.length > 0 && (
        <Chip
          icon={<BoltIcon />}
          size="small"
          data-testid="signal-chip-capacity"
          label={`Capacity: ${capParts.join(", ")}`}
        />
      )}
    </Stack>
  );
}
