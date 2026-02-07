import { useMemo, useState } from "react";
import {
  Button, Chip, IconButton, List, ListItem, ListItemButton, ListItemText,
  Paper, Stack, TextField, Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import DeleteIcon from "@mui/icons-material/Delete";
import AddIcon from "@mui/icons-material/Add";
import type { Program, ProgramInput } from "../api/types";
import { usePrograms, useVens, useCreateProgram, useUpdateProgram, useDeleteProgram } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";
import { ProgramFormDialog } from "../components/ProgramFormDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";

function enrollmentLabel(program: Program): string {
  const venNames = (program.targets ?? [])
    .filter((t) => t.type === "VEN_NAME")
    .flatMap((t) => t.values);
  if (venNames.length === 0) return "Open — all VENs";
  return venNames.join(", ");
}

export function ProgramsPage() {
  const { data: programs = [], dataUpdatedAt } = usePrograms();
  const { data: vens = [] } = useVens();
  const createMut = useCreateProgram();
  const updateMut = useUpdateProgram();
  const deleteMut = useDeleteProgram();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Program | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Program | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Program | null>(null);

  const filtered = useMemo(() => {
    return programs.filter((p) => {
      const hay = `${p.id} ${p.programName ?? ""} ${p.programLongName ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [programs, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  function handleCreate() {
    setEditTarget(null);
    setFormOpen(true);
  }

  function handleEdit(p: Program) {
    setEditTarget(p);
    setFormOpen(true);
  }

  function handleFormSubmit(input: ProgramInput) {
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
          <Typography variant="h5" data-testid="programs-heading">
            Programs
          </Typography>
          <Typography variant="body2" color="text.secondary" data-testid="programs-last-updated">
            Last updated: {lastUpdated}
          </Typography>
        </div>
        <Button
          variant="contained"
          startIcon={<AddIcon />}
          onClick={handleCreate}
          data-testid="create-program-btn"
        >
          Create
        </Button>
      </Stack>

      <Paper sx={{ p: 2 }}>
        <TextField
          label="Search"
          size="small"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          fullWidth
          inputProps={{
            "data-testid": "programs-search",
            "aria-label": "Search programs",
          }}
        />
      </Paper>

      <Paper>
        <List dense data-testid="programs-list">
          {filtered.map((p) => (
            <ListItem
              key={p.id}
              disablePadding
              data-testid={`program-item-${p.id}`}
              secondaryAction={
                <Stack direction="row" spacing={0.5}>
                  <IconButton
                    size="small"
                    onClick={(e) => { e.stopPropagation(); handleEdit(p); }}
                    data-testid={`edit-program-${p.id}`}
                    aria-label={`Edit ${p.programName}`}
                  >
                    <EditIcon fontSize="small" />
                  </IconButton>
                  <IconButton
                    size="small"
                    onClick={(e) => { e.stopPropagation(); setDeleteTarget(p); }}
                    data-testid={`delete-program-${p.id}`}
                    aria-label={`Delete ${p.programName}`}
                  >
                    <DeleteIcon fontSize="small" />
                  </IconButton>
                </Stack>
              }
            >
              <ListItemButton onClick={() => setSelected(p)}>
                <ListItemText
                  primary={p.programName ?? p.id}
                  secondary={
                    <>
                      {p.programLongName && <span>{p.programLongName} — </span>}
                      <Chip
                        label={enrollmentLabel(p)}
                        size="small"
                        variant="outlined"
                        sx={{ height: 18, fontSize: "0.75rem" }}
                        data-testid={`enrollment-${p.id}`}
                      />
                    </>
                  }
                />
              </ListItemButton>
            </ListItem>
          ))}
          {filtered.length === 0 && (
            <ListItem data-testid="programs-empty">
              <ListItemText primary="No programs" />
            </ListItem>
          )}
        </List>
      </Paper>

      <JsonDialog
        open={!!selected}
        title={selected ? `Program: ${selected.programName ?? selected.id}` : "Program"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />

      <ProgramFormDialog
        open={formOpen}
        program={editTarget}
        vens={vens}
        onSubmit={handleFormSubmit}
        onCancel={() => setFormOpen(false)}
        loading={createMut.isPending || updateMut.isPending}
      />

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Program"
        message={`Delete "${deleteTarget?.programName ?? deleteTarget?.id}"? This cannot be undone.`}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteTarget(null)}
        loading={deleteMut.isPending}
      />
    </Stack>
  );
}
