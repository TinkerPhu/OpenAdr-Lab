import { useMemo, useState } from "react";
import {
  Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import type { VtnEvent } from "../api/types";
import { useEvents } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";

export function EventsPage() {
  const { data: events = [], dataUpdatedAt } = useEvents();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<VtnEvent | null>(null);

  const filtered = useMemo(() => {
    return events.filter((e) => {
      const hay = `${e.id} ${e.eventName ?? ""} ${e.programID ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [events, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="events-heading">
          Events
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="events-last-updated">
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
            "data-testid": "events-search",
            "aria-label": "Search events",
          }}
        />
      </Paper>

      <TableContainer component={Paper}>
        <Table size="small" data-testid="events-table">
          <TableHead>
            <TableRow>
              <TableCell>Event Name</TableCell>
              <TableCell>Program ID</TableCell>
              <TableCell>Created</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {filtered.map((e) => (
              <TableRow
                key={e.id}
                hover
                sx={{ cursor: "pointer" }}
                onClick={() => setSelected(e)}
                data-testid={`event-row-${e.id}`}
              >
                <TableCell>{e.eventName ?? e.id}</TableCell>
                <TableCell sx={{ fontFamily: "monospace" }}>{e.programID ?? "—"}</TableCell>
                <TableCell>{e.createdDateTime ?? "—"}</TableCell>
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={3} align="center" data-testid="events-empty">
                  No events
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      <JsonDialog
        open={!!selected}
        title={selected ? `Event: ${selected.eventName ?? selected.id}` : "Event"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
