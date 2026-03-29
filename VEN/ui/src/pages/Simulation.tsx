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
import { useSim, useTrace, useEvents, useSimInject, useSetSimInject } from "../api/hooks";
import type { SimInjectState, SimSnapshot, VtnEvent } from "../api/types";

function fmtNum(v: number | undefined | null, decimals = 1): string {
  if (v == null) return "—";
  return v.toFixed(decimals);
}

// ─── Section A: Device State Cards ──────────────────────────────────────────

function PowerCard({ sim }: { sim: SimSnapshot }) {
  const netW = sim.grid.net_power_w;
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Power &amp; Energy
      </Typography>
      <Grid container spacing={1}>
        <Grid item xs={6}>
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary">Instantaneous</Typography>
            <Typography>Net: {fmtNum(netW, 0)} W</Typography>
            <Typography>Import: {fmtNum(netW > 0 ? netW : 0, 0)} W</Typography>
            <Typography>Export: {fmtNum(netW < 0 ? -netW : 0, 0)} W</Typography>
            <Typography>Base load: {fmtNum((sim.assets["base_load"]?.power_kw ?? 0) * 1000, 0)} W</Typography>
          </Stack>
        </Grid>
        <Grid item xs={6}>
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary">Session totals</Typography>
            <Typography>Import: {fmtNum(sim.grid.import_kwh, 3)} kWh</Typography>
            <Typography>Export: {fmtNum(sim.grid.export_kwh, 3)} kWh</Typography>
          </Stack>
        </Grid>
      </Grid>
    </Paper>
  );
}

function EvCard({ sim }: { sim: SimSnapshot }) {
  const evAsset = sim.assets["ev"];
  if (!evAsset) return null;
  const soc = evAsset.soc ?? 0;
  const plugged = (evAsset.plugged ?? 0) !== 0;
  const current_kw = evAsset.power_kw;
  const max_charge_kw = evAsset.max_charge_kw ?? 0;
  const soc_target = evAsset.soc_target ?? 0;
  const battery_kwh = evAsset.battery_kwh ?? 0;
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
  const heaterAsset = sim.assets["heater"];
  if (!heaterAsset) return null;
  const temp_c = heaterAsset.temp_c ?? 0;
  const current_kw = heaterAsset.power_kw;
  const max_kw = heaterAsset.max_kw ?? 0;
  const temp_min_c = heaterAsset.temp_min_c ?? 0;
  const temp_max_c = heaterAsset.temp_max_c ?? 0;
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
  const pvAsset = sim.assets["pv"];
  if (!pvAsset) return null;
  const irradiance = pvAsset.irradiance ?? 0;
  const current_kw = pvAsset.power_kw;
  const rated_kw = pvAsset.rated_kw ?? 0;
  const export_limit_kw = "export_limit_kw" in pvAsset ? pvAsset.export_limit_kw : null;
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        PV Inverter
      </Typography>
      <Stack spacing={0.5}>
        <Typography>Output: {fmtNum(current_kw)} kW / {fmtNum(rated_kw)} kW rated</Typography>
        <Stack direction="row" spacing={1}>
          <Typography>Irradiance: {(irradiance * 100).toFixed(0)}%</Typography>
          <Typography>
            Export limit: {export_limit_kw != null ? `${fmtNum(export_limit_kw)} kW` : "none"}
          </Typography>
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
  isFuture?: boolean;
  // Actual dispatcher setpoints (past only)
  ev_charge_kw?: number;
  heater_kw?: number;
  pv_export_limit_kw?: number;   // curtailment_pct/100 * rated_kw
  // Desired values from VTN event payloads (past + future)
  ev_desired?: number;
  heater_desired?: number;
  pv_desired?: number;
};

