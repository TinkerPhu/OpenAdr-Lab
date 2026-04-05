import { useRef, useState } from "react";
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
  const controls = schema[assetId] ?? [];

  const socRaw = sim?.assets?.[assetId]?.["soc"] ?? null;
  const liveSocPct = socRaw !== null && socRaw !== undefined ? socRaw * 100 : null;

  // Debounced SoC editing: while user is dragging, hold a local pending value
  // and suppress the live update. 500ms after the last drag event, POST reset.
  const [pendingSocPct, setPendingSocPct] = useState<number | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Local state for schema-driven slider controls: updated immediately on drag
  // so the slider is responsive without waiting for the POST roundtrip.
  // Alpha (blend-back speed) knobs also benefit: once set locally they never
  // revert to the server value, which is correct since they are config knobs
  // rather than live measurements.
  const [localControlValues, setLocalControlValues] = useState<Record<string, number | boolean>>({});
  const controlTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const socPct = pendingSocPct ?? liveSocPct;

  function handleSocChange(_: Event, value: number | number[]) {
    const pct = value as number;
    setPendingSocPct(pct);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      onResetSoc(assetId, pct / 100, () => setPendingSocPct(null));
    }, 500);
  }

  function handleChange(key: string, val: number | boolean) {
    setLocalControlValues(prev => ({ ...prev, [key]: val }));
    if (controlTimerRef.current) clearTimeout(controlTimerRef.current);
    controlTimerRef.current = setTimeout(() => {
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
    }, 300);
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
    return null;
  }

  return (
    <Box
      data-testid={`right-section-${assetId}`}
      sx={{ minWidth: 200, maxWidth: 300, p: 1.5 }}
    >
      <Typography variant="caption" fontWeight="bold" sx={{ display: "block", mb: 1 }}>
        Status Settings
      </Typography>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        {/* SoC slider — editable, debounced POST /sim/reset/:id on commit */}
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
              onChange={handleSocChange}
              valueLabelDisplay="auto"
              valueLabelFormat={(v) => `${v}%`}
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
