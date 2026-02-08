import {
  Box, Chip, Dialog, DialogContent, DialogTitle, Divider, IconButton,
  Stack, Table, TableBody, TableCell, TableContainer, TableHead,
  TableRow, Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import type { Interval, VtnEvent } from "../api/types";
import { getEventStatus, statusColor } from "../utils/eventStatus";

const PAYLOAD_LABELS: Record<string, string> = {
  SIMPLE: "Simple Signal",
  PRICE: "Price Signal",
  IMPORT_CAPACITY_LIMIT: "Import Capacity Limit",
  EXPORT_CAPACITY_LIMIT: "Export Capacity Limit",
  CHARGE_STATE_SETPOINT: "Charge State Setpoint",
};

function payloadLabel(type: string): string {
  return PAYLOAD_LABELS[type] ?? type;
}

function formatValues(payloads: Interval["payloads"]): string {
  if (!payloads || payloads.length === 0) return "—";
  return payloads.map((p) => `${payloadLabel(p.type)}: ${p.values.join(", ")}`).join("; ");
}

type EventDetailPanelProps = {
  open: boolean;
  event: VtnEvent | null;
  programName: string | null;
  onClose: () => void;
};

export function EventDetailPanel(props: EventDetailPanelProps) {
  const { open, event, programName, onClose } = props;
  if (!event) return null;

  const status = getEventStatus(event);
  const intervals = event.intervals ?? [];
  const targets = event.targets ?? [];

  return (
    <Dialog
      open={open}
      onClose={onClose}
      fullWidth
      maxWidth="md"
      data-testid="event-detail-panel"
      aria-modal="true"
      aria-labelledby="event-detail-title"
    >
      <DialogTitle
        id="event-detail-title"
        data-testid="event-detail-title"
        sx={{ display: "flex", alignItems: "center", gap: 1 }}
      >
        {event.eventName ?? event.id}
        <Chip
          label={status}
          size="small"
          color={statusColor(status)}
          variant={status === "completed" ? "outlined" : "filled"}
        />
        {event.priority != null && (
          <Chip label={`Priority: ${event.priority}`} size="small" variant="outlined" />
        )}
        <span style={{ flex: 1 }} />
        <IconButton
          onClick={onClose}
          size="small"
          data-testid="event-detail-close"
          aria-label="Close dialog"
        >
          <CloseIcon />
        </IconButton>
      </DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          {/* Summary */}
          <Box>
            <Typography variant="body2" color="text.secondary">
              Program: {programName ?? event.programID ?? "—"}
            </Typography>
            {event.intervalPeriod && (
              <Typography variant="body2" color="text.secondary">
                Start: {new Date(event.intervalPeriod.start).toLocaleString()}
                {event.intervalPeriod.duration && ` | Duration: ${event.intervalPeriod.duration}`}
              </Typography>
            )}
            {event.createdDateTime && (
              <Typography variant="body2" color="text.secondary">
                Created: {event.createdDateTime}
              </Typography>
            )}
          </Box>

          {/* Targets */}
          {targets.length > 0 && (
            <Box>
              <Typography variant="subtitle2">Targets</Typography>
              <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
                {targets.map((t, i) => (
                  <Chip key={i} label={`${t.type}: ${t.values.join(", ")}`} size="small" variant="outlined" />
                ))}
              </Stack>
            </Box>
          )}

          <Divider />

          {/* Intervals table */}
          <Box>
            <Typography variant="subtitle2" sx={{ mb: 1 }}>
              Intervals ({intervals.length})
            </Typography>
            <TableContainer>
              <Table size="small" data-testid="event-detail-intervals">
                <TableHead>
                  <TableRow>
                    <TableCell>ID</TableCell>
                    <TableCell>Start</TableCell>
                    <TableCell>Duration</TableCell>
                    <TableCell>Payload</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {intervals.map((iv) => (
                    <TableRow key={iv.id}>
                      <TableCell>{iv.id}</TableCell>
                      <TableCell>
                        {iv.intervalPeriod?.start
                          ? new Date(iv.intervalPeriod.start).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell>{iv.intervalPeriod?.duration ?? "—"}</TableCell>
                      <TableCell>{formatValues(iv.payloads)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
          </Box>

          <Divider />

          {/* Raw JSON */}
          <Box>
            <Typography variant="subtitle2" sx={{ mb: 1 }}>Raw JSON</Typography>
            <pre
              data-testid="event-detail-json"
              style={{ margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", fontSize: "0.8rem", maxHeight: 300, overflow: "auto" }}
            >
              {JSON.stringify(event, null, 2)}
            </pre>
          </Box>
        </Stack>
      </DialogContent>
    </Dialog>
  );
}
