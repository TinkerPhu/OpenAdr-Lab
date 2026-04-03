import {
  Chip,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { useTrace } from "../api/hooks";
import type { TraceEntry } from "../api/types";

// ─── Event type chip ──────────────────────────────────────────────────────────

function TypeChip({ type: t }: { type: string }) {
  const color =
    t === "OpenAdrArrived"    ? "success" :
    t === "OpenAdrExpired"    ? "warning" :
    t === "RateChange"        ? "info"    :
    t === "CapacityChange"    ? "secondary" :
    t === "PlanCycle"         ? "primary" :
    t === "PacketTransition"  ? "default" :
    t === "RequestTransition" ? "default" : "default";
  return <Chip label={t} size="small" color={color} />;
}

// ─── Detail cell — one line per variant ──────────────────────────────────────

function DetailCell({ entry }: { entry: TraceEntry }) {
  switch (entry.type) {
    case "OpenAdrArrived":
      return (
        <span>
          <b>{entry.event_name}</b> · {entry.signal_type} · {entry.value} (interval {entry.interval})
        </span>
      );
    case "OpenAdrExpired":
      return <span><b>{entry.event_name}</b></span>;
    case "RateChange":
      return (
        <span>
          import {entry.import_eur_kwh.toFixed(4)} €/kWh · export {entry.export_eur_kwh.toFixed(4)} €/kWh
          · from {new Date(entry.interval_start).toLocaleTimeString()}
        </span>
      );
    case "CapacityChange":
      return (
        <span>
          import {entry.import_limit_kw != null ? `${entry.import_limit_kw} kW` : "—"}
          · export {entry.export_limit_kw != null ? `${entry.export_limit_kw} kW` : "—"}
        </span>
      );
    case "PlanCycle":
      return (
        <span>
          <b>{entry.trigger_reason}</b> · firm {entry.firm_slots} · flex {entry.flexible_slots}
        </span>
      );
    case "PacketTransition":
      return (
        <span>
          <b>{entry.asset_id}</b> · {entry.from_status} → {entry.to_status}
          · <code style={{ fontSize: "0.7rem" }}>{entry.packet_id.slice(0, 8)}</code>
        </span>
      );
    case "RequestTransition":
      return (
        <span>
          <b>{entry.asset_id}</b> · {entry.from_status} → {entry.to_status}
          · <code style={{ fontSize: "0.7rem" }}>{entry.request_id.slice(0, 8)}</code>
        </span>
      );
  }
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export function TracePage() {
  const { data: entries, dataUpdatedAt } = useTrace();
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="trace-heading">
          Decision Trace
        </Typography>
        <Typography variant="body2" color="text.secondary">
          Last {entries?.length ?? 0} controller events (newest first). Updated: {lastUpdated}
        </Typography>
      </div>

      <TableContainer component={Paper}>
        <Table size="small" data-testid="trace-table">
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell>Event</TableCell>
              <TableCell>Detail</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {entries?.map((entry, i) => (
              <TableRow key={i} data-testid={`trace-row-${i}`}>
                <TableCell sx={{ whiteSpace: "nowrap", fontSize: "0.75rem" }}>
                  {new Date(entry.ts).toLocaleTimeString()}
                </TableCell>
                <TableCell>
                  <TypeChip type={entry.type} />
                </TableCell>
                <TableCell sx={{ fontSize: "0.75rem" }}>
                  <DetailCell entry={entry} />
                </TableCell>
              </TableRow>
            ))}
            {(!entries || entries.length === 0) && (
              <TableRow>
                <TableCell colSpan={3} align="center">
                  <Typography color="text.secondary">
                    No trace events yet
                  </Typography>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </Stack>
  );
}
