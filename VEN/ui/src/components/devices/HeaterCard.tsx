import { useState } from "react";
import {
  Box,
  Button,
  Card,
  CardContent,
  CardHeader,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  TextField,
  Typography,
} from "@mui/material";
import type { CreateUserRequestBody, UserRequestWithSession } from "../../api/types";

// ── Helpers ──────────────────────────────────────────────────────────────────

function defaultDateTime(hoursOffset: number): string {
  const d = new Date();
  d.setHours(d.getHours() + hoursOffset);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

// ── Props ────────────────────────────────────────────────────────────────────

export type HeaterCardProps = {
  request: UserRequestWithSession | undefined;
  postRequest: (body: CreateUserRequestBody) => Promise<unknown>;
  deleteRequest: (id: string) => Promise<unknown>;
  isPosting: boolean;
  isDeleting: boolean;
};

// ── Component ────────────────────────────────────────────────────────────────

export function HeaterCard(props: HeaterCardProps) {
  const { request, postRequest, deleteRequest, isPosting, isDeleting } = props;

  const [dialogOpen, setDialogOpen] = useState(false);
  const [tempC, setTempC] = useState("55");
  const [readyBy, setReadyBy] = useState(defaultDateTime(4));

  const session = request?.session?.type === "heater" ? request.session : null;

  function handleConfirm() {
    const dt = new Date(readyBy);
    postRequest({
      asset_id: "heater",
      target_soc: null,
      target_energy_kwh: null,
      desired_power_kw: null,
      target_temp_c: Number(tempC),
      completion_policy: "STOP",
      deadlines: [{
        latest_end: dt.toISOString(),
        max_total_cost_eur: null,
        max_marginal_rate_eur_kwh: null,
        min_completion: 1.0,
      }],
      comfort_rates: null,
    });
    setDialogOpen(false);
  }

  return (
    <Card sx={{ height: "100%" }} data-testid="heater-card">
      <CardHeader title="Water Heater" />
      <CardContent>
        {session && request ? (
          <Box data-testid="heater-active-view">
            <Chip label="ACTIVE" color="success" size="small" data-testid="heater-status-chip" sx={{ mb: 1 }} />
            <Typography data-testid="heater-temp">
              → {session.target_temp_c}°C
            </Typography>
            <Typography data-testid="heater-ready-by">
              Ready by: {fmtDate(session.ready_by)}
            </Typography>
            <Typography data-testid="heater-estimated-cost" sx={{ mt: 0.5 }}>
              Est. €{request.estimated_cost_eur.toFixed(2)}
            </Typography>
            <Button
              variant="outlined"
              color="warning"
              size="small"
              sx={{ mt: 1 }}
              data-testid="heater-clear-btn"
              disabled={isDeleting}
              onClick={() => deleteRequest(request.id)}
            >
              Clear
            </Button>
          </Box>
        ) : (
          <Box data-testid="heater-idle-view">
            <Typography color="text.secondary">No target set</Typography>
            <Button
              variant="contained"
              size="small"
              sx={{ mt: 1 }}
              data-testid="heater-set-btn"
              disabled={isPosting}
              onClick={() => setDialogOpen(true)}
            >
              Set Target
            </Button>
          </Box>
        )}
      </CardContent>

      {/* Set Heater Dialog */}
      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="heater-dialog">
        <DialogTitle>Set Heater Target</DialogTitle>
        <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 300, pt: 2 }}>
          <TextField
            label="Target Temperature (°C)"
            type="number"
            value={tempC}
            onChange={(e) => setTempC(e.target.value)}
            inputProps={{ min: 30, max: 80, step: 1 }}
            data-testid="heater-temp-input"
          />
          <TextField
            label="Ready By"
            type="datetime-local"
            value={readyBy}
            onChange={(e) => setReadyBy(e.target.value)}
            InputLabelProps={{ shrink: true }}
            data-testid="heater-readyby-input"
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="heater-dialog-cancel">Cancel</Button>
          <Button variant="contained" onClick={handleConfirm} data-testid="heater-dialog-confirm" disabled={isPosting}>
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
