import { Box, FormControlLabel, Slider, Switch, TextField, Typography } from "@mui/material";
import type { ControlDescriptor } from "../../api/types";

interface DynamicControlProps {
  descriptor: ControlDescriptor;
  value: number | boolean | null;
  onChange: (key: string, val: number | boolean) => void;
}

/**
 * Renders a single control driven by a ControlDescriptor from GET /sim/schema.
 * data-testid uses hyphen-normalised key: ctrl-{key.replace(/_/g, '-')}
 */
export function DynamicControl({ descriptor, value, onChange }: DynamicControlProps) {
  const { key, label, kind, min, max, unit } = descriptor;
  const testId = `ctrl-${key.replace(/_/g, "-")}`;

  if (kind === "switch") {
    return (
      <FormControlLabel
        control={
          <Switch
            size="small"
            checked={typeof value === "boolean" ? value : Boolean(value)}
            onChange={(e) => onChange(key, e.target.checked)}
            data-testid={testId}
          />
        }
        label={<Typography variant="caption">{label}</Typography>}
      />
    );
  }

  if (kind === "slider") {
    const numVal = typeof value === "number" ? value : (min ?? 0);
    return (
      <Box>
        <Typography variant="caption">
          {label}: {unit ? `${numVal.toFixed(2)} ${unit}` : numVal.toFixed(2)}
        </Typography>
        <Slider
          size="small"
          min={min ?? 0}
          max={max ?? 100}
          step={(max != null && min != null) ? (max - min) / 100 : 1}
          value={numVal}
          data-testid={testId}
          onChange={(_e, v) => onChange(key, v as number)}
          valueLabelDisplay="auto"
        />
      </Box>
    );
  }

  // NumberInput
  const numVal = typeof value === "number" ? value : (min ?? 0);
  return (
    <Box>
      <Typography variant="caption">{label}{unit ? ` [${unit}]` : ""}</Typography>
      <TextField
        size="small"
        type="number"
        value={numVal}
        inputProps={{ step: 0.5, "data-testid": testId }}
        onChange={(e) => onChange(key, parseFloat(e.target.value) || 0)}
        fullWidth
      />
    </Box>
  );
}
