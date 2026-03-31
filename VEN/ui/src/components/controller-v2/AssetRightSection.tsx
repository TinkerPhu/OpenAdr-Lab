import { useRef, useState } from "react";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Slider,
  Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import type { AssetId } from "./types";
import type { SimSnapshot, SimInjectState } from "../../api/types";
import { useSimSchema } from "../../api/hooks";
import { DynamicControl } from "./DynamicControl";

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

  const socRaw = sim?.assets?.[assetId]?.["soc"];
  const liveSocPct = socRaw !== undefined ? socRaw * 100 : null;

  // Debounced SoC editing: while user is dragging, hold a local pending value
  // and suppress the live update. 500ms after the last drag event, POST reset.
  const [pendingSocPct, setPendingSocPct] = useState<number | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Local state for schema-driven slider controls: updated immediately on drag
  // so the slider is responsive without waiting for the POST roundtrip.
  // pv_irradiance_alpha (blend-back speed) also benefits: once set locally it
  // never reverts to the server value, which is correct since it's a config
  // knob rather than a live measurement.
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
      // pv_irradiance tracks the live sim value when the user is not actively
      // dragging. Release the local hold after posting so the slider follows
      // sim.assets.pv.irradiance once the server applies the override.
      // pv_irradiance_alpha is a config knob — it retains local state.
      if (key === "pv_irradiance") {
        setLocalControlValues(prev => {
          const { pv_irradiance: _, ...rest } = prev;
          return rest;
        });
      }
    }, 300);
  }

  function getValue(key: string): number | boolean | null {
    if (key in localControlValues) return localControlValues[key];
    // pv_irradiance is a one-shot: the backend applies it and auto-clears it
    // within one sim tick. simInject may cache the pre-clear value briefly,
    // so we must NOT read overrides for this key — go straight to live sim.
    if (key === "pv_irradiance") {
      const irr = sim?.assets?.["pv"]?.["irradiance"];
      return typeof irr === "number" ? irr : null;
    }
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
      sx={{ minWidth: 200, maxWidth: 300 }}
    >
      {/* Status Settings */}
      <Accordion data-testid={`status-settings-accordion-${assetId}`}>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Typography variant="caption" fontWeight="bold">
            Status Settings
          </Typography>
        </AccordionSummary>
        <AccordionDetails sx={{ pt: 0, pb: 1 }}>
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
        </AccordionDetails>
      </Accordion>
    </Box>
  );
}
