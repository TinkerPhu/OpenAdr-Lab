import { useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Collapse,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  Paper,
  Slider,
  Stack,
  Switch,
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
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import {
  useEvSession,
  usePostEvSession,
  useDeleteEvSession,
  useHeaterTarget,
  usePostHeaterTarget,
  useDeleteHeaterTarget,
  useShiftableLoads,
  usePostShiftableLoad,
  useDeleteShiftableLoad,
} from "../api/hooks";

// ── Helpers ──────────────────────────────────────────────────────────────────

function toLocalDatetime(iso: string): string {
  const d = new Date(iso);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

function defaultDeparture(): string {
  const d = new Date();
  d.setHours(d.getHours() + 8);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

function defaultReadyBy(): string {
  const d = new Date();
  d.setHours(d.getHours() + 2);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

function defaultEarliestStart(): string {
  const d = new Date();
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

function defaultLatestEnd(): string {
  const d = new Date();
  d.setHours(d.getHours() + 4);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

// ── EV Session Section ───────────────────────────────────────────────────────

function EvSessionSection() {
  const { data: session, isLoading, isError } = useEvSession();
  const postMut = usePostEvSession();
  const deleteMut = useDeleteEvSession();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [targetSoc, setTargetSoc] = useState(80);
  const [departure, setDeparture] = useState(defaultDeparture);
  const [opportunistic, setOpportunistic] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handlePlugIn = async () => {
    setError(null);
    try {
      await postMut.mutateAsync({
        target_soc: targetSoc / 100,
        departure_time: new Date(departure).toISOString(),
        opportunistic,
      });
      setDialogOpen(false);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleUnplug = async () => {
    setError(null);
    try {
      await deleteMut.mutateAsync();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (isLoading) return <CircularProgress data-testid="ev-loading" size={24} />;

  return (
    <Box data-testid="ev-session-section">
      {isError && <Alert severity="error" data-testid="ev-error">Failed to load EV session</Alert>}
      {error && <Alert severity="error" data-testid="ev-action-error">{error}</Alert>}

      {session ? (
        <Paper sx={{ p: 2, mb: 1 }} data-testid="ev-session-card">
          <Stack direction="row" spacing={2} alignItems="center">
            <Chip label="EV Plugged" color="success" data-testid="ev-status" />
            <Typography data-testid="ev-target-soc">
              Target SoC: {(session.target_soc * 100).toFixed(0)}%
            </Typography>
            <Typography data-testid="ev-departure">
              Departure: {new Date(session.departure_time).toLocaleString()}
            </Typography>
            {session.opportunistic && (
              <Chip label="Opportunistic" size="small" data-testid="ev-opportunistic" />
            )}
            <Button
              variant="outlined"
              color="error"
              size="small"
              onClick={handleUnplug}
              disabled={deleteMut.isPending}
              data-testid="ev-unplug-btn"
            >
              Unplug
            </Button>
          </Stack>
        </Paper>
      ) : (
        <Paper sx={{ p: 2, mb: 1 }} data-testid="ev-no-session">
          <Stack direction="row" spacing={2} alignItems="center">
            <Typography color="text.secondary">No active EV session</Typography>
            <Button
              variant="contained"
              size="small"
              onClick={() => setDialogOpen(true)}
              data-testid="ev-plugin-btn"
            >
              Plug In
            </Button>
          </Stack>
        </Paper>
      )}

      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="ev-dialog">
        <DialogTitle>Plug In EV</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1, minWidth: 300 }}>
            <Typography>Target SoC: {targetSoc}%</Typography>
            <Slider
              value={targetSoc}
              onChange={(_, v) => setTargetSoc(v as number)}
              min={20}
              max={100}
              step={5}
              data-testid="ev-soc-slider"
            />
            <TextField
              label="Departure Time"
              type="datetime-local"
              value={departure}
              onChange={(e) => setDeparture(e.target.value)}
              InputLabelProps={{ shrink: true }}
              data-testid="ev-departure-input"
            />
            <Stack direction="row" alignItems="center" spacing={1}>
              <Typography>Opportunistic</Typography>
              <Switch
                checked={opportunistic}
                onChange={(e) => setOpportunistic(e.target.checked)}
                data-testid="ev-opportunistic-switch"
              />
            </Stack>
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="ev-dialog-cancel">Cancel</Button>
          <Button
            variant="contained"
            onClick={handlePlugIn}
            disabled={postMut.isPending}
            data-testid="ev-dialog-confirm"
          >
            Plug In
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

// ── Heater Target Section ────────────────────────────────────────────────────

function HeaterTargetSection() {
  const { data: target, isLoading, isError } = useHeaterTarget();
  const postMut = usePostHeaterTarget();
  const deleteMut = useDeleteHeaterTarget();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [tempC, setTempC] = useState("55");
  const [readyBy, setReadyBy] = useState(defaultReadyBy);
  const [error, setError] = useState<string | null>(null);

  const handleSet = async () => {
    setError(null);
    try {
      await postMut.mutateAsync({
        target_temp_c: parseFloat(tempC),
        ready_by: new Date(readyBy).toISOString(),
      });
      setDialogOpen(false);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleClear = async () => {
    setError(null);
    try {
      await deleteMut.mutateAsync();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (isLoading) return <CircularProgress data-testid="heater-loading" size={24} />;

  return (
    <Box data-testid="heater-target-section">
      {isError && <Alert severity="error" data-testid="heater-error">Failed to load heater target</Alert>}
      {error && <Alert severity="error" data-testid="heater-action-error">{error}</Alert>}

      {target ? (
        <Paper sx={{ p: 2, mb: 1 }} data-testid="heater-target-card">
          <Stack direction="row" spacing={2} alignItems="center">
            <Chip label="Target Set" color="warning" data-testid="heater-status" />
            <Typography data-testid="heater-temp">
              Target: {target.target_temp_c}°C
            </Typography>
            <Typography data-testid="heater-ready-by">
              Ready By: {new Date(target.ready_by).toLocaleString()}
            </Typography>
            <Button
              variant="outlined"
              color="error"
              size="small"
              onClick={handleClear}
              disabled={deleteMut.isPending}
              data-testid="heater-clear-btn"
            >
              Clear
            </Button>
          </Stack>
        </Paper>
      ) : (
        <Paper sx={{ p: 2, mb: 1 }} data-testid="heater-no-target">
          <Stack direction="row" spacing={2} alignItems="center">
            <Typography color="text.secondary">No active heater target</Typography>
            <Button
              variant="contained"
              size="small"
              onClick={() => setDialogOpen(true)}
              data-testid="heater-set-btn"
            >
              Set Target
            </Button>
          </Stack>
        </Paper>
      )}

      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="heater-dialog">
        <DialogTitle>Set Heater Target</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1, minWidth: 300 }}>
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
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="heater-dialog-cancel">Cancel</Button>
          <Button
            variant="contained"
            onClick={handleSet}
            disabled={postMut.isPending}
            data-testid="heater-dialog-confirm"
          >
            Set
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

// ── Shiftable Loads Section ──────────────────────────────────────────────────

function ShiftableLoadsSection() {
  const { data: loads, isLoading, isError } = useShiftableLoads();
  const postMut = usePostShiftableLoad();
  const deleteMut = useDeleteShiftableLoad();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [assetId, setAssetId] = useState("wm");
  const [powerKw, setPowerKw] = useState("2.0");
  const [durationMin, setDurationMin] = useState("60");
  const [earliestStart, setEarliestStart] = useState(defaultEarliestStart);
  const [latestEnd, setLatestEnd] = useState(defaultLatestEnd);
  const [error, setError] = useState<string | null>(null);

  const handleAdd = async () => {
    setError(null);
    try {
      await postMut.mutateAsync({
        asset_id: assetId,
        power_kw: parseFloat(powerKw),
        duration_min: parseInt(durationMin, 10),
        earliest_start: new Date(earliestStart).toISOString(),
        latest_end: new Date(latestEnd).toISOString(),
      });
      setDialogOpen(false);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDelete = async (id: string) => {
    setError(null);
    try {
      await deleteMut.mutateAsync(id);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (isLoading) return <CircularProgress data-testid="shiftable-loading" size={24} />;

  return (
    <Box data-testid="shiftable-loads-section">
      {isError && <Alert severity="error" data-testid="shiftable-error">Failed to load shiftable loads</Alert>}
      {error && <Alert severity="error" data-testid="shiftable-action-error">{error}</Alert>}

      <Stack direction="row" spacing={1} sx={{ mb: 1 }}>
        <Button
          variant="contained"
          size="small"
          onClick={() => setDialogOpen(true)}
          data-testid="shiftable-add-btn"
        >
          Add Load
        </Button>
      </Stack>

      {loads && loads.length > 0 ? (
        <TableContainer component={Paper} data-testid="shiftable-table">
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>Asset</TableCell>
                <TableCell>Power (kW)</TableCell>
                <TableCell>Duration (min)</TableCell>
                <TableCell>Earliest Start</TableCell>
                <TableCell>Latest End</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {loads.map((load) => (
                <TableRow key={load.id} data-testid={`shiftable-row-${load.id}`}>
                  <TableCell data-testid={`shiftable-asset-${load.id}`}>{load.asset_id}</TableCell>
                  <TableCell data-testid={`shiftable-power-${load.id}`}>{load.power_kw}</TableCell>
                  <TableCell data-testid={`shiftable-duration-${load.id}`}>{load.duration_min}</TableCell>
                  <TableCell data-testid={`shiftable-start-${load.id}`}>
                    {new Date(load.earliest_start).toLocaleString()}
                  </TableCell>
                  <TableCell data-testid={`shiftable-end-${load.id}`}>
                    {new Date(load.latest_end).toLocaleString()}
                  </TableCell>
                  <TableCell>
                    <Tooltip title="Remove">
                      <IconButton
                        size="small"
                        onClick={() => handleDelete(load.id)}
                        disabled={deleteMut.isPending}
                        data-testid={`shiftable-delete-${load.id}`}
                      >
                        <DeleteIcon fontSize="small" />
                      </IconButton>
                    </Tooltip>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      ) : (
        <Paper sx={{ p: 2 }} data-testid="shiftable-empty">
          <Typography color="text.secondary">No shiftable loads scheduled</Typography>
        </Paper>
      )}

      <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} data-testid="shiftable-dialog">
        <DialogTitle>Add Shiftable Load</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1, minWidth: 300 }}>
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
              data-testid="shiftable-earliest-input"
            />
            <TextField
              label="Latest End"
              type="datetime-local"
              value={latestEnd}
              onChange={(e) => setLatestEnd(e.target.value)}
              InputLabelProps={{ shrink: true }}
              data-testid="shiftable-latest-input"
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="shiftable-dialog-cancel">Cancel</Button>
          <Button
            variant="contained"
            onClick={handleAdd}
            disabled={postMut.isPending}
            data-testid="shiftable-dialog-confirm"
          >
            Add
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

// ── Main Page ────────────────────────────────────────────────────────────────

export function DeviceSessionsPage() {
  const [evOpen, setEvOpen] = useState(true);
  const [heaterOpen, setHeaterOpen] = useState(true);
  const [shiftableOpen, setShiftableOpen] = useState(true);

  return (
    <Box data-testid="device-sessions-page" sx={{ py: 2 }}>
      <Typography variant="h5" gutterBottom data-testid="page-title">
        Device Sessions
      </Typography>

      {/* EV Session */}
      <Paper sx={{ mb: 2, overflow: "hidden" }}>
        <Stack
          direction="row"
          alignItems="center"
          sx={{ px: 2, py: 1, cursor: "pointer", bgcolor: "grey.100" }}
          onClick={() => setEvOpen(!evOpen)}
          data-testid="ev-section-header"
        >
          <Typography variant="subtitle1" sx={{ flexGrow: 1 }}>
            🔌 EV Charging
          </Typography>
          {evOpen ? <ExpandLessIcon /> : <ExpandMoreIcon />}
        </Stack>
        <Collapse in={evOpen}>
          <Box sx={{ p: 2 }}>
            <EvSessionSection />
          </Box>
        </Collapse>
      </Paper>

      {/* Heater Target */}
      <Paper sx={{ mb: 2, overflow: "hidden" }}>
        <Stack
          direction="row"
          alignItems="center"
          sx={{ px: 2, py: 1, cursor: "pointer", bgcolor: "grey.100" }}
          onClick={() => setHeaterOpen(!heaterOpen)}
          data-testid="heater-section-header"
        >
          <Typography variant="subtitle1" sx={{ flexGrow: 1 }}>
            🔥 Water Heater
          </Typography>
          {heaterOpen ? <ExpandLessIcon /> : <ExpandMoreIcon />}
        </Stack>
        <Collapse in={heaterOpen}>
          <Box sx={{ p: 2 }}>
            <HeaterTargetSection />
          </Box>
        </Collapse>
      </Paper>

      {/* Shiftable Loads */}
      <Paper sx={{ mb: 2, overflow: "hidden" }}>
        <Stack
          direction="row"
          alignItems="center"
          sx={{ px: 2, py: 1, cursor: "pointer", bgcolor: "grey.100" }}
          onClick={() => setShiftableOpen(!shiftableOpen)}
          data-testid="shiftable-section-header"
        >
          <Typography variant="subtitle1" sx={{ flexGrow: 1 }}>
            ⏱ Shiftable Loads
          </Typography>
          {shiftableOpen ? <ExpandLessIcon /> : <ExpandMoreIcon />}
        </Stack>
        <Collapse in={shiftableOpen}>
          <Box sx={{ p: 2 }}>
            <ShiftableLoadsSection />
          </Box>
        </Collapse>
      </Paper>
    </Box>
  );
}