/** Parse ISO 8601 duration string (e.g. "PT1H30M") → seconds. */
function parseIsoDuration(s: string): number {
  const m = s.match(
    /^P(?:(\d+)Y)?(?:(\d+)M)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$/
  );
  if (!m) return 0;
  return (
    parseFloat(m[1] ?? "0") * 365 * 24 * 3600 +
    parseFloat(m[2] ?? "0") * 30 * 24 * 3600 +
    parseFloat(m[3] ?? "0") * 24 * 3600 +
    parseFloat(m[4] ?? "0") * 3600 +
    parseFloat(m[5] ?? "0") * 60 +
    parseFloat(m[6] ?? "0")
  );
}

/**
 * For a given Unix timestamp (ms) and event list, return the winning event's
 * payload value for `payloadType`. Returns undefined if no event interval is
 * active at that moment. Arbitration: lowest priority wins; newest
 * createdDateTime breaks ties (mirrors planner arbitration).
 */
function getDesiredValue(
  tsMs: number,
  events: VtnEvent[],
  payloadType: string
): number | undefined {
  type Candidate = { priority: number; createdMs: number; value: number };
  const candidates: Candidate[] = [];

  for (const event of events) {
    const topStart = event.intervalPeriod?.start
      ? new Date(event.intervalPeriod.start).getTime()
      : null;
    const topDurationMs = event.intervalPeriod?.duration
      ? parseIsoDuration(event.intervalPeriod.duration) * 1000
      : null;

    for (const interval of event.intervals ?? []) {
      const startMs =
        interval.intervalPeriod?.start
          ? new Date(interval.intervalPeriod.start).getTime()
          : topStart;
      const durationMs =
        interval.intervalPeriod?.duration
          ? parseIsoDuration(interval.intervalPeriod.duration) * 1000
          : topDurationMs;

      if (startMs == null) continue;
      const endMs = durationMs != null ? startMs + durationMs : Infinity;
      if (tsMs < startMs || tsMs >= endMs) continue;

      const payload = interval.payloads?.find((p) => p.type === payloadType);
      if (!payload || payload.values.length === 0) continue;

      candidates.push({
        priority: event.priority ?? 999,
        createdMs: event.createdDateTime
          ? new Date(event.createdDateTime).getTime()
          : 0,
        value: payload.values[0],
      });
    }
  }

  if (candidates.length === 0) return undefined;
  candidates.sort((a, b) =>
    a.priority !== b.priority
      ? a.priority - b.priority
      : b.createdMs - a.createdMs
  );
  return candidates[0].value;
}

