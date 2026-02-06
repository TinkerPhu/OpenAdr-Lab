import { useMemo, useState } from "react";
import {
  Box, Chip, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import type { Event } from "../api/types";
import { useEvents } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";

export function EventsPage() {
  const { data: events = [], dataUpdatedAt } = useEvents();
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Event | null>(null);

  const statuses = useMemo(() => {
    const s = new Set<string>();
    events.forEach((e) => e.status && s.add(e.status));
    return Array.from(s).sort();
  }, [events]);

  const filtered = useMemo(() => {
    return events.filter((e) => {
      if (statusFilter && e.status !== statusFilter) return false;
      const hay = `${e.id} ${e.program_id ?? ""} ${e.status ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [events, statusFilter, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <Box>
        <Typography
          variant="h5"
          data-testid="events-heading"
        >
          Events
        </Typography>
        <Typography
          variant="body2"
          color="text.secondary"
          data-testid="events-last-updated"
        >
          Last updated: {lastUpdated}
        </Typography>
      </Box>

      <Paper sx={{ p: 2 }}>
        <Stack direction={{ xs: "column", sm: "row" }} spacing={2} alignItems="center">
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
          <Stack direction="row" spacing={1} flexWrap="wrap">
            <Chip
              label="All"
              clickable
              color={!statusFilter ? "primary" : "default"}
              onClick={() => setStatusFilter(null)}
              data-testid="events-filter-all"
              aria-pressed={!statusFilter}
            />
            {statuses.map((s) => (
              <Chip
                key={s}
                label={s}
                clickable
                color={statusFilter === s ? "primary" : "default"}
                onClick={() => setStatusFilter(s)}
                data-testid={`events-filter-${s}`}
                aria-pressed={statusFilter === s}
              />
            ))}
          </Stack>
        </Stack>
      </Paper>

      <TableContainer component={Paper}>
        <Table size="small" data-testid="events-table">
          <TableHead>
            <TableRow>
              <TableCell>ID</TableCell>
              <TableCell>Program</TableCell>
              <TableCell>Status</TableCell>
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
                <TableCell sx={{ fontFamily: "monospace" }}>{e.id}</TableCell>
                <TableCell>{e.program_id ?? "—"}</TableCell>
                <TableCell>{e.status ?? "—"}</TableCell>
                <TableCell>{e.created_at ?? "—"}</TableCell>
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={4} align="center" data-testid="events-empty">
                  No events
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      <JsonDialog
        open={!!selected}
        title={selected ? `Event ${selected.id}` : "Event"}
        json={selected?.raw ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
