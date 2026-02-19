import { useCallback, useEffect, useRef, useState } from "react";
import {
  Box,
  Chip,
  Divider,
  FormControlLabel,
  Grid,
  Paper,
  Slider,
  Stack,
  Switch,
  Typography,
} from "@mui/material";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useSim, useTrace, useSimOverride, useSetSimOverride } from "../api/hooks";
import type { UserOverrides, SimSnapshot } from "../api/types";

function fmtNum(v: number | undefined | null, decimals = 1): string {
  if (v == null) return "—";
  return v.toFixed(decimals);
}

// ─── Section A: Device State Cards ──────────────────────────────────────────

function PowerCard({ sim }: { sim: SimSnapshot }) {
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Power &amp; Energy
      </Typography>
      <Grid container spacing={1}>
        <Grid item xs={6}>
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary">Instantaneous</Typography>
            <Typography>Net: {fmtNum(sim.net_power_w, 0)} W</Typography>
            <Typography>Import: {fmtNum(sim.import_w, 0)} W</Typography>
            <Typography>Export: {fmtNum(sim.export_w, 0)} W</Typography>
            <Typography>Base load: {fmtNum(sim.base_load_w, 0)} W</Typography>
          </Stack>
        </Grid>
        <Grid item xs={6}>
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary">Session totals</Typography>
            <Typography>Import: {fmtNum(sim.import_kwh, 3)} kWh</Typography>
            <Typography>Export: {fmtNum(sim.export_kwh, 3)} kWh</Typography>
          </Stack>
        </Grid>
      </Grid>
    </Paper>
  );
}

function EvCard({ sim }: { sim: SimSnapshot }) {
  if (!sim.ev) return null;
  const { soc, plugged, current_kw, max_charge_kw, soc_target, battery_kwh } = sim.ev;
  const socPct = (soc * 100).toFixed(1);
  const targetPct = (soc_target * 100).toFixed(0);
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        EV Charger
      </Typography>
      <Stack spacing={0.5}>
        <Stack direction="row" alignItems="center" spacing={1}>
          <Typography>SOC: {socPct}%</Typography>
          <Chip
            label={plugged ? "Plugged" : "Unplugged"}
            size="small"
            color={plugged ? "success" : "default"}
          />
        </Stack>
        <Box sx={{ px: 1 }}>
          <Box
            sx={{
              height: 8,
              borderRadius: 1,
              bgcolor: "grey.200",
              position: "relative",
              overflow: "hidden",
            }}
          >
            <Box
              sx={{
                position: "absolute",
                left: 0,
                top: 0,
                bottom: 0,
                width: `${soc * 100}%`,
                bgcolor: soc > 0.8 ? "success.main" : soc > 0.3 ? "primary.main" : "warning.main",
                transition: "width 0.5s",
              }}
            />
          </Box>
        </Box>
        <Typography>Charging: {fmtNum(current_kw)} kW / {fmtNum(max_charge_kw)} kW max</Typography>
        <Typography>Target SOC: {targetPct}% | Capacity: {fmtNum(battery_kwh)} kWh</Typography>
      </Stack>
    </Paper>
  );
}

function HeaterCard({ sim }: { sim: SimSnapshot }) {
  if (!sim.heater) return null;
  const { temp_c, current_kw, max_kw, temp_min_c, temp_max_c } = sim.heater;
  const tempRange = temp_max_c - temp_min_c;
  const tempFraction = Math.max(0, Math.min(1, (temp_c - temp_min_c) / (tempRange || 1)));
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Heater
      </Typography>
      <Stack spacing={0.5}>
        <Typography>Temperature: {fmtNum(temp_c, 1)}°C</Typography>
        <Box sx={{ px: 1 }}>
          <Box
            sx={{
              height: 8,
              borderRadius: 1,
              bgcolor: "grey.200",
              position: "relative",
              overflow: "hidden",
            }}
          >
            <Box
              sx={{
                position: "absolute",
                left: 0,
                top: 0,
                bottom: 0,
                width: `${tempFraction * 100}%`,
                bgcolor: temp_c > temp_max_c - 2 ? "error.main" : temp_c < temp_min_c + 2 ? "info.main" : "warning.main",
                transition: "width 0.5s",
              }}
            />
          </Box>
          <Stack direction="row" justifyContent="space-between">
            <Typography variant="caption">{fmtNum(temp_min_c, 0)}°C</Typography>
            <Typography variant="caption">{fmtNum(temp_max_c, 0)}°C</Typography>
          </Stack>
        </Box>
        <Typography>Heating: {fmtNum(current_kw)} kW / {fmtNum(max_kw)} kW max</Typography>
      </Stack>
    </Paper>
  );
}

