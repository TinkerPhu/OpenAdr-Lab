import { Box, FormControlLabel, Slider, Switch, TextField, Typography } from "@mui/material";
import type { ControlDescriptor } from "../../api/types";

interface DynamicControlProps {
  descriptor: ControlDescriptor;
  value: number | boolean | null;
  onChange: (key: string, val: number | boolean) => void;
  onCommit: (key: string, val: number | boolean | null) => void;
}

/** Fraction of a nullable slider's range, at the top, that snaps to "Off" on release. */
const OFF_SNAP_FRACTION = 0.05;

/**
 * Renders a single control driven by a ControlDescriptor from GET /sim/schema.
 * data-testid uses hyphen-normalised key: ctrl-{key.replace(/_/g, '-')}
 *
 * onChange fires on every drag event (live local display update).
 * onCommit fires once on mouse-up / touch-end / key-up (triggers the POST).
 * For Switch and NumberInput there is no drag phase — onChange fires onCommit directly.
 */
export function DynamicControl({ descriptor, value, onChange, onCommit }: DynamicControlProps) {
  const { key, label, kind, min, max, unit, display_scale, nullable } = descriptor;
  const testId = `ctrl-${key.replace(/_/g, "-")}`;

  if (kind === "switch") {
    return (
      <FormControlLabel
        control={
          <Switch
            size="small"
            checked={typeof value === "boolean" ? value : Boolean(value)}
            onChange={(e) => {
              onChange(key, e.target.checked);
              onCommit(key, e.target.checked);
            }}
            data-testid={testId}
          />
        }
        label={<Typography variant="caption">{label}</Typography>}
      />
    );
  }

  if (kind === "slider") {
    const scale = display_scale ?? 1;
    const minRaw = min ?? 0;
    const maxRaw = max ?? 1;
    // A nullable slider with no active override (value === null) reads as
    // "unrestricted" and is pinned to the top of the range, not min ?? 0 —
    // otherwise "no override" and "override = 0" render identically.
    const numVal = typeof value === "number" ? value : (nullable ? maxRaw : minRaw);
    const offSnapZone = nullable ? (maxRaw - minRaw) * OFF_SNAP_FRACTION : 0;
    const isOff = nullable && (value === null || numVal >= maxRaw - offSnapZone);
    const displayVal = (isOff ? maxRaw : numVal) * scale;
    const displayMin = minRaw * scale;
    const displayMax = maxRaw * scale;
    const step = scale > 1 ? 1 : (max != null && min != null ? (max - min) / 100 : 1);
    const labelFmt = (v: number) => scale > 1 ? v.toFixed(0) : v.toFixed(2);
    const tooltipFmt = (v: number) => v.toFixed(2);
    return (
      <Box>
        <Typography variant="caption">
          {label}: {isOff ? "Off" : (unit ? `${labelFmt(displayVal)} ${unit}` : labelFmt(displayVal))}
        </Typography>
        <Slider
          size="small"
          min={displayMin}
          max={displayMax}
          step={step}
          value={displayVal}
          data-testid={testId}
          onChange={(_e, v) => onChange(key, (v as number) / scale)}
          onChangeCommitted={(_e, v) => {
            const raw = (v as number) / scale;
            onCommit(key, nullable && raw >= maxRaw - offSnapZone ? null : raw);
          }}
          valueLabelDisplay="auto"
          valueLabelFormat={(v) => {
            const raw = v / scale;
            if (nullable && raw >= maxRaw - offSnapZone) return "Off";
            return unit ? `${tooltipFmt(v)} ${unit}` : tooltipFmt(v);
          }}
        />
      </Box>
    );
  }

  // NumberInput — no drag phase; commit on every change (same as before)
  const numVal = typeof value === "number" ? value : (min ?? 0);
  return (
    <Box>
      <Typography variant="caption">{label}{unit ? ` [${unit}]` : ""}</Typography>
      <TextField
        size="small"
        type="number"
        value={numVal}
        inputProps={{ step: 0.5, "data-testid": testId }}
        onChange={(e) => {
          const v = parseFloat(e.target.value) || 0;
          onChange(key, v);
          onCommit(key, v);
        }}
        fullWidth
      />
    </Box>
  );
}
