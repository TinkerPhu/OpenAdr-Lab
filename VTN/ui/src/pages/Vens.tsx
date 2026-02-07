import { useMemo, useState } from "react";
import {
  Chip, IconButton, List, ListItem, ListItemButton, ListItemText,
  Paper, Stack, TextField, Typography,
} from "@mui/material";
import DeleteIcon from "@mui/icons-material/Delete";
import type { Program, Ven } from "../api/types";
import { useVens, useDeleteVen, usePrograms } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";

function enrolledProgramNames(ven: Ven, programs: Program[]): string[] {
  const venName = ven.venName ?? ven.id;
  return programs
    .filter((p) => {
      if (!p.targets) return false; // open programs don't count as "enrolled"
      return p.targets.some(
        (t) => t.type === "VEN_NAME" && t.values.includes(venName),
      );
    })
    .map((p) => p.programName ?? p.id);
}

export function VensPage() {
  const { data: vens = [], dataUpdatedAt } = useVens();
  const { data: programs = [] } = usePrograms();
  const deleteMut = useDeleteVen();

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Ven | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Ven | null>(null);

  const filtered = useMemo(() => {
    return vens.filter((v) => {
      const hay = `${v.id} ${v.venName ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [vens, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  function handleDeleteConfirm() {
    if (deleteTarget) {
      deleteMut.mutate(deleteTarget.id, { onSuccess: () => setDeleteTarget(null) });
    }
  }

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="vens-heading">
          VENs
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="vens-last-updated">
          Last updated: {lastUpdated}
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <TextField
          label="Search"
          size="small"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          fullWidth
          inputProps={{
            "data-testid": "vens-search",
            "aria-label": "Search VENs",
          }}
        />
      </Paper>

      <Paper>
        <List dense data-testid="vens-list">
          {filtered.map((v) => {
            const enrolled = enrolledProgramNames(v, programs);
            return (
              <ListItem
                key={v.id}
                disablePadding
                data-testid={`ven-item-${v.id}`}
                secondaryAction={
                  <IconButton
                    size="small"
                    onClick={(e) => { e.stopPropagation(); setDeleteTarget(v); }}
                    data-testid={`delete-ven-${v.id}`}
                    aria-label={`Delete ${v.venName}`}
                  >
                    <DeleteIcon fontSize="small" />
                  </IconButton>
                }
              >
                <ListItemButton onClick={() => setSelected(v)}>
                  <ListItemText
                    primary={v.venName ?? v.id}
                    secondary={
                      <>
                        {v.id}
                        {enrolled.length > 0 && (
                          <span>
                            {" — "}
                            {enrolled.map((name) => (
                              <Chip
                                key={name}
                                label={name}
                                size="small"
                                variant="outlined"
                                sx={{ height: 18, fontSize: "0.7rem", mr: 0.5 }}
                              />
                            ))}
                          </span>
                        )}
                      </>
                    }
                  />
                </ListItemButton>
              </ListItem>
            );
          })}
          {filtered.length === 0 && (
            <ListItem data-testid="vens-empty">
              <ListItemText primary="No VENs" />
            </ListItem>
          )}
        </List>
      </Paper>

      <JsonDialog
        open={!!selected}
        title={selected ? `VEN: ${selected.venName ?? selected.id}` : "VEN"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete VEN"
        message={`Delete "${deleteTarget?.venName ?? deleteTarget?.id}"? This cannot be undone.`}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteTarget(null)}
        loading={deleteMut.isPending}
      />
    </Stack>
  );
}
