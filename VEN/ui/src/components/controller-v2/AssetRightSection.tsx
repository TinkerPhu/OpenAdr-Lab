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
import type { SimSnapshot, UserOverrides } from "../../api/types";
import { useSimSchema } from "../../api/hooks";
import { DynamicControl } from "./DynamicControl";

interface AssetRightSectionProps {
  assetId: AssetId;
  simSnapshot: SimSnapshot | undefined;
  overrides: UserOverrides | undefined;
  onOverrideChange: (patch: Partial<UserOverrides>) => void;
}

export function AssetRightSection({
  assetId,
  simSnapshot: sim,
  overrides,
  onOverrideChange,
}: AssetRightSectionProps) {
  const { data: schema = {} } = useSimSchema();
  const controls = schema[assetId] ?? [];

  const socRaw = sim?.assets?.[assetId]?.["soc"];
  const socPct = socRaw !== undefined ? socRaw * 100 : null;

  function handleChange(key: string, val: number | boolean) {
    onOverrideChange({ [key]: val } as Partial<UserOverrides>);
  }

  function getValue(key: string): number | boolean | null {
    if (overrides != null) {
      const v = (overrides as Record<string, unknown>)[key];
      if (typeof v === "number" || typeof v === "boolean") return v;
    }
    // Fall back to the sim's actual current value so the switch reflects reality
    // when no override is active (null override means "use sim default").
    if (key === "ev_plugged") {
      const p = sim?.assets?.["ev"]?.["plugged"];
      return p !== undefined ? p !== 0 : null;
    }
    return null;
  }

  return (
    <Box
      data-testid={`right-section-${assetId}`}
      sx={{ minWidth: 200, maxWidth: 300 }}
    >
      {/* Status Settings — expanded by default */}
      <Accordion defaultExpanded data-testid={`status-settings-accordion-${assetId}`}>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Typography variant="caption" fontWeight="bold">
            Status Settings
          </Typography>
        </AccordionSummary>
        <AccordionDetails sx={{ pt: 0, pb: 1 }}>
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            {/* Read-only SoC display — rendered generically for any asset that has soc */}
            {socPct !== null && (
              <Box>
                <Typography variant="caption">SoC: {socPct.toFixed(0)}%</Typography>
                <Slider
                  size="small"
                  min={0}
                  max={100}
                  step={1}
                  value={socPct}
                  data-testid={`ctrl-${assetId}-soc`}
                  disabled
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
