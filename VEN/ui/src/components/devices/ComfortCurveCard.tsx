import type { AssetKind } from "../../api/generated/AssetKind";
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
  useComfortCurveCost,
  useDeleteComfortCurve,
  useSetComfortCurve,
} from "../../api/hooks";
import type { ComfortRate, SimSnapshot } from "../../api/types";
import { assetsInRosterOrder } from "../../api/assets";
import { CurveChart } from "../charts/CurveChart";

/** The asset types a comfort curve prices, with what the curve's x axis means for each —
 *  declared per type rather than branched on at the field (`declare-dont-branch`). For an EV
 *  it is state of charge — `ev-comfort-piecewise-core` prices every kWh from the current SoC to
 *  full, so "50 %" is the battery being half full, not a task being half done. */
const FILL_LABEL_BY_TYPE: Partial<Record<AssetKind, string>> = {
  ev: "State of charge (%)",
  heater: "Fill (%)",
  battery: "Fill (%)",
};

const DEFAULT_FILL_LABEL = "Fill (%)";

/** WP4.2 (BL-19): per-asset comfort-curve editor — a plain table of
 *  (fill %, bid €/kWh) points; POST installs an override, DELETE restores
 *  the built-in default. */
export function ComfortCurveCard({ sim }: { sim: SimSnapshot | undefined }) {
  // Every asset of the roster whose type a curve prices, in roster order (W6).
  const curveAssets = assetsInRosterOrder(sim).filter(([, a]) => a.asset_type in FILL_LABEL_BY_TYPE);
  const [chosenId, setAssetId] = useState<string | null>(null);
  const assetId = chosenId ?? curveAssets[0]?.[0] ?? "";
  const { data } = useComfortCurve(assetId);
  const setMut = useSetComfortCurve();
  const deleteMut = useDeleteComfortCurve();

  // The editor shows the fetched curve until the user edits; local edits are
  // kept in `edited` and dropped on asset switch / save / reset, so no effect
  // is needed to sync state with the query result.
  const [edited, setEdited] = useState<ComfortRate[] | null>(null);
  const rows = edited ?? data?.rates ?? [];
  const { data: cost } = useComfortCurveCost(assetId, rows);
  const setRows = (fn: (rs: ComfortRate[]) => ComfortRate[]) => setEdited(fn(rows));

  function updateRow(i: number, patch: Partial<ComfortRate>) {
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }

  const assetType = curveAssets.find(([id]) => id === assetId)?.[1].asset_type;
  const fillLabel = (assetType && FILL_LABEL_BY_TYPE[assetType]) ?? DEFAULT_FILL_LABEL;
  // The API refuses a rising curve and names the offending point; show that
  // rather than letting a failed save look like nothing happened.
  const saveError = setMut.error instanceof Error ? setMut.error.message : null;

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
          {curveAssets.map(([id]) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </TextField>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
          Each point is the most you will pay for the <em>next</em> kWh at that level of
          fill — a marginal bid, not a total. Bids must not rise as fill increases;
          a rising curve is rejected.
        </Typography>
        <CurveChart rows={rows} />
        {cost?.max_cost_eur != null && (
          <Box data-testid="comfort-cost" sx={{ mt: 1 }}>
            <Typography variant="body2">
              Most this curve can spend: €{cost.max_cost_eur.toFixed(2)}
              {cost.energy_kwh != null && ` for ${cost.energy_kwh.toFixed(1)} kWh`}
              {" "}(from the current charge to the limit)
            </Typography>
            <Typography variant="body2">
              Planned (current plan): €{cost.planned_cost_eur.toFixed(2)}
            </Typography>
            {cost.planned_cost_eur > cost.max_cost_eur + 0.005 && (
              <Typography
                variant="body2"
                color="warning.main"
                data-testid="comfort-cost-forced"
              >
                The plan spends more than the curve allows: a deadline forces charging beyond
                what the curve values.
              </Typography>
            )}
          </Box>
        )}
        {rows.length === 0 ? (
          <Typography color="text.secondary">No curve points</Typography>
        ) : (
          <Stack spacing={1} sx={{ mt: 2 }}>
            {rows.map((r, i) => (
              <Box
                key={i}
                data-testid={`comfort-row-${i}`}
                sx={{ display: "flex", gap: 1, alignItems: "center" }}
              >
                <TextField
                  label={fillLabel}
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
                <TextField
                  label="CO2 bid (gCO2/kWh)"
                  type="number"
                  size="small"
                  value={r.max_marginal_co2}
                  onChange={(e) =>
                    updateRow(i, { max_marginal_co2: Number(e.target.value) })
                  }
                  inputProps={{ min: 0, step: 10, "data-testid": `comfort-co2-${i}` }}
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
        {saveError && (
          <Typography
            variant="body2"
            color="error"
            sx={{ mt: 2 }}
            data-testid="comfort-save-error"
          >
            {saveError}
          </Typography>
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
