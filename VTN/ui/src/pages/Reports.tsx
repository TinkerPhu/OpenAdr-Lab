import { useMemo, useState } from "react";
import {
  IconButton, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import DeleteIcon from "@mui/icons-material/Delete";
import type { Report } from "../api/types";
import { useReports, useDeleteReport } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";

export function ReportsPage() {
  const { data: reports = [], dataUpdatedAt } = useReports();
  const deleteMut = useDeleteReport();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Report | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Report | null>(null);

  const filtered = useMemo(() => {
    return reports.filter((r) => {
      const hay = `${r.id} ${r.clientName ?? ""} ${r.reportName ?? ""} ${r.programID ?? ""} ${r.eventID ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [reports, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  function handleDeleteConfirm() {
    if (deleteTarget) {
      deleteMut.mutate(deleteTarget.id, { onSuccess: () => setDeleteTarget(null) });
    }
  }

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="reports-heading">
          Reports
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="reports-last-updated">
          Last updated: {lastUpdated}
        </Typography>
      </div>

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
              <TableCell>Program ID</TableCell>
              <TableCell>Event ID</TableCell>
              <TableCell>Created</TableCell>
              <TableCell align="right">Actions</TableCell>
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
                <TableCell sx={{ fontFamily: "monospace" }}>{r.programID ?? "—"}</TableCell>
                <TableCell sx={{ fontFamily: "monospace" }}>{r.eventID ?? "—"}</TableCell>
                <TableCell>{r.createdDateTime ?? "—"}</TableCell>
                <TableCell align="right">
                  <IconButton
                    size="small"
                    onClick={(ev) => { ev.stopPropagation(); setDeleteTarget(r); }}
                    data-testid={`delete-report-${r.id}`}
                    aria-label={`Delete report ${r.reportName ?? r.id}`}
                  >
                    <DeleteIcon fontSize="small" />
                  </IconButton>
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

      <JsonDialog
        open={!!selected}
        title={selected ? `Report: ${selected.reportName ?? selected.id}` : "Report"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Report"
        message={`Delete report "${deleteTarget?.reportName ?? deleteTarget?.id}"? This cannot be undone.`}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteTarget(null)}
        loading={deleteMut.isPending}
      />
    </Stack>
  );
}