function PvCard({ sim }: { sim: SimSnapshot }) {
  if (!sim.pv) return null;
  const { irradiance, curtailment, current_kw, rated_kw } = sim.pv;
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        PV Inverter
      </Typography>
      <Stack spacing={0.5}>
        <Typography>Output: {fmtNum(current_kw)} kW / {fmtNum(rated_kw)} kW rated</Typography>
        <Stack direction="row" spacing={1}>
          <Typography>Irradiance: {(irradiance * 100).toFixed(0)}%</Typography>
          <Typography>Curtailment: {(curtailment * 100).toFixed(0)}%</Typography>
        </Stack>
        <Box sx={{ px: 1 }}>
          <Box
            sx={{
              height: 8,
              borderRadius: 1,
              bgcolor: "grey.200",
              position: "relative",
              overflow: "hidden",
            }}
          >
            <Box
              sx={{
                position: "absolute",
                left: 0,
                top: 0,
                bottom: 0,
                width: `${irradiance * 100}%`,
                bgcolor: "warning.main",
                transition: "width 0.5s",
              }}
            />
          </Box>
        </Box>
      </Stack>
    </Paper>
  );
}

// ─── Section B: Time-Series Chart ────────────────────────────────────────────

type ChartPoint = {
  time: string;
  ev_charge_kw?: number;
  heater_kw?: number;
  pv_curtailment_pct?: number;
};

function TraceChart({ sim }: { sim: SimSnapshot | undefined }) {
  const traceQuery = useTrace(100);

  const chartData: ChartPoint[] = (traceQuery.data ?? [])
    .slice()
    .reverse()
    .map((entry) => ({
      time: new Date(entry.ts).toLocaleTimeString(),
      ev_charge_kw: sim?.ev != null ? entry.setpoints.ev_charge_kw : undefined,
      heater_kw: sim?.heater != null ? entry.setpoints.heater_kw : undefined,
      pv_curtailment_pct: sim?.pv != null ? entry.setpoints.pv_curtailment * 100 : undefined,
    }));

  const hasEv = sim?.ev != null;
  const hasHeater = sim?.heater != null;
  const hasPv = sim?.pv != null;

  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={2}>
        Reactor Setpoints (last 100 ticks)
      </Typography>
      {chartData.length === 0 ? (
        <Typography color="text.secondary">No trace data yet…</Typography>
      ) : (
        <ResponsiveContainer width="100%" height={280}>
          <LineChart data={chartData} margin={{ top: 4, right: 16, left: 0, bottom: 4 }}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis
              dataKey="time"
              tick={{ fontSize: 11 }}
              interval="preserveStartEnd"
            />
            <YAxis tick={{ fontSize: 11 }} />
            <Tooltip />
            <Legend />
            {hasEv && (
              <Line
                type="monotone"
                dataKey="ev_charge_kw"
                name="EV charge (kW)"
                stroke="#1976d2"
                dot={false}
                isAnimationActive={false}
              />
            )}
            {hasHeater && (
              <Line
                type="monotone"
                dataKey="heater_kw"
                name="Heater (kW)"
                stroke="#ed6c02"
                dot={false}
                isAnimationActive={false}
              />
            )}
            {hasPv && (
              <Line
                type="monotone"
                dataKey="pv_curtailment_pct"
                name="PV curtailment (%)"
                stroke="#f5c518"
                dot={false}
                isAnimationActive={false}
              />
            )}
          </LineChart>
        </ResponsiveContainer>
      )}
    </Paper>
  );
}

// ─── Section C: Simulation Controls ──────────────────────────────────────────

type ControlsProps = {
  sim: SimSnapshot;
  overrides: UserOverrides;
  onChange: (patch: Partial<UserOverrides>) => void;
  isEventActive: boolean;
};

