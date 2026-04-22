import { useState } from "react";
import {
  Box,
  Button,
  Card,
  CardActions,
  CardContent,
  CardHeader,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControlLabel,
  Slider,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import type {
  CreateUserRequestBody,
  EvSettings,
  UpdateEvSettingsBody,
  UserRequestWithSession,
} from "../../api/types";

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
    hour12: false,
  });
}

// ── Props ────────────────────────────────────────────────────────────────────

export type EvCardProps = {
  request: UserRequestWithSession | undefined;
  evSettings: EvSettings | undefined;
  postRequest: (body: CreateUserRequestBody) => Promise<unknown>;
  deleteRequest: (id: string) => Promise<unknown>;
  putEvSettings: (body: UpdateEvSettingsBody) => void;
  isPosting: boolean;
  isDeleting: boolean;
};

// ── Component ────────────────────────────────────────────────────────────────

export function EvCard(props: EvCardProps) {
  const { request, evSettings, postRequest, deleteRequest, putEvSettings, isPosting, isDeleting } = props;

  const [dialogOpen, setDialogOpen] = useState(false);
  const [targetSoc, setTargetSoc] = useState(80);
  const [departure, setDeparture] = useState(defaultDateTime(8));
  const [softDeadline, setSoftDeadline] = useState(false);

  const session = request?.session?.type === "ev" ? request.session : null;
  const paused = evSettings?.paused_by_active_session ?? false;
  const oppEnabled = evSettings?.opportunistic_charging_enabled ?? false;

  function handleConfirm() {
    const dt = new Date(departure);
    postRequest({
      asset_id: "ev",
      target_soc: targetSoc / 100,
      target_energy_kwh: null,
      desired_power_kw: 7.0,
      soft_deadline: softDeadline,
      completion_policy: "CONTINUE",
      deadlines: [{
        latest_end: dt.toISOString(),
        max_total_cost_eur: null,
        max_marginal_rate_eur_kwh: null,
        min_completion: targetSoc / 100,
      }],
      comfort_rates: null,
    });
    setDialogOpen(false);
  }

  return (
    <Card sx={{ height: "100%" }} data-testid="ev-card">
      <CardHeader title="EV Charging" />
      <CardContent>
        {session && request ? (
          <Box data-testid="ev-active-view">
            <Chip label="ACTIVE" color="success" size="small" data-testid="ev-status-chip" sx={{ mb: 1 }} />
            <Typography data-testid="ev-target-soc">
              → {(session.target_soc * 100).toFixed(0)}% SoC
            </Typography>
            <Typography data-testid="ev-departure">
              Depart: {fmtDate(session.departure_time)}
            </Typography>
            {session.soft_deadline && (
              <Chip label="Soft deadline" size="small" data-testid="ev-soft-deadline-chip" sx={{ mt: 0.5 }} />
            )}
            <Typography data-testid="ev-estimated-cost" sx={{ mt: 0.5 }}>
              Est. €{request.estimated_cost_eur.toFixed(2)}
            </Typography>
            <Button
              variant="outlined"
              color="warning"
              size="small"
              sx={{ mt: 1 }}
              data-testid="ev-unplan-btn"
              disabled={isDeleting}
              onClick={() => deleteRequest(request.id)}
            >
              Unplan
            </Button>
          </Box>
        ) : (
          <Box data-testid="ev-idle-view">
            <Typography color="text.secondary">No session planned</Typography>
            <Button
              variant="contained"
              size="small"
              sx={{ mt: 1 }}
              data-testid="ev-plan-btn"
              disabled={isPosting}
              onClick={() => setDialogOpen(true)}
            >
              Plan Charging
            </Button>
          </Box>
        )}
      </CardContent>

      <Divider />

      <CardActions data-testid="ev-settings-section" sx={{ px: 2, flexDirection: "column", alignItems: "flex-start" }}>
        <FormControlLabel
          label="Automatic surplus charging"
          control={
            <Switch
              checked={oppEnabled}
              disabled={paused}
              data-testid="ev-opportunistic-charging-switch"
              onChange={() => putEvSettings({ opportunistic_charging_enabled: !oppEnabled })}
            />
          }
        />
        {paused && (
          <Chip label="Paused — active plan" size="small" data-testid="ev-opportunistic-paused-chip" />
        )}
      </CardActions>

      {/* Plan EV Dialog */}
      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="ev-dialog">
        <DialogTitle>Plan EV Charging</DialogTitle>
        <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 300, pt: 2 }}>
          <Typography gutterBottom>Target SoC: {targetSoc}%</Typography>
          <Slider
            value={targetSoc}
            onChange={(_, v) => setTargetSoc(v as number)}
            min={20}
            max={100}
            step={5}
            data-testid="ev-soc-slider"
          />
          <TextField
            label="Departure"
            type="datetime-local"
            value={departure}
            onChange={(e) => setDeparture(e.target.value)}
            InputLabelProps={{ shrink: true }}
            inputProps={{ lang: "de" }}
            data-testid="ev-departure-input"
          />
          <FormControlLabel
            label="Soft deadline"
            control={
              <Switch
                checked={softDeadline}
                onChange={(_, v) => setSoftDeadline(v)}
                data-testid="ev-soft-deadline-switch"
              />
            }
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="ev-dialog-cancel">Cancel</Button>
          <Button variant="contained" onClick={handleConfirm} data-testid="ev-dialog-confirm" disabled={isPosting}>
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
