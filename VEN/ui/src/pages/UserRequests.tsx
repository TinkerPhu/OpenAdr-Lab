import { useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  IconButton,
  InputLabel,
  MenuItem,
  Paper,
  Select,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import DeleteIcon from "@mui/icons-material/Delete";
import { useRequests, usePostRequest, useDeleteRequest } from "../api/hooks";
import type { CreateUserRequestBody, UserRequest, UserRequestStatus } from "../api/types";

const DEADLINES_PLACEHOLDER = JSON.stringify(
  [{ latest_end: "2026-03-12T22:00:00Z", max_total_cost_eur: 2.5, min_completion: 0.8 }],
  null,
  2,
);

function statusColor(
  status: UserRequestStatus,
): "primary" | "success" | "default" | "error" | "warning" {
  switch (status) {
    case "ACTIVE":     return "primary";
    case "COMPLETED":  return "success";
    case "CANCELLED":  return "default";
    case "FAILED":     return "error";
    default:           return "warning";
  }
}

// ── Create Dialog ─────────────────────────────────────────────────────────────

function CreateUserRequestDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const postMut = usePostRequest();

  const [assetId, setAssetId] = useState("");
  const [targetSoc, setTargetSoc] = useState("");
  const [targetEnergyKwh, setTargetEnergyKwh] = useState("");
  const [desiredPowerKw, setDesiredPowerKw] = useState("");
  const [completionPolicy, setCompletionPolicy] = useState("STOP");
  const [deadlinesJson, setDeadlinesJson] = useState(DEADLINES_PLACEHOLDER);
  const [error, setError] = useState<string | null>(null);

  let deadlinesValid = false;
  try {
    const parsed = JSON.parse(deadlinesJson);
    deadlinesValid = Array.isArray(parsed) && parsed.length > 0;
  } catch {
    deadlinesValid = false;
  }

  const submitDisabled = !assetId.trim() || !deadlinesValid || postMut.isPending;

  async function handleSubmit() {
    setError(null);
    const body: CreateUserRequestBody = {
      asset_id: assetId.trim(),
      target_soc: targetSoc !== "" ? parseFloat(targetSoc) : null,
      target_energy_kwh: targetEnergyKwh !== "" ? parseFloat(targetEnergyKwh) : null,
      desired_power_kw: desiredPowerKw !== "" ? parseFloat(desiredPowerKw) : null,
      completion_policy: completionPolicy || null,
      deadlines: JSON.parse(deadlinesJson),
      comfort_rates: null,
    };
    try {
      await postMut.mutateAsync(body);
      // reset and close
      setAssetId("");
      setTargetSoc("");
      setTargetEnergyKwh("");
      setDesiredPowerKw("");
      setCompletionPolicy("STOP");
      setDeadlinesJson(DEADLINES_PLACEHOLDER);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>New User Request</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2, mt: 1 }}>
          {error && <Alert severity="error">{error}</Alert>}

          <TextField
            label="Asset ID"
            value={assetId}
            onChange={(e) => setAssetId(e.target.value)}
            required
            placeholder="ev"
            size="small"
          />

          <TextField
            label="Target SoC (0.0–1.0)"
            value={targetSoc}
            onChange={(e) => setTargetSoc(e.target.value)}
            type="number"
            inputProps={{ min: 0, max: 1, step: 0.05 }}
            size="small"
            placeholder="0.8"
            helperText="Optional — percentage as decimal (e.g. 0.8 = 80%)"
          />

          <TextField
            label="Target Energy (kWh)"
            value={targetEnergyKwh}
            onChange={(e) => setTargetEnergyKwh(e.target.value)}
            type="number"
            inputProps={{ min: 0, step: 0.1 }}
            size="small"
            helperText="Optional — set if target_soc not provided"
          />

          <TextField
            label="Desired Power (kW)"
            value={desiredPowerKw}
            onChange={(e) => setDesiredPowerKw(e.target.value)}
            type="number"
            inputProps={{ min: 0, step: 0.1 }}
            size="small"
            placeholder="3.7"
            helperText="Optional — max charge rate"
          />

          <FormControl size="small">
            <InputLabel>Completion Policy</InputLabel>
            <Select
              value={completionPolicy}
              label="Completion Policy"
              onChange={(e) => setCompletionPolicy(e.target.value)}
            >
              <MenuItem value="STOP">STOP</MenuItem>
              <MenuItem value="CONTINUE">CONTINUE</MenuItem>
            </Select>
          </FormControl>

          <TextField
            label="Deadlines (JSON array)"
            value={deadlinesJson}
            onChange={(e) => setDeadlinesJson(e.target.value)}
            multiline
            rows={5}
            required
            error={!deadlinesValid}
            helperText={
              deadlinesValid
                ? "Valid JSON array"
                : "Must be a non-empty JSON array with at least one deadline"
            }
            size="small"
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={postMut.isPending}>
          Cancel
        </Button>
        <Button
          onClick={handleSubmit}
          variant="contained"
          disabled={submitDisabled}
          startIcon={postMut.isPending ? <CircularProgress size={14} /> : undefined}
        >
          Submit
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ── Cancel Confirm Dialog ─────────────────────────────────────────────────────

function CancelUserRequestDialog({
  userRequest,
  onClose,
}: {
  userRequest: UserRequest | null;
  onClose: () => void;
}) {
  const deleteMut = useDeleteRequest();
  const [error, setError] = useState<string | null>(null);

  async function handleConfirm() {
    if (!userRequest) return;
    setError(null);
    try {
      await deleteMut.mutateAsync(userRequest.id);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <Dialog open={!!userRequest} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>Cancel User Request</DialogTitle>
      <DialogContent>
        {error && <Alert severity="error" sx={{ mb: 1 }}>{error}</Alert>}
        <Typography>
          Cancel user request for asset <strong>{userRequest?.asset_id}</strong>? The associated
          packet will be ABANDONED.
        </Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={deleteMut.isPending}>
          Close
        </Button>
        <Button
          onClick={handleConfirm}
          color="error"
          variant="contained"
          disabled={deleteMut.isPending}
          startIcon={deleteMut.isPending ? <CircularProgress size={14} /> : undefined}
        >
          Confirm Cancel
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────────────

export function UserRequestsPage() {
  const { data: userRequests, isLoading, isError, error } = useRequests();
  const [createOpen, setCreateOpen] = useState(false);
  const [cancelTarget, setCancelTarget] = useState<UserRequest | null>(null);

  return (
    <Box>
      <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between", mb: 2 }}>
        <Typography variant="h5">User Requests</Typography>
        <Button variant="contained" onClick={() => setCreateOpen(true)}>
          New User Request
        </Button>
      </Box>

      {isLoading && <CircularProgress />}
      {isError && <Alert severity="error">{String(error)}</Alert>}

      {userRequests && (
        <TableContainer component={Paper}>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>Asset</TableCell>
                <TableCell>Target</TableCell>
                <TableCell>Deadline (first)</TableCell>
                <TableCell>Policy</TableCell>
                <TableCell>Status</TableCell>
                <TableCell align="right">Est. Cost €</TableCell>
                <TableCell align="right">Est. CO₂ g</TableCell>
                <TableCell>Created</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {userRequests.length === 0 && (
                <TableRow>
                  <TableCell colSpan={9} align="center" sx={{ color: "text.secondary" }}>
                    No user requests yet
                  </TableCell>
                </TableRow>
              )}
              {userRequests.map((req) => {
                const firstDeadline = req.deadlines?.[0];
                const target =
                  req.target_soc != null
                    ? `SoC ${(req.target_soc * 100).toFixed(0)}%`
                    : req.target_energy_kwh != null
                    ? `${req.target_energy_kwh.toFixed(1)} kWh`
                    : "—";

                return (
                  <TableRow key={req.id} hover>
                    <TableCell>{req.asset_id}</TableCell>
                    <TableCell>{target}</TableCell>
                    <TableCell>
                      {firstDeadline
                        ? new Date(firstDeadline.latest_end).toLocaleString()
                        : "—"}
                    </TableCell>
                    <TableCell>{req.completion_policy ?? "—"}</TableCell>
                    <TableCell>
                      <Chip
                        label={req.status}
                        color={statusColor(req.status)}
                        size="small"
                      />
                    </TableCell>
                    <TableCell align="right">
                      {req.estimated_cost_eur != null
                        ? req.estimated_cost_eur.toFixed(3)
                        : "—"}
                    </TableCell>
                    <TableCell align="right">
                      {req.estimated_co2_g != null
                        ? req.estimated_co2_g.toFixed(1)
                        : "—"}
                    </TableCell>
                    <TableCell>
                      {new Date(req.created_at).toLocaleString()}
                    </TableCell>
                    <TableCell padding="none">
                      <Tooltip title="Cancel user request">
                        <span>
                          <IconButton
                            size="small"
                            onClick={() => setCancelTarget(req)}
                            disabled={req.status !== "ACTIVE"}
                          >
                            <DeleteIcon fontSize="small" />
                          </IconButton>
                        </span>
                      </Tooltip>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      <CreateUserRequestDialog open={createOpen} onClose={() => setCreateOpen(false)} />
      <CancelUserRequestDialog userRequest={cancelTarget} onClose={() => setCancelTarget(null)} />
    </Box>
  );
}
