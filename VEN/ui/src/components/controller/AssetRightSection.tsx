import { useState } from "react";
import {
  Box,
  Slider,
  Typography,
} from "@mui/material";
import type { AssetId } from "./types";
import type { SimSnapshot, SimInjectState } from "../../api/types";
import { useSimSchema } from "../../api/hooks";
import { DynamicControl } from "./DynamicControl";

/**
 * Controls that follow Behaviour B (one-shot inject + EMA decay):
 * after posting the override the UI releases its local hold so the slider
 * tracks the live sim value as it blends back to the natural baseline.
 *
 * Maps inject key → function that reads the live value from a SimSnapshot.
 */
const DECAY_CONTROLS: Record<string, (sim: SimSnapshot | undefined) => number | null> = {
  pv_irradiance: (sim) => {
    const v = sim?.assets?.["pv"]?.["irradiance"];
    return typeof v === "number" ? v : null;
  },
  base_load_kw: (sim) => {
    const v = sim?.assets?.["base_load"]?.["baseline_kw"];
    return typeof v === "number" ? v : null;
  },
  // One-shot temperature reset: backend clears it after one tick so the slider
  // must track the live sim temperature after posting (Behaviour B).
  heater_temp_c: (sim) => {
    const v = sim?.assets?.["heater"]?.["temp_c"];
    return typeof v === "number" ? v : null;
  },
};

interface AssetRightSectionProps {
  assetId: AssetId;
  simSnapshot: SimSnapshot | undefined;
  overrides: SimInjectState | undefined;
  onOverrideChange: (patch: Partial<SimInjectState>) => void;
  onResetSoc: (assetId: string, soc: number, onDone: () => void) => void;
}

