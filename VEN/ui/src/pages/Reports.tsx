import { useMemo, useState } from "react";
import {
  Button, IconButton, MenuItem, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Tooltip, Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import AutoFixHighIcon from "@mui/icons-material/AutoFixHigh";
import EditIcon from "@mui/icons-material/Edit";
import type { Report, VtnEvent } from "../api/types";
import { useReports, useSubmitReport, useUpdateReport, useEvents, usePrograms } from "../api/hooks";
import { useVenContext } from "../App";
import { JsonDialog } from "../components/JsonDialog";

export function buildExampleResources(event: VtnEvent, venName: string): string {
  const intervals = (event.intervals ?? []).map((iv) => ({
    id: iv.id,
    payloads: (iv.payloads ?? []).map((p) => ({
      type: p.type,
      values: p.values.map((v) => {
        if (p.type === "SIMPLE" && v === 0) return 1;
        if (v === 0) return 0;
        const offset = 1 + (Math.random() * 0.08 - 0.04); // ±4%
        return Math.round(v * offset * 10) / 10;
      }),
    })),
  }));
  const resource = { resourceName: `${venName}-meter`, intervals };
  return JSON.stringify([resource], null, 2);
}

export function ReportsPage() {
  const { data: reports = [], dataUpdatedAt } = useReports();
  const { data: events = [] } = useEvents();
  const { data: programs = [] } = usePrograms();
  const submitMut = useSubmitReport();
  const updateMut = useUpdateReport();
  const { venName } = useVenContext();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Report | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editingReport, setEditingReport] = useState<Report | null>(null);

  // Form state
  const [eventID, setEventID] = useState("");
  const [reportName, setReportName] = useState("");
  const [resources, setResources] = useState("[]");

  const programMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of programs) {
      map.set(p.id, p.programName ?? p.id);
    }
    return map;
  }, [programs]);

  const filtered = useMemo(() => {
    return reports.filter((r) => {
      const hay = `${r.id} ${r.clientName ?? ""} ${r.reportName ?? ""} ${r.programID ?? ""} ${r.eventID ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [reports, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  // Derive programID from selected event
  const selectedEvent = events.find((e) => e.id === eventID);
  const programID = selectedEvent?.programID ?? "";

  let resourcesValid = true;
  try {
    JSON.parse(resources);
  } catch {
    resourcesValid = false;
  }

  const isEditing = editingReport !== null;

  function handleOpenForm() {
    setEditingReport(null);
    setEventID(events[0]?.id ?? "");
    setReportName("");
    setResources("[]");
    setFormOpen(true);
  }

  function handleEdit(report: Report) {
    setEditingReport(report);
    setEventID(report.eventID ?? "");
    setReportName(report.reportName ?? "");
    setResources(JSON.stringify(report.resources ?? [], null, 2));
    setFormOpen(true);
  }

  function handleSubmit() {
    const payload: Record<string, unknown> = {
      programID,
      eventID,
      clientName: venName,
      resources: JSON.parse(resources),
    };
    if (reportName.trim()) payload.reportName = reportName.trim();

    if (isEditing) {
      updateMut.mutate(
        { id: editingReport.id, payload },
        { onSuccess: () => { setFormOpen(false); setEditingReport(null); } },
      );
    } else {
      submitMut.mutate(payload, { onSuccess: () => setFormOpen(false) });
    }
  }

  return (
    <Stack spacing={2}>
      <Stack direction="row" alignItems="center" justifyContent="space-between">
        <div>
          <Typography variant="h5" data-testid="reports-heading">
            Reports
          </Typography>
          <Typography variant="body2" color="text.secondary" data-testid="reports-last-updated">
            Last updated: {lastUpdated}
          </Typography>
        </div>
        <Button
          variant="contained"
          startIcon={<AddIcon />}
          onClick={handleOpenForm}
          data-testid="submit-report-btn"
        >
          Submit Report
        </Button>
      </Stack>

      <Paper sx={{ p: 2 }}>
        <TextField
          label="Search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          size="small"
          fullWidth
          inputProps={{
            "data-testid": "reports-search",
            "aria-label": "Search reports",
          }}
        />
      </Paper>

      <TableContainer component={Paper}>
        <Table size="small" data-testid="reports-table">
          <TableHead>
            <TableRow>
              <TableCell>Client Name</TableCell>
              <TableCell>Report Name</TableCell>
              <TableCell>Program</TableCell>
              <TableCell>Event</TableCell>
              <TableCell>Created</TableCell>
              <TableCell>Actions</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {filtered.map((r) => (
              <TableRow
                key={r.id}
                hover
                sx={{ cursor: "pointer" }}
                onClick={() => setSelected(r)}
                data-testid={`report-row-${r.id}`}
              >
                <TableCell>{r.clientName ?? "—"}</TableCell>
                <TableCell>{r.reportName ?? "—"}</TableCell>
                <TableCell>
                  {r.programID ? (programMap.get(r.programID) ?? r.programID) : "—"}
                </TableCell>
                <TableCell sx={{ fontFamily: "monospace" }}>{r.eventID ?? "—"}</TableCell>
                <TableCell>{r.createdDateTime ?? "—"}</TableCell>
                <TableCell>
                  <Tooltip title="Edit report">
                    <IconButton
                      size="small"
                      onClick={(e) => { e.stopPropagation(); handleEdit(r); }}
                      data-testid={`report-edit-${r.id}`}
                    >
                      <EditIcon fontSize="small" />
                    </IconButton>
                  </Tooltip>
                </TableCell>
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} align="center" data-testid="reports-empty">
                  No reports
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      {formOpen && (
        <Paper sx={{ p: 2 }} data-testid="report-form">
          <Typography variant="h6" sx={{ mb: 1 }}>{isEditing ? "Edit Report" : "Submit Report"}</Typography>
          <Stack spacing={1.5}>
            <TextField
              select
              label="Event"
              fullWidth
              value={eventID}
              onChange={(e) => setEventID(e.target.value)}
              inputProps={{ "data-testid": "report-event-select" }}
            >
              {events.map((e) => (
                <MenuItem key={e.id} value={e.id}>
                  {e.eventName ?? e.id}
                </MenuItem>
              ))}
            </TextField>
            <TextField
              label="Program (auto)"
              fullWidth
              value={programID ? (programMap.get(programID) ?? programID) : "—"}
              disabled
              inputProps={{ "data-testid": "report-program-display" }}
            />
            <TextField
              label="Client Name"
              fullWidth
              value={venName}
              disabled
              inputProps={{ "data-testid": "report-client-name" }}
            />
            <TextField
              label="Report Name (optional)"
              fullWidth
              value={reportName}
              onChange={(e) => setReportName(e.target.value)}
              inputProps={{ "data-testid": "report-name-input" }}
            />
            <TextField
              label="Resources (JSON)"
              fullWidth
              multiline
              minRows={3}
              value={resources}
              onChange={(e) => setResources(e.target.value)}
              error={!resourcesValid}
              helperText={resourcesValid ? "" : "Invalid JSON"}
              inputProps={{ "data-testid": "report-resources-input" }}
            />
            <Stack direction="row" spacing={1}>
              <Button onClick={() => setFormOpen(false)}>Cancel</Button>
              <Button
                variant="outlined"
                startIcon={<AutoFixHighIcon />}
                disabled={!selectedEvent}
                onClick={() => {
                  if (!selectedEvent) return;
                  setResources(buildExampleResources(selectedEvent, venName));
                  if (!reportName.trim()) {
                    const ts = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
                    setReportName(`report-${selectedEvent.eventName ?? selectedEvent.id}-${ts}`);
                  }
                }}
                data-testid="report-suggest-btn"
              >
                Suggest Example
              </Button>
              <Button
                variant="contained"
                onClick={handleSubmit}
                disabled={!eventID || !resourcesValid || submitMut.isPending || updateMut.isPending}
                data-testid="report-submit-btn"
              >
                {(submitMut.isPending || updateMut.isPending)
                  ? (isEditing ? "Updating..." : "Submitting...")
                  : (isEditing ? "Update" : "Submit")}
              </Button>
            </Stack>
          </Stack>
        </Paper>
      )}

      <JsonDialog
        open={!!selected}
        title={selected ? `Report: ${selected.reportName ?? selected.id}` : "Report"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
