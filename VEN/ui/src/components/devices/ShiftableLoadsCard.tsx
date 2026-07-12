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
  IconButton,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import type { CreateUserRequestBody, UserRequestMode, UserRequestWithSession } from "../../api/types";
import { ModeSelect } from "./ModeSelect";

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

export type ShiftableLoadsCardProps = {
  loads: UserRequestWithSession[];
  postRequest: (body: CreateUserRequestBody) => Promise<unknown>;
  deleteRequest: (id: string) => Promise<unknown>;
  isPosting: boolean;
  isDeleting: boolean;
};

// ── Component ────────────────────────────────────────────────────────────────

export function ShiftableLoadsCard(props: ShiftableLoadsCardProps) {
  const { loads, postRequest, deleteRequest, isPosting, isDeleting } = props;

  const [dialogOpen, setDialogOpen] = useState(false);
  const [assetId, setAssetId] = useState("wm");
  const [powerKw, setPowerKw] = useState("2.0");
  const [durationMin, setDurationMin] = useState("60");
  const [earliestStart, setEarliestStart] = useState(defaultDateTime(0));
  const [latestEnd, setLatestEnd] = useState(defaultDateTime(4));
  const [mode, setMode] = useState<UserRequestMode>("BY_DEADLINE");

  function handleConfirm() {
    postRequest({
      asset_id: assetId,
      target_soc: null,
      target_energy_kwh: null,
      desired_power_kw: null,
      completion_policy: null,
      deadlines: [],
      comfort_rates: null,
      mode,
      power_kw: Number(powerKw),
      duration_min: Number(durationMin),
      earliest_start: new Date(earliestStart).toISOString(),
      latest_end: new Date(latestEnd).toISOString(),
    });
    setDialogOpen(false);
  }

  return (
    <Card sx={{ height: "100%" }} data-testid="shiftable-card">
      <CardHeader title="Shiftable Loads" />
      <CardContent>
        {loads.length === 0 ? (
          <Typography color="text.secondary" data-testid="shiftable-empty">
            No loads scheduled
          </Typography>
        ) : (
          <Stack spacing={1}>
            {loads.map((req) => {
              const s = req.session?.type === "shiftable_load" ? req.session : null;
              return (
                <Box
                  key={req.id}
                  data-testid={`shiftable-row-${req.id}`}
                  sx={{ display: "flex", alignItems: "center", gap: 1 }}
                >
                  <Typography variant="body2" sx={{ flex: 1 }}>
                    {s ? `${s.asset_id} · ${s.power_kw}kW · ${s.duration_min}min · by ${fmtDate(s.latest_end)}` : req.asset_id}
                  </Typography>
                  {s && s.mode !== "BY_DEADLINE" && (
                    <Chip label={s.mode} size="small" data-testid={`shiftable-mode-chip-${req.id}`} />
                  )}
                  <IconButton
                    size="small"
                    data-testid={`shiftable-cancel-${req.id}`}
                    disabled={isDeleting}
                    onClick={() => deleteRequest(req.id)}
                  >
                    <CloseIcon fontSize="small" />
                  </IconButton>
                </Box>
              );
            })}
          </Stack>
        )}
      </CardContent>

      <CardActions sx={{ px: 2 }}>
        <Button
          variant="outlined"
          size="small"
          data-testid="shiftable-add-btn"
          disabled={isPosting}
          onClick={() => setDialogOpen(true)}
        >
          Add Load
        </Button>
      </CardActions>

      {/* Add Load Dialog */}
      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="shiftable-dialog">
        <DialogTitle>Add Shiftable Load</DialogTitle>
        <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 300, pt: 2 }}>
          <TextField
            label="Asset ID"
            value={assetId}
            onChange={(e) => setAssetId(e.target.value)}
            data-testid="shiftable-asset-input"
          />
          <TextField
            label="Power (kW)"
            type="number"
            value={powerKw}
            onChange={(e) => setPowerKw(e.target.value)}
            inputProps={{ min: 0.1, step: 0.1 }}
            data-testid="shiftable-power-input"
          />
          <TextField
            label="Duration (min)"
            type="number"
            value={durationMin}
            onChange={(e) => setDurationMin(e.target.value)}
            inputProps={{ min: 1, step: 1 }}
            data-testid="shiftable-duration-input"
          />
          <TextField
            label="Earliest Start"
            type="datetime-local"
            value={earliestStart}
            onChange={(e) => setEarliestStart(e.target.value)}
            InputLabelProps={{ shrink: true }}
            inputProps={{ lang: "de" }}
            data-testid="shiftable-earliest-input"
          />
          <TextField
            label="Latest End"
            type="datetime-local"
            value={latestEnd}
            onChange={(e) => setLatestEnd(e.target.value)}
            InputLabelProps={{ shrink: true }}
            inputProps={{ lang: "de" }}
            data-testid="shiftable-latest-input"
          />
          <ModeSelect value={mode} onChange={setMode} testId="shiftable-mode-select" />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="shiftable-dialog-cancel">Cancel</Button>
          <Button variant="contained" onClick={handleConfirm} data-testid="shiftable-dialog-confirm" disabled={isPosting}>
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
