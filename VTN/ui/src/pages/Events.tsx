import { useMemo, useState } from "react";
import {
  Button, IconButton, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import DeleteIcon from "@mui/icons-material/Delete";
import AddIcon from "@mui/icons-material/Add";
import type { EventInput, VtnEvent } from "../api/types";
import { useEvents, usePrograms, useCreateEvent, useUpdateEvent, useDeleteEvent } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";
import { EventFormDialog } from "../components/EventFormDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";

export function EventsPage() {
  const { data: events = [], dataUpdatedAt } = useEvents();
  const { data: programs = [] } = usePrograms();
  const createMut = useCreateEvent();
  const updateMut = useUpdateEvent();
  const deleteMut = useDeleteEvent();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<VtnEvent | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<VtnEvent | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<VtnEvent | null>(null);

  const programMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of programs) {
      map.set(p.id, p.programName ?? p.id);
    }
    return map;
  }, [programs]);

  const filtered = useMemo(() => {
    return events.filter((e) => {
      const progName = e.programID ? programMap.get(e.programID) ?? "" : "";
      const hay = `${e.id} ${e.eventName ?? ""} ${e.programID ?? ""} ${progName}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [events, query, programMap]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  function handleCreate() {
    setEditTarget(null);
    setFormOpen(true);
  }

  function handleEdit(e: VtnEvent) {
    setEditTarget(e);
    setFormOpen(true);
  }

  function handleFormSubmit(input: EventInput) {
    if (editTarget) {
      updateMut.mutate({ id: editTarget.id, input }, { onSuccess: () => setFormOpen(false) });
    } else {
      createMut.mutate(input, { onSuccess: () => setFormOpen(false) });
    }
  }

  function handleDeleteConfirm() {
    if (deleteTarget) {
      deleteMut.mutate(deleteTarget.id, { onSuccess: () => setDeleteTarget(null) });
    }
  }

  return (
    <Stack spacing={2}>
      <Stack direction="row" alignItems="center" justifyContent="space-between">
        <div>
          <Typography variant="h5" data-testid="events-heading">
            Events
          </Typography>
          <Typography variant="body2" color="text.secondary" data-testid="events-last-updated">
            Last updated: {lastUpdated}
          </Typography>
        </div>
        <Button
          variant="contained"
          startIcon={<AddIcon />}
          onClick={handleCreate}
          data-testid="create-event-btn"
        >
          Create
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
              <TableCell>Program</TableCell>
              <TableCell>Created</TableCell>
              <TableCell align="right">Actions</TableCell>
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
                <TableCell>
                  {e.programID ? (programMap.get(e.programID) ?? e.programID) : "—"}
                </TableCell>
                <TableCell>{e.createdDateTime ?? "—"}</TableCell>
                <TableCell align="right">
                  <IconButton
                    size="small"
                    onClick={(ev) => { ev.stopPropagation(); handleEdit(e); }}
                    data-testid={`edit-event-${e.id}`}
                    aria-label={`Edit ${e.eventName}`}
                  >
                    <EditIcon fontSize="small" />
                  </IconButton>
                  <IconButton
                    size="small"
                    onClick={(ev) => { ev.stopPropagation(); setDeleteTarget(e); }}
                    data-testid={`delete-event-${e.id}`}
                    aria-label={`Delete ${e.eventName}`}
                  >
                    <DeleteIcon fontSize="small" />
                  </IconButton>
                </TableCell>
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
        title={selected ? `Event: ${selected.eventName ?? selected.id}` : "Event"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />

      <EventFormDialog
        open={formOpen}
        event={editTarget}
        programs={programs}
        onSubmit={handleFormSubmit}
        onCancel={() => setFormOpen(false)}
        loading={createMut.isPending || updateMut.isPending}
      />

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Event"
        message={`Delete "${deleteTarget?.eventName ?? deleteTarget?.id}"? This cannot be undone.`}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteTarget(null)}
        loading={deleteMut.isPending}
      />
    </Stack>
  );
}
