import { useMemo, useState } from "react";
import {
  Button, MenuItem, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import type { Report } from "../api/types";
import { useReports, useSubmitReport, useEvents, usePrograms } from "../api/hooks";
import { useVenContext } from "../App";
import { JsonDialog } from "../components/JsonDialog";

export function ReportsPage() {
  const { data: reports = [], dataUpdatedAt } = useReports();
  const { data: events = [] } = useEvents();
  const { data: programs = [] } = usePrograms();
  const submitMut = useSubmitReport();
  const { venName } = useVenContext();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Report | null>(null);
  const [formOpen, setFormOpen] = useState(false);

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

  function handleOpenForm() {
    setEventID(events[0]?.id ?? "");
    setReportName("");
    setResources("[]");
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
    submitMut.mutate(payload, { onSuccess: () => setFormOpen(false) });
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
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={5} align="center" data-testid="reports-empty">
                  No reports
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      {formOpen && (
        <Paper sx={{ p: 2 }} data-testid="report-form">
          <Typography variant="h6" sx={{ mb: 1 }}>Submit Report</Typography>
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
                variant="contained"
                onClick={handleSubmit}
                disabled={!eventID || !resourcesValid || submitMut.isPending}
                data-testid="report-submit-btn"
              >
                {submitMut.isPending ? "Submitting..." : "Submit"}
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