export function AssetRightSection({
  assetId,
  simSnapshot: sim,
  overrides,
  onOverrideChange,
  onResetSoc,
}: AssetRightSectionProps) {
  const { data: schema = {} } = useSimSchema();
  // heater_temp_c is rendered by the dedicated T_tank hardcoded slider below;
  // exclude it from schema-driven DynamicControl to avoid a duplicate.
  const controls = (schema[assetId] ?? []).filter(
    (d) => !(assetId === "heater" && d.key === "heater_temp_c")
  );

  const socRaw = sim?.assets?.[assetId]?.["soc"] ?? null;
  const liveSocPct = socRaw !== null && socRaw !== undefined ? socRaw * 100 : null;

  // SoC drag: hold a local pending value while the user is dragging so the
  // slider is responsive. Cleared after POST /sim/reset/:id succeeds.
  const [pendingSocPct, setPendingSocPct] = useState<number | null>(null);

  // Local state for schema-driven slider controls: updated immediately on drag
  // so the slider is responsive without waiting for the POST roundtrip.
  // Alpha (blend-back speed) knobs also benefit: once set locally they never
  // revert to the server value, which is correct since they are config knobs
  // rather than live measurements.
  const [localControlValues, setLocalControlValues] = useState<Record<string, number | boolean>>({});

  const socPct = pendingSocPct ?? liveSocPct;

  function handleSocDrag(_: Event, value: number | number[]) {
    setPendingSocPct(value as number);
  }

  function handleSocCommit(_: Event | React.SyntheticEvent, value: number | number[]) {
    const pct = value as number;
    setPendingSocPct(pct);
    onResetSoc(assetId, pct / 100, () => setPendingSocPct(null));
  }

  // Live drag: update local display only — no POST yet.
  function handleChange(key: string, val: number | boolean) {
    setLocalControlValues(prev => ({ ...prev, [key]: val }));
  }

  // Mouse-up commit: POST the value; release local hold for Behaviour B controls.
  function handleCommit(key: string, val: number | boolean) {
    onOverrideChange({ [key]: val } as Partial<SimInjectState>);
    // Decay controls (Behaviour B): release the local hold after posting so
    // the slider follows the live sim value as the backend EMA-blends back.
    // Alpha knobs are config, not measurements — they retain local state.
    if (key in DECAY_CONTROLS) {
      setLocalControlValues(prev => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
  }

  function getValue(key: string): number | boolean | null {
    if (key in localControlValues) return localControlValues[key];
    // Decay controls (Behaviour B): the backend clears the inject field within
    // one tick after applying the offset. Read from live sim so the slider
    // follows the decaying value rather than the stale inject cache.
    if (key in DECAY_CONTROLS) return DECAY_CONTROLS[key](sim);
    if (overrides != null) {
      const v = (overrides as Record<string, unknown>)[key];
      if (typeof v === "number" || typeof v === "boolean") return v;
    }
    // Fall back to the sim's actual current value so controls reflect reality
    // when no override is active (null override means "use sim default").
    if (key === "ev_plugged") {
      const p = sim?.assets?.["ev"]?.["plugged"];
      return p !== undefined ? p !== 0 : null;
    }
    if (key === "ev_soc_target") {
      const t = sim?.assets?.["ev"]?.["soc_target"];
      return typeof t === "number" ? t : null;
    }
    // Persistent heater comfort-band overrides: fall back to live sim values.
    if (key === "heater_temp_min_c") {
      const v = sim?.assets?.["heater"]?.["temp_min_c"];
      return typeof v === "number" ? v : null;
    }
    if (key === "heater_temp_max_c") {
      const v = sim?.assets?.["heater"]?.["temp_max_c"];
      return typeof v === "number" ? v : null;
    }
    return null;
  }

  // T_tank: live tank temperature for heater assets. Uses the existing
  // DECAY_CONTROLS["heater_temp_c"] path via getValue so the slider tracks
  // the live sim value at rest and the drag value while moving.
  const tankC = assetId === "heater" ? getValue("heater_temp_c") : null;
  const hasTank = typeof tankC === "number";

  return (
    <Box
      data-testid={`right-section-${assetId}`}
      sx={{ minWidth: 200, maxWidth: 300, p: 1.5 }}
    >
      <Typography variant="caption" fontWeight="bold" sx={{ display: "block", mb: 1 }}>
        Status Settings
      </Typography>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        {/* SoC slider — POST /sim/reset/:id fires on mouse-up */}
        {socPct !== null && (
          <Box>
            <Typography variant="caption">
              SoC: {socPct.toFixed(0)}%{pendingSocPct !== null ? " (setting…)" : ""}
            </Typography>
            <Slider
              size="small"
              min={0}
              max={100}
              step={1}
              value={socPct}
              data-testid={`ctrl-${assetId}-soc`}
              onChange={handleSocDrag}
              onChangeCommitted={handleSocCommit}
              valueLabelDisplay="auto"
              valueLabelFormat={(v) => `${v}%`}
            />
          </Box>
        )}

        {/* T_tank lever — heater assets only. Tooltip shows full precision for
            observing thermal dynamics. One-shot inject (Behaviour A): slider
            follows live sim temp_c after commit via DECAY_CONTROLS. */}
        {hasTank && (
          <Box>
            <Typography variant="caption">
              T_tank: {(tankC as number).toFixed(2)} °C
            </Typography>
            <Slider
              size="small"
              min={18}
              max={95}
              step={0.1}
              value={tankC as number}
              data-testid={`ctrl-${assetId}-t-tank`}
              onChange={(_e, v) => handleChange("heater_temp_c", v as number)}
              onChangeCommitted={(_e, v) => handleCommit("heater_temp_c", v as number)}
              valueLabelDisplay="auto"
              valueLabelFormat={(v) => `${v.toFixed(6)} °C`}
            />
          </Box>
        )}

        {/* Schema-driven editable controls */}
        {controls.map((d) => (
          <DynamicControl
            key={d.key}
            descriptor={d}
            value={getValue(d.key)}
            onChange={handleChange}
            onCommit={handleCommit}
          />
        ))}

        {controls.length === 0 && socPct === null && (
          <Typography variant="caption" color="text.secondary">
            No controls available
          </Typography>
        )}
      </Box>
    </Box>
  );
}