function EvControls({ sim, overrides, onChange, isEventActive }: ControlsProps) {
  if (!sim.ev) return null;
  const maxKw = overrides.ev_max_charge_kw ?? sim.ev.max_charge_kw;
  return (
    <Box>
      <Stack direction="row" alignItems="center" spacing={1} mb={1}>
        <Typography variant="subtitle2">EV Charger</Typography>
        {isEventActive && (
          <Chip label="⚡ Event active" size="small" color="warning" />
        )}
      </Stack>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <Box>
          <Typography variant="body2" gutterBottom>
            Desired charge rate: {fmtNum(overrides.ev_desired_kw ?? sim.ev.max_charge_kw)} kW
            {isEventActive && (
              <Typography component="span" variant="caption" color="text.secondary" ml={1}>
                (overridden by event)
              </Typography>
            )}
          </Typography>
          <Slider
            min={0}
            max={maxKw}
            step={0.1}
            value={overrides.ev_desired_kw ?? sim.ev.max_charge_kw}
            onChange={(_, v) => onChange({ ev_desired_kw: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v.toFixed(1)} kW`}
          />
        </Box>
        <Box>
          <Typography variant="body2" gutterBottom>
            Max charge rate (profile override): {fmtNum(overrides.ev_max_charge_kw ?? sim.ev.max_charge_kw)} kW
          </Typography>
          <Slider
            min={0}
            max={22}
            step={0.5}
            value={overrides.ev_max_charge_kw ?? sim.ev.max_charge_kw}
            onChange={(_, v) => onChange({ ev_max_charge_kw: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v.toFixed(1)} kW`}
          />
        </Box>
        <Box>
          <Typography variant="body2" gutterBottom>
            SOC target: {((overrides.ev_soc_target ?? sim.ev.soc_target) * 100).toFixed(0)}%
          </Typography>
          <Slider
            min={0}
            max={1}
            step={0.05}
            value={overrides.ev_soc_target ?? sim.ev.soc_target}
            onChange={(_, v) => onChange({ ev_soc_target: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${(v * 100).toFixed(0)}%`}
          />
        </Box>
        <FormControlLabel
          control={
            <Switch
              checked={overrides.ev_plugged ?? true}
              onChange={(e) => onChange({ ev_plugged: e.target.checked })}
            />
          }
          label="Plugged in"
        />
      </Stack>
    </Box>
  );
}

function PvControls({ sim, overrides, onChange }: ControlsProps) {
  if (!sim.pv) return null;
  const [manualIrradiance, setManualIrradiance] = useState(overrides.pv_irradiance != null);

  function handleManualToggle(checked: boolean) {
    setManualIrradiance(checked);
    if (!checked) {
      onChange({ pv_irradiance: undefined });
    } else {
      onChange({ pv_irradiance: sim.pv?.irradiance ?? 0.5 });
    }
  }

  return (
    <Box>
      <Typography variant="subtitle2" mb={1}>PV Inverter</Typography>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <FormControlLabel
          control={
            <Switch
              checked={manualIrradiance}
              onChange={(e) => handleManualToggle(e.target.checked)}
            />
          }
          label={manualIrradiance ? "Irradiance — Manual" : "Irradiance — Auto (time-based)"}
        />
        {manualIrradiance && (
          <Box>
            <Typography variant="body2" gutterBottom>
              Irradiance: {((overrides.pv_irradiance ?? 0) * 100).toFixed(0)}%
            </Typography>
            <Slider
              min={0}
              max={1}
              step={0.01}
              value={overrides.pv_irradiance ?? 0}
              onChange={(_, v) => onChange({ pv_irradiance: v as number })}
              valueLabelDisplay="auto"
              valueLabelFormat={(v) => `${(v * 100).toFixed(0)}%`}
            />
          </Box>
        )}
        <Box>
          <Typography variant="body2" gutterBottom>
            Rated capacity (profile override): {fmtNum(overrides.pv_rated_kw ?? sim.pv.rated_kw)} kW
          </Typography>
          <Slider
            min={0}
            max={20}
            step={0.5}
            value={overrides.pv_rated_kw ?? sim.pv.rated_kw}
            onChange={(_, v) => onChange({ pv_rated_kw: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v.toFixed(1)} kW`}
          />
        </Box>
      </Stack>
    </Box>
  );
}

function HeaterControls({ sim, overrides, onChange }: ControlsProps) {
  if (!sim.heater) return null;
  const minC = overrides.heater_temp_min_c ?? sim.heater.temp_min_c;
  const maxC = overrides.heater_temp_max_c ?? sim.heater.temp_max_c;
  return (
    <Box>
      <Typography variant="subtitle2" mb={1}>Heater</Typography>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <Box>
          <Typography variant="body2" gutterBottom>
            Ambient temperature: {fmtNum(overrides.ambient_temp_c ?? 10.0, 0)}°C
          </Typography>
          <Slider
            min={-15}
            max={40}
            step={1}
            value={overrides.ambient_temp_c ?? 10.0}
            onChange={(_, v) => onChange({ ambient_temp_c: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v}°C`}
          />
        </Box>
        <Box>
          <Typography variant="body2" gutterBottom>
            Max heating power (profile override): {fmtNum(overrides.heater_max_kw ?? sim.heater.max_kw)} kW
          </Typography>
          <Slider
            min={0}
            max={10}
            step={0.1}
            value={overrides.heater_max_kw ?? sim.heater.max_kw}
            onChange={(_, v) => onChange({ heater_max_kw: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v.toFixed(1)} kW`}
          />
        </Box>
        <Box>
          <Typography variant="body2" gutterBottom>
            Thermostat range: {fmtNum(minC, 0)}°C – {fmtNum(maxC, 0)}°C
          </Typography>
          <Slider
            min={5}
            max={30}
            step={1}
            value={[minC, maxC]}
            onChange={(_, v) => {
              const [lo, hi] = v as number[];
              onChange({ heater_temp_min_c: lo, heater_temp_max_c: hi });
            }}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v}°C`}
          />
        </Box>
      </Stack>
    </Box>
  );
}

function BaseLoadControls({ overrides, onChange, sim }: ControlsProps) {
  return (
    <Box>
      <Typography variant="subtitle2" mb={1}>Base Load</Typography>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <Box>
          <Typography variant="body2" gutterBottom>
            Base load (profile override): {fmtNum(overrides.base_load_w ?? sim.base_load_w, 0)} W
          </Typography>
          <Slider
            min={0}
            max={5000}
            step={50}
            value={overrides.base_load_w ?? sim.base_load_w}
            onChange={(_, v) => onChange({ base_load_w: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v} W`}
          />
        </Box>
      </Stack>
    </Box>
  );
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export function SimulationPage() {
  const simQuery = useSim();
  const overrideQuery = useSimOverride();
  const setOverrideMutation = useSetSimOverride();

  const [localOverrides, setLocalOverrides] = useState<UserOverrides>({});
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Initialize local overrides from server once
  useEffect(() => {
    if (overrideQuery.data && Object.keys(localOverrides).length === 0) {
      setLocalOverrides(overrideQuery.data);
    }
  }, [overrideQuery.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const updateOverride = useCallback(
    (patch: Partial<UserOverrides>) => {
      setLocalOverrides((prev) => {
        const updated = { ...prev, ...patch };
        // Clear undefined keys
        Object.keys(updated).forEach((k) => {
          if ((updated as Record<string, unknown>)[k] === undefined) {
            delete (updated as Record<string, unknown>)[k];
          }
        });
        if (timerRef.current) clearTimeout(timerRef.current);
        timerRef.current = setTimeout(() => {
          setOverrideMutation.mutate(updated);
        }, 500);
        return updated;
      });
    },
    [setOverrideMutation]
  );

  const sim = simQuery.data;
  const traceQuery = useTrace(1);
  const isEventActive = traceQuery.data?.[0]?.mode !== "IDLE" && traceQuery.data?.[0]?.mode != null;

  return (
    <Stack spacing={3}>
      <Typography variant="h5">Simulation</Typography>

      {/* Section A — Device State */}
      <Box>
        <Typography variant="h6" mb={1}>Device State</Typography>
        {simQuery.isError ? (
          <Typography color="error">Simulator not available</Typography>
        ) : !sim ? (
          <Typography color="text.secondary">Loading…</Typography>
        ) : (
          <Grid container spacing={2}>
            <Grid item xs={12} md={6}>
              <PowerCard sim={sim} />
            </Grid>
            {sim.ev && (
              <Grid item xs={12} md={6}>
                <EvCard sim={sim} />
              </Grid>
            )}
            {sim.heater && (
              <Grid item xs={12} md={6}>
                <HeaterCard sim={sim} />
              </Grid>
            )}
            {sim.pv && (
              <Grid item xs={12} md={6}>
                <PvCard sim={sim} />
              </Grid>
            )}
          </Grid>
        )}
      </Box>

      <Divider />

      {/* Section B — Chart */}
      <Box>
        <Typography variant="h6" mb={1}>Setpoints Over Time</Typography>
        <TraceChart sim={sim} />
      </Box>

      <Divider />

      {/* Section C — Controls */}
      <Box>
        <Typography variant="h6" mb={1}>Simulation Controls</Typography>
        {!sim ? (
          <Typography color="text.secondary">Waiting for simulator…</Typography>
        ) : (
          <Paper sx={{ p: 3 }}>
            <Stack spacing={3} divider={<Divider />}>
              <EvControls
                sim={sim}
                overrides={localOverrides}
                onChange={updateOverride}
                isEventActive={isEventActive}
              />
              <PvControls
                sim={sim}
                overrides={localOverrides}
                onChange={updateOverride}
                isEventActive={isEventActive}
              />
              <HeaterControls
                sim={sim}
                overrides={localOverrides}
                onChange={updateOverride}
                isEventActive={isEventActive}
              />
              <BaseLoadControls
                sim={sim}
                overrides={localOverrides}
                onChange={updateOverride}
                isEventActive={isEventActive}
              />
            </Stack>
          </Paper>
        )}
      </Box>
    </Stack>
  );
}