function TraceChart({ sim }: { sim: SimSnapshot | undefined }) {
  const traceQuery = useTrace(1000);
  const eventsQuery = useEvents();
  const events = eventsQuery.data ?? [];

  // Chronological order from the (newest-first) trace response
  const traceEntries = [...(traceQuery.data ?? [])].reverse();

  // Infer tick interval from consecutive timestamps (median of first 10 diffs)
  let tickIntervalMs = 1000;
  if (traceEntries.length >= 2) {
    const diffs: number[] = [];
    for (let i = 1; i < Math.min(traceEntries.length, 11); i++) {
      const d =
        new Date(traceEntries[i].ts).getTime() -
        new Date(traceEntries[i - 1].ts).getTime();
      if (d > 0) diffs.push(d);
    }
    if (diffs.length > 0) {
      diffs.sort((a, b) => a - b);
      tickIntervalMs = diffs[Math.floor(diffs.length / 2)];
    }
  }

  const hasEv = "ev" in (sim?.assets ?? {});
  const hasHeater = "heater" in (sim?.assets ?? {});
  const hasPv = "pv" in (sim?.assets ?? {});

  // Past points — actual setpoints + desired from events
  // Guard: new ControllerEvent format no longer has setpoints; entries skipped until Phase 4
  const pastPoints: ChartPoint[] = traceEntries.filter((e) => !!e.setpoints).map((entry) => {
    const tsMs = new Date(entry.ts).getTime();
    return {
      time: new Date(entry.ts).toLocaleTimeString(),
      isFuture: false,
      ev_charge_kw: hasEv ? entry.setpoints.ev_charge_kw : undefined,
      heater_kw: hasHeater ? entry.setpoints.heater_kw : undefined,
      pv_export_limit_kw: hasPv ? (entry.setpoints.pv_export_limit_kw ?? undefined) : undefined,
      ev_desired: hasEv ? getDesiredValue(tsMs, events, "CHARGE_STATE_SETPOINT") : undefined,
      heater_desired: hasHeater ? getDesiredValue(tsMs, events, "IMPORT_CAPACITY_LIMIT") : undefined,
      pv_desired: hasPv ? getDesiredValue(tsMs, events, "EXPORT_CAPACITY_LIMIT") : undefined,
    };
  });

  // Future points — desired from events only (500 synthetic ticks)
  const FUTURE_COUNT = 500;
  const now = Date.now();
  const futurePoints: ChartPoint[] = Array.from({ length: FUTURE_COUNT }, (_, i) => {
    const tsMs = now + (i + 1) * tickIntervalMs;
    return {
      time: new Date(tsMs).toLocaleTimeString(),
      isFuture: true,
      ev_desired: hasEv ? getDesiredValue(tsMs, events, "CHARGE_STATE_SETPOINT") : undefined,
      heater_desired: hasHeater ? getDesiredValue(tsMs, events, "IMPORT_CAPACITY_LIMIT") : undefined,
      pv_desired: hasPv ? getDesiredValue(tsMs, events, "EXPORT_CAPACITY_LIMIT") : undefined,
    };
  });

  const chartData: ChartPoint[] = [...pastPoints, ...futurePoints];

  const isEvEventActive = chartData.some((p) => p.ev_desired !== undefined);
  const isImportCapActive = chartData.some((p) => p.heater_desired !== undefined);
  const isExportCapActive = chartData.some((p) => p.pv_desired !== undefined);

  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="subtitle1" fontWeight="bold" mb={2}>
        Dispatcher Setpoints — last 1 000 ticks + 8 min projection
      </Typography>
      {traceEntries.length === 0 ? (
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
                dataKey="pv_export_limit_kw"
                name="PV export limit (kW)"
                stroke="#f5c518"
                dot={false}
                isAnimationActive={false}
              />
            )}
            {hasEv && isEvEventActive && (
              <Line
                type="monotone"
                dataKey="ev_desired"
                name="EV target (VTN)"
                stroke="#1976d2"
                strokeDasharray="5 5"
                dot={false}
                isAnimationActive={false}
              />
            )}
            {hasHeater && isImportCapActive && (
              <Line
                type="monotone"
                dataKey="heater_desired"
                name="Import cap (VTN)"
                stroke="#7b1fa2"
                strokeDasharray="5 5"
                dot={false}
                isAnimationActive={false}
              />
            )}
            {hasPv && isExportCapActive && (
              <Line
                type="monotone"
                dataKey="pv_desired"
                name="Export cap (VTN)"
                stroke="#388e3c"
                strokeDasharray="5 5"
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
  overrides: SimInjectState;
  onChange: (patch: Partial<SimInjectState>) => void;
};


function EvControls({ sim, overrides, onChange }: ControlsProps) {
  const evAsset = sim.assets["ev"];
  if (!evAsset) return null;
  const simSocTarget = evAsset.soc_target ?? 0.8;

  return (
    <Box>
      <Typography variant="subtitle2" mb={1}>EV Charger</Typography>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <Box>
          <Typography variant="body2" gutterBottom>
            SOC target: {((overrides.ev_soc_target ?? simSocTarget) * 100).toFixed(0)}%
          </Typography>
          <Typography variant="caption" color="text.secondary">BMS charge ceiling — charging stops here</Typography>
          <Slider
            min={0.1}
            max={1}
            step={0.05}
            value={overrides.ev_soc_target ?? simSocTarget}
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
  const pvAsset = sim.assets["pv"];
  if (!pvAsset) return null;
  const simIrradiance = pvAsset.irradiance ?? 0;
  const [manualIrradiance, setManualIrradiance] = useState(overrides.pv_irradiance != null);

  function handleManualToggle(checked: boolean) {
    setManualIrradiance(checked);
    if (!checked) {
      onChange({ pv_irradiance: null });
    } else {
      onChange({ pv_irradiance: simIrradiance ?? 0.5 });
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
              Irradiance: {((overrides.pv_irradiance ?? simIrradiance) * 100).toFixed(0)}%
            </Typography>
            <Slider
              min={0}
              max={1}
              step={0.01}
              value={overrides.pv_irradiance ?? simIrradiance}
              onChange={(_, v) => onChange({ pv_irradiance: v as number })}
              valueLabelDisplay="auto"
              valueLabelFormat={(v) => `${(v * 100).toFixed(0)}%`}
            />
          </Box>
        )}
      </Stack>
    </Box>
  );
}

function HeaterControls({ sim, overrides, onChange }: ControlsProps) {
  const heaterAsset = sim.assets["heater"];
  if (!heaterAsset) return null;
  const minC = overrides.heater_temp_min_c ?? (heaterAsset.temp_min_c ?? 18);
  const maxC = overrides.heater_temp_max_c ?? (heaterAsset.temp_max_c ?? 24);

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
            Thermostat range: {fmtNum(minC, 0)}°C – {fmtNum(maxC, 0)}°C
          </Typography>
          <Typography variant="caption" color="text.secondary">Comfort band — heater cuts off above max, forced on below min</Typography>
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
  const baseLoadKw = sim.assets["base_load"]?.power_kw ?? 0;
  return (
    <Box>
      <Typography variant="subtitle2" mb={1}>Base Load</Typography>
      <Stack spacing={2} sx={{ pl: 1 }}>
        <Box>
          <Typography variant="body2" gutterBottom>
            Base load (profile override): {fmtNum(overrides.base_load_kw ?? baseLoadKw, 2)} kW
          </Typography>
          <Slider
            min={0}
            max={5}
            step={0.05}
            value={overrides.base_load_kw ?? baseLoadKw}
            onChange={(_, v) => onChange({ base_load_kw: v as number })}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => `${v.toFixed(2)} kW`}
          />
        </Box>
      </Stack>
    </Box>
  );
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export function SimulationPage() {
  const simQuery = useSim();
  const injectQuery = useSimInject();
  const setInjectMutation = useSetSimInject();

  const [localInject, setLocalInject] = useState<SimInjectState>({});
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingPatchRef = useRef<Partial<SimInjectState>>({});

  // Initialize local inject from server once
  useEffect(() => {
    if (injectQuery.data && Object.keys(localInject).length === 0) {
      setLocalInject(injectQuery.data);
    }
  }, [injectQuery.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const updateInject = useCallback(
    (patch: Partial<SimInjectState>) => {
      setLocalInject((prev) => ({ ...prev, ...patch }));
      pendingPatchRef.current = { ...pendingPatchRef.current, ...patch };
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => {
        const firing = pendingPatchRef.current;
        setInjectMutation.mutate(firing);
        pendingPatchRef.current = {};
        // pv_irradiance is a one-shot: backend clears it after applying.
        // Drop it from localInject too so the slider follows the live sim value.
        if (firing.pv_irradiance != null) {
          setLocalInject((prev) => {
            const { pv_irradiance: _, ...rest } = prev;
            return rest;
          });
        }
      }, 500);
    },
    [setInjectMutation]
  );

  const sim = simQuery.data;

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
            {"ev" in sim.assets && (
              <Grid item xs={12} md={6}>
                <EvCard sim={sim} />
              </Grid>
            )}
            {"heater" in sim.assets && (
              <Grid item xs={12} md={6}>
                <HeaterCard sim={sim} />
              </Grid>
            )}
            {"pv" in sim.assets && (
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
                overrides={localInject}
                onChange={updateInject}
              />
              <PvControls
                sim={sim}
                overrides={localInject}
                onChange={updateInject}
              />
              <HeaterControls
                sim={sim}
                overrides={localInject}
                onChange={updateInject}
              />
              <BaseLoadControls
                sim={sim}
                overrides={localInject}
                onChange={updateInject}
              />
            </Stack>
          </Paper>
        )}
      </Box>
    </Stack>
  );
}
