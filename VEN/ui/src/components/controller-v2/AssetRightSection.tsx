import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  FormControlLabel,
  Slider,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import type { AssetId } from "./types";
import type { SimSnapshot, UserOverrides } from "../../api/types";

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
          {assetId === "ev" && <EvStatusControls sim={sim} overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "battery" && <BatteryStatusControls sim={sim} overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "heater" && <HeaterStatusControls overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "pv" && <PvStatusControls overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "base_load" && <BaseLoadStatusControls overrides={overrides} onChange={onOverrideChange} />}
        </AccordionDetails>
      </Accordion>

      {/* Simulation Characteristics — collapsed by default */}
      <Accordion data-testid={`sim-characteristics-accordion-${assetId}`}>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Typography variant="caption" fontWeight="bold">
            Simulation Characteristics
          </Typography>
        </AccordionSummary>
        <AccordionDetails sx={{ pt: 0, pb: 1 }}>
          {assetId === "ev" && <EvSimCharacteristics overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "battery" && <BatterySimCharacteristics sim={sim} overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "heater" && <HeaterSimCharacteristics overrides={overrides} onChange={onOverrideChange} />}
          {assetId === "pv" && <PvSimCharacteristics overrides={overrides} onChange={onOverrideChange} />}
        </AccordionDetails>
      </Accordion>
    </Box>
  );
}

// ─── EV ───────────────────────────────────────────────────────────────────────

function EvStatusControls({
  sim,
  overrides,
  onChange,
}: {
  sim?: SimSnapshot;
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  const plugged = overrides?.ev_plugged ?? true;
  const socPct = sim?.ev ? sim.ev.soc * 100 : 50;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <FormControlLabel
        control={
          <Switch
            size="small"
            checked={plugged}
            onChange={(e) => onChange({ ev_plugged: e.target.checked })}
            data-testid="ctrl-ev-plugged"
          />
        }
        label={<Typography variant="caption">Plugged in</Typography>}
      />

      <Typography variant="caption">SoC: {socPct.toFixed(0)}%</Typography>
      <Slider
        size="small"
        min={0}
        max={100}
        step={1}
        value={socPct}
        data-testid="ctrl-ev-soc"
        onChange={(_e, v) => onChange({ ev_initial_soc: (v as number) / 100 })}
        valueLabelDisplay="auto"
        valueLabelFormat={(v) => `${v}%`}
      />
    </Box>
  );
}

function EvSimCharacteristics({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Max charge kW</Typography>
      <Slider
        size="small"
        min={0}
        max={22}
        step={0.5}
        value={overrides?.ev_max_charge_kw ?? 7}
        data-testid="ctrl-ev-max_charge_kw"
        onChange={(_e, v) => onChange({ ev_max_charge_kw: v as number })}
        valueLabelDisplay="auto"
      />
      <Typography variant="caption">SoC target</Typography>
      <Slider
        size="small"
        min={0}
        max={100}
        step={1}
        value={(overrides?.ev_soc_target ?? 0.8) * 100}
        data-testid="ctrl-ev-soc_target"
        onChange={(_e, v) => onChange({ ev_soc_target: (v as number) / 100 })}
        valueLabelDisplay="auto"
        valueLabelFormat={(v) => `${v}%`}
      />
    </Box>
  );
}

// ─── Battery ─────────────────────────────────────────────────────────────────

function BatteryStatusControls({
  sim,
  overrides,
  onChange,
}: {
  sim?: SimSnapshot;
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  const socPct = sim?.battery ? sim.battery.soc * 100 : 50;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">SoC: {socPct.toFixed(0)}%</Typography>
      <Slider
        size="small"
        min={0}
        max={100}
        step={1}
        value={socPct}
        data-testid="ctrl-battery-soc"
        onChange={(_e, v) => onChange({ battery_initial_soc: (v as number) / 100 })}
        valueLabelDisplay="auto"
        valueLabelFormat={(v) => `${v}%`}
      />
    </Box>
  );
}

