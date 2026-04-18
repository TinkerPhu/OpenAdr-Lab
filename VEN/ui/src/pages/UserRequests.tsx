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
  IconButton,
  Paper,
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
import type { CreateUserRequestBody, UserRequestWithSession, UserRequestStatus } from "../api/types";

function evExample(): string {
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(7, 0, 0, 0);
  const deadline = tomorrow.toISOString().replace(".000", "");
  const body: CreateUserRequestBody = {
    asset_id: "ev",
    target_soc: 0.80,
    target_energy_kwh: null,
    desired_power_kw: 7.0,
    completion_policy: "CONTINUE",
    deadlines: [
      {
        latest_end: deadline,
        max_total_cost_eur: 3.00,
        max_marginal_rate_eur_kwh: null,
        min_completion: 0.8,
      },
    ],
    comfort_rates: null,
  };
  return JSON.stringify(body, null, 2);
}

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
  const [json, setJson] = useState(() => evExample());
  const [error, setError] = useState<string | null>(null);

  let parsed: CreateUserRequestBody | null = null;
  let jsonError: string | null = null;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    jsonError = e instanceof Error ? e.message : "Invalid JSON";
  }

  const submitDisabled = parsed === null || postMut.isPending;

  async function handleSubmit() {
    if (!parsed) return;
    setError(null);
    try {
      await postMut.mutateAsync(parsed);
      setJson(evExample());
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
            label="Request JSON"
            value={json}
            onChange={(e) => setJson(e.target.value)}
            multiline
            rows={18}
            required
            error={jsonError !== null}
            helperText={jsonError ?? "Edit the JSON and click Submit"}
            size="small"
            inputProps={{ style: { fontFamily: "monospace", fontSize: 12 } }}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => setJson(evExample())} disabled={postMut.isPending}>
          Reset to EV example
        </Button>
        <Box sx={{ flex: 1 }} />
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
  userRequest: UserRequestWithSession | null;
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
  const [cancelTarget, setCancelTarget] = useState<UserRequestWithSession | null>(null);

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
