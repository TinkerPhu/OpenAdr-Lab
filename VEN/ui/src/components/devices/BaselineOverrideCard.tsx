import { useState } from "react";
import {
  Box,
  Button,
  Card,
  CardActions,
  CardContent,
  CardHeader,
  IconButton,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import {
  useBaselineOverride,
  useDeleteBaselineOverride,
  usePostBaselineOverride,
} from "../../api/hooks";
import type { BaselineSlot } from "../../api/types";

// ── Helpers ──────────────────────────────────────────────────────────────────

/** ISO 8601 (wire format) -> "YYYY-MM-DDTHH:mm" local value for a datetime-local input. */
function isoToLocalInput(iso: string): string {
  const d = new Date(iso);
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

/** datetime-local input value -> ISO 8601 (wire format). */
function localInputToIso(local: string): string {
  return new Date(local).toISOString();
}

function defaultLocalInput(): string {
  return isoToLocalInput(new Date().toISOString());
}

/** WP-BL-42: Devices page control surfacing the `/baseline-override` capability —
 *  view the active per-slot (`slot_start`, `add_kw`) baseline-load override, edit
 *  it locally, save it via POST, or clear it via DELETE. */
export function BaselineOverrideCard() {
  const { data } = useBaselineOverride();
  const postMut = usePostBaselineOverride();
  const deleteMut = useDeleteBaselineOverride();

  // The card shows fetched slots until the user edits; local edits are kept in
  // `edited` and dropped on save/clear, so no effect is needed to sync state
  // with the query result (same pattern as ComfortCurveCard).
  const [edited, setEdited] = useState<BaselineSlot[] | null>(null);
  const rows = edited ?? data?.slots ?? [];
  const setRows = (fn: (rs: BaselineSlot[]) => BaselineSlot[]) => setEdited(fn(rows));

  function updateRow(i: number, patch: Partial<BaselineSlot>) {
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }

  return (
    <Card data-testid="baseline-override-card">
      <CardHeader
        title="Baseline Override"
        subheader={data?.updated_at ? `Updated ${new Date(data.updated_at).toLocaleString()}` : undefined}
      />
      <CardContent>
        {rows.length === 0 ? (
          <Typography color="text.secondary">No baseline override active</Typography>
        ) : (
          <Stack spacing={1}>
            {rows.map((r, i) => (
              <Box
                key={i}
                data-testid={`baseline-row-${i}`}
                sx={{ display: "flex", gap: 1, alignItems: "center" }}
              >
                <TextField
                  label="Slot start"
                  type="datetime-local"
                  size="small"
                  value={isoToLocalInput(r.slot_start)}
                  onChange={(e) => updateRow(i, { slot_start: localInputToIso(e.target.value) })}
                  InputLabelProps={{ shrink: true }}
                  inputProps={{ lang: "de", "data-testid": `baseline-slot-start-${i}` }}
                />
                <TextField
                  label="Add (kW)"
                  type="number"
                  size="small"
                  value={r.add_kw}
                  onChange={(e) => updateRow(i, { add_kw: Number(e.target.value) })}
                  inputProps={{ step: 0.1, "data-testid": `baseline-add-kw-${i}` }}
                />
                <IconButton
                  size="small"
                  aria-label="Remove slot"
                  data-testid={`baseline-remove-${i}`}
                  onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}
                >
                  <CloseIcon fontSize="small" />
                </IconButton>
              </Box>
            ))}
          </Stack>
        )}
      </CardContent>
      <CardActions sx={{ px: 2 }}>
        <Button
          size="small"
          data-testid="baseline-add-btn"
          onClick={() =>
            setRows((rs) => [...rs, { slot_start: localInputToIso(defaultLocalInput()), add_kw: 0 }])
          }
        >
          Add slot
        </Button>
        <Button
          size="small"
          variant="contained"
          data-testid="baseline-save-btn"
          disabled={postMut.isPending || rows.length === 0}
          onClick={() => postMut.mutateAsync({ slots: rows }).then(() => setEdited(null))}
        >
          Save
        </Button>
        <Button
          size="small"
          color="warning"
          data-testid="baseline-clear-btn"
          disabled={deleteMut.isPending || !data}
          onClick={() => deleteMut.mutateAsync().then(() => setEdited(null))}
        >
          Clear
        </Button>
      </CardActions>
    </Card>
  );
}