function BatterySimCharacteristics({
  sim,
  overrides,
  onChange,
}: {
  sim?: SimSnapshot;
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  const cap = overrides?.battery_capacity_kwh ?? sim?.battery?.capacity_kwh ?? 10;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Capacity kWh: {cap.toFixed(1)}</Typography>
      <Slider
        size="small"
        min={1}
        max={100}
        step={1}
        value={cap}
        data-testid="ctrl-battery-battery_capacity_kwh"
        onChange={(_e, v) => onChange({ battery_capacity_kwh: v as number })}
        valueLabelDisplay="auto"
      />
      <Typography variant="caption">Force charge/discharge kW</Typography>
      <Slider
        size="small"
        min={-20}
        max={20}
        step={0.5}
        value={overrides?.battery_force_kw ?? 0}
        data-testid="ctrl-battery-battery_force_kw"
        onChange={(_e, v) => onChange({ battery_force_kw: v as number || undefined })}
        valueLabelDisplay="auto"
      />
    </Box>
  );
}

// ─── Heater ──────────────────────────────────────────────────────────────────

function HeaterStatusControls({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Force kW</Typography>
      <Slider
        size="small"
        min={0}
        max={6}
        step={0.1}
        value={overrides?.heater_force_kw ?? 0}
        data-testid="ctrl-heater-heater_force_kw"
        onChange={(_e, v) => onChange({ heater_force_kw: (v as number) || undefined })}
        valueLabelDisplay="auto"
      />
    </Box>
  );
}

function HeaterSimCharacteristics({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Max kW</Typography>
      <Slider
        size="small"
        min={0}
        max={6}
        step={0.1}
        value={overrides?.heater_max_kw ?? 3}
        data-testid="ctrl-heater-heater_max_kw"
        onChange={(_e, v) => onChange({ heater_max_kw: v as number })}
        valueLabelDisplay="auto"
      />
      <Typography variant="caption">Temp range [°C]</Typography>
      <Slider
        size="small"
        min={0}
        max={30}
        step={0.5}
        value={[overrides?.heater_temp_min_c ?? 18, overrides?.heater_temp_max_c ?? 22]}
        data-testid="ctrl-heater-temp_range"
        onChange={(_e, v) => {
          const [min, max] = v as number[];
          onChange({ heater_temp_min_c: min, heater_temp_max_c: max });
        }}
        valueLabelDisplay="auto"
      />
      <Typography variant="caption">Ambient temp [°C]</Typography>
      <TextField
        size="small"
        type="number"
        value={overrides?.ambient_temp_c ?? 10}
        inputProps={{ step: 0.5, "data-testid": "ctrl-heater-ambient_temp_c" }}
        onChange={(e) => onChange({ ambient_temp_c: parseFloat(e.target.value) || undefined })}
      />
    </Box>
  );
}

// ─── PV ──────────────────────────────────────────────────────────────────────

function PvStatusControls({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Irradiance (0–1)</Typography>
      <Slider
        size="small"
        min={0}
        max={1}
        step={0.05}
        value={overrides?.pv_irradiance ?? 0.5}
        data-testid="ctrl-pv-pv_irradiance"
        onChange={(_e, v) => onChange({ pv_irradiance: v as number })}
        valueLabelDisplay="auto"
      />
    </Box>
  );
}

function PvSimCharacteristics({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Rated kW</Typography>
      <Slider
        size="small"
        min={0}
        max={20}
        step={0.5}
        value={overrides?.pv_rated_kw ?? 5}
        data-testid="ctrl-pv-pv_rated_kw"
        onChange={(_e, v) => onChange({ pv_rated_kw: v as number })}
        valueLabelDisplay="auto"
      />
      <Typography variant="caption">Export limit kW</Typography>
      <Slider
        size="small"
        min={0}
        max={20}
        step={0.5}
        value={overrides?.pv_force_export_limit_kw ?? 0}
        data-testid="ctrl-pv-pv_force_export_limit_kw"
        onChange={(_e, v) => onChange({ pv_force_export_limit_kw: (v as number) || undefined })}
        valueLabelDisplay="auto"
      />
    </Box>
  );
}

// ─── BaseLoad ────────────────────────────────────────────────────────────────

function BaseLoadStatusControls({
  overrides,
  onChange,
}: {
  overrides?: UserOverrides;
  onChange: (p: Partial<UserOverrides>) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="caption">Base load W</Typography>
      <Slider
        size="small"
        min={0}
        max={5000}
        step={50}
        value={overrides?.base_load_w ?? 500}
        data-testid="ctrl-base_load-base_load_w"
        onChange={(_e, v) => onChange({ base_load_w: v as number })}
        valueLabelDisplay="auto"
      />
    </Box>
  );
}
