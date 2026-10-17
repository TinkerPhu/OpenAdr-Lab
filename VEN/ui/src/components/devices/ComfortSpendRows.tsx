import { Box, IconButton, Stack, TextField, Typography } from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import type { ComfortSpendPoint } from "../../api/types";

/** The comfort curve as the total you will pay to reach a level, counted from the asset's
 *  current charge. The VEN turns these points into the marginal bids it plans on, so this is
 *  an input and a read-back, never a second curve. */
export function ComfortSpendRows({
  points,
  fromFill,
  onChange,
}: {
  points: ComfortSpendPoint[];
  fromFill: number | null;
  onChange: (points: ComfortSpendPoint[]) => void;
}) {
  const update = (i: number, patch: Partial<ComfortSpendPoint>) =>
    onChange(points.map((p, j) => (j === i ? { ...p, ...patch } : p)));
  return (
    <Stack spacing={1} sx={{ mt: 2 }}>
      <Typography variant="body2" color="text.secondary" data-testid="comfort-spend-basis">
        Total you will pay to reach each level, counted from{" "}
        {fromFill == null ? "the current charge" : `${Math.round(fromFill * 100)} % (current charge)`}.
        Each step may cost no more per kWh than the one before.
      </Typography>
      {points.length === 0 && <Typography color="text.secondary">No curve points</Typography>}
      {points.map((p, i) => (
        <Box
          key={i}
          data-testid={`comfort-spend-row-${i}`}
          sx={{ display: "flex", gap: 1, alignItems: "center" }}
        >
          <TextField
            label="Reach (%)"
            type="number"
            size="small"
            value={Math.round(p.fill * 100)}
            onChange={(e) => update(i, { fill: Number(e.target.value) / 100 })}
            inputProps={{ min: 0, max: 100, step: 5, "data-testid": `comfort-spend-fill-${i}` }}
          />
          <TextField
            label="Total (€)"
            type="number"
            size="small"
            value={p.cumulative_max_cost_eur}
            onChange={(e) => update(i, { cumulative_max_cost_eur: Number(e.target.value) })}
            inputProps={{ min: 0, step: 0.25, "data-testid": `comfort-spend-eur-${i}` }}
          />
          <TextField
            label="CO2 bid (gCO2/kWh)"
            type="number"
            size="small"
            value={p.max_marginal_co2}
            onChange={(e) => update(i, { max_marginal_co2: Number(e.target.value) })}
            inputProps={{ min: 0, step: 10, "data-testid": `comfort-spend-co2-${i}` }}
          />
          <IconButton
            size="small"
            aria-label="Remove point"
            data-testid={`comfort-spend-remove-${i}`}
            onClick={() => onChange(points.filter((_, j) => j !== i))}
          >
            <CloseIcon fontSize="small" />
          </IconButton>
        </Box>
      ))}
    </Stack>
  );
}
