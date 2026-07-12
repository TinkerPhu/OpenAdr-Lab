import { useState } from "react";
import {
  Box,
  Button,
  Card,
  CardActions,
  CardContent,
  CardHeader,
  Chip,
  IconButton,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import {
  useComfortCurve,
  useDeleteComfortCurve,
  useSetComfortCurve,
} from "../../api/hooks";
import type { ComfortRate } from "../../api/types";

const CURVE_ASSETS = ["ev", "heater", "battery"];

/** WP4.2 (BL-19): per-asset comfort-curve editor — a plain table of
 *  (fill %, bid €/kWh) points; POST installs an override, DELETE restores
 *  the built-in default. */
export function ComfortCurveCard() {
  const [assetId, setAssetId] = useState("ev");
  const { data } = useComfortCurve(assetId);
  const setMut = useSetComfortCurve();
  const deleteMut = useDeleteComfortCurve();

  // The editor shows the fetched curve until the user edits; local edits are
  // kept in `edited` and dropped on asset switch / save / reset, so no effect
  // is needed to sync state with the query result.
  const [edited, setEdited] = useState<ComfortRate[] | null>(null);
  const rows = edited ?? data?.rates ?? [];
  const setRows = (fn: (rs: ComfortRate[]) => ComfortRate[]) => setEdited(fn(rows));

  function updateRow(i: number, patch: Partial<ComfortRate>) {
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }

  return (
    <Card data-testid="comfort-curve-card">
      <CardHeader
        title="Comfort Curve"
        action={
          <Chip
            label={data?.source ?? "default"}
            size="small"
            color={data?.source === "override" ? "info" : "default"}
            data-testid="comfort-source-chip"
          />
        }
      />
      <CardContent>
        <TextField
          select
          label="Asset"
          value={assetId}
          onChange={(e) => { setAssetId(e.target.value); setEdited(null); }}
          SelectProps={{ native: true }}
          size="small"
          sx={{ mb: 2 }}
          data-testid="comfort-asset-select"
        >
          {CURVE_ASSETS.map((a) => (
            <option key={a} value={a}>
              {a}
            </option>
          ))}
        </TextField>
        {rows.length === 0 ? (
          <Typography color="text.secondary">No curve points</Typography>
        ) : (
          <Stack spacing={1}>
            {rows.map((r, i) => (
              <Box
                key={i}
                data-testid={`comfort-row-${i}`}
                sx={{ display: "flex", gap: 1, alignItems: "center" }}
              >
                <TextField
                  label="Fill (%)"
                  type="number"
                  size="small"
                  value={Math.round(r.fill * 100)}
                  onChange={(e) => updateRow(i, { fill: Number(e.target.value) / 100 })}
                  inputProps={{ min: 0, max: 100, step: 5, "data-testid": `comfort-fill-${i}` }}
                />
                <TextField
                  label="Bid (€/kWh)"
                  type="number"
                  size="small"
                  value={r.max_marginal_price}
                  onChange={(e) =>
                    updateRow(i, { max_marginal_price: Number(e.target.value) })
                  }
                  inputProps={{ min: 0, step: 0.05, "data-testid": `comfort-bid-${i}` }}
                />
                <IconButton
                  size="small"
                  aria-label="Remove point"
                  data-testid={`comfort-remove-${i}`}
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
          data-testid="comfort-add-btn"
          onClick={() =>
            setRows((rs) => [
              ...rs,
              { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 0 },
            ])
          }
        >
          Add point
        </Button>
        <Button
          size="small"
          variant="contained"
          data-testid="comfort-save-btn"
          disabled={setMut.isPending || rows.length === 0}
          onClick={() => setMut.mutateAsync({ assetId, rates: rows }).then(() => setEdited(null))}
        >
          Save
        </Button>
        <Button
          size="small"
          color="warning"
          data-testid="comfort-reset-btn"
          disabled={deleteMut.isPending || data?.source !== "override"}
          onClick={() => deleteMut.mutateAsync(assetId).then(() => setEdited(null))}
        >
          Reset to default
        </Button>
      </CardActions>
    </Card>
  );
}
