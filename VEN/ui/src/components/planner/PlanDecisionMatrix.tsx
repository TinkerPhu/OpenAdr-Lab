import { useState, useMemo } from "react";
import {
  Box, Drawer, IconButton, Stack, Table, TableBody, TableCell,
  TableRow, Tooltip, Typography,
} from "@mui/material";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import type { Plan, PlanStep, PlanTimeSlot } from "../../api/types";

// ─── Tariff → color (green → yellow → red) ───────────────────────────────────

function tariffColor(value: number, min: number, max: number): string {
  if (max === min) return "#4caf50";
  const t = (value - min) / (max - min);
  if (t < 0.5) {
    const g = Math.round(255 * (1 - t * 2) + 150 * t * 2);
    return `rgb(${Math.round(t * 2 * 255)}, ${g}, 0)`;
  }
  const r = 255;
  const g = Math.round(150 * (1 - (t - 0.5) * 2));
  return `rgb(${r}, ${g}, 0)`;
}

// ─── Allocation → color (grey = idle, teal = allocated) ──────────────────────

function allocationColor(power_kw: number, maxPower: number): string {
  if (power_kw <= 0) return "#9e9e9e";
  const t = Math.min(power_kw / maxPower, 1);
  const r = Math.round(158 * (1 - t));
  const g = Math.round(158 + (175 - 158) * t);
  const b = Math.round(158 * (1 - t) + 212 * t);
  return `rgb(${r},${g},${b})`;
}

// ─── Legend ───────────────────────────────────────────────────────────────────

function MatrixLegend() {
  return (
    <Box data-testid="matrix-legend" sx={{ mt: 1 }}>
      <Typography variant="caption" color="text.secondary">
        Cell color: grey = idle, teal = allocated (intensity scales with power)
      </Typography>
    </Box>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { plan: Plan | null | undefined };

export function PlanDecisionMatrix({ plan }: Props) {
  const [collapsed, setCollapsed] = useState(false);
  const [selectedStep, setSelectedStep] = useState<PlanStep | null>(null);

  // Derive sorted unique asset IDs from steps; always include "battery" so the
  // row is visible even when all its steps are idle.
  // Exclude uncontrollable assets (pv, base_load) — covered by forecast rows below.
  const assetIds = useMemo(() => {
    if (!plan) return [];
    const UNCONTROLLABLE = new Set(["pv", "base_load"]);
    const ids = new Set(
      plan.steps.map((s) => s.asset_id).filter((id) => !UNCONTROLLABLE.has(id))
    );
    ids.add("battery");
    return [...ids].sort();
  }, [plan]);

  const allSlots: PlanTimeSlot[] = useMemo(() => {
    if (!plan) return [];
    return plan.slots;
  }, [plan]);

  // Map ts → slot index for step lookup
  const slotIndexByTs = useMemo(() => {
    const map = new Map<string, number>();
    allSlots.forEach((s, i) => map.set(s.start, i));
    return map;
  }, [allSlots]);

  // Map (assetId, slotIndex) → step
  const stepMap = useMemo(() => {
    const map = new Map<string, PlanStep>();
    if (!plan) return map;
    for (const step of plan.steps) {
      const slotIdx = slotIndexByTs.get(step.ts);
      if (slotIdx !== undefined) {
        map.set(`${step.asset_id}:${slotIdx}`, step);
      }
    }
    return map;
  }, [plan, slotIndexByTs]);

  // Max allocated power across all slots (for color scaling)
  const maxAllocPower = useMemo(() => {
    if (!plan) return 1;
    let max = 0.01;
    for (const slot of plan.slots) {
      for (const alloc of slot.allocations) {
        if (alloc.power_kw > max) max = alloc.power_kw;
      }
    }
    return max;
  }, [plan]);

  // Map (assetId, slotIndex) → allocated power_kw
  const allocMap = useMemo(() => {
    const map = new Map<string, number>();
    if (!plan) return map;
    for (let i = 0; i < plan.slots.length; i++) {
      for (const alloc of plan.slots[i].allocations) {
        map.set(`${alloc.asset_id}:${i}`, alloc.power_kw);
      }
    }
    return map;
  }, [plan]);

  // Tariff range for color gradient
  const [tariffMin, tariffMax] = useMemo(() => {
    if (!plan || allSlots.length === 0) return [0, 1];
    const vals = allSlots.map((s) => s.import_tariff_eur_kwh);
    return [Math.min(...vals), Math.max(...vals)];
  }, [plan, allSlots]);

  if (!plan) {
    return (
      <Typography data-testid="matrix-empty" color="text.secondary">
        No plan available — waiting for planner to run.
      </Typography>
    );
  }

  const CELL_W = 22;
  const CELL_H = 22;

  return (
    <Box>
      {/* Section header */}
      <Stack direction="row" alignItems="center" spacing={1} sx={{ mb: 1 }}>
        <Typography variant="subtitle1" fontWeight="medium">Decision Matrix</Typography>
        <IconButton
          size="small"
          data-testid="matrix-collapse-btn"
          onClick={() => setCollapsed((v) => !v)}
          title={collapsed ? "Expand decision matrix" : "Collapse decision matrix"}
        >
          {collapsed ? <ExpandMoreIcon fontSize="small" /> : <ExpandLessIcon fontSize="small" />}
        </IconButton>
      </Stack>

      {!collapsed && <Box data-testid="decision-matrix" sx={{ overflowX: "auto" }}>
          {/* Grid wrapper: fixed left asset column + scrollable cell columns */}
          <Box sx={{ display: "flex" }}>
            {/* Left label column */}
            <Box sx={{ flexShrink: 0, width: 72 }}>
              {/* Spacer for time axis row */}
              <Box sx={{ height: 14, mb: 0.5 }} />
              {/* Tariff header label */}
              <Box sx={{ height: CELL_H, display: "flex", alignItems: "center" }}>
                <Typography variant="caption" color="text.secondary" noWrap>Tariff</Typography>
              </Box>
              {/* Asset row labels */}
              {assetIds.map((id) => (
                <Box
                  key={id}
                  data-testid={`matrix-row-${id}`}
                  sx={{ height: CELL_H, display: "flex", alignItems: "center" }}
                >
                  <Typography variant="caption" noWrap title={id}>{id}</Typography>
                </Box>
              ))}
              {/* PV forecast label */}
              <Box
                data-testid="matrix-row-pv"
                sx={{ height: CELL_H, display: "flex", alignItems: "center" }}
              >
                <Typography variant="caption" noWrap>pv</Typography>
              </Box>
              {/* Baseline forecast label */}
              <Box
                data-testid="matrix-row-baseline"
                sx={{ height: CELL_H, display: "flex", alignItems: "center" }}
              >
                <Typography variant="caption" noWrap>base</Typography>
              </Box>
            </Box>

            {/* Cell columns */}
            <Box sx={{ position: "relative", flexShrink: 0 }}>
              {/* Time axis row — label the first slot of each new hour */}
              <Box sx={{ position: "relative", height: 14, mb: 0.5 }}>
                {allSlots.map((slot, ci) => {
                  const d = new Date(slot.start);
                  const prevHour = ci > 0 ? new Date(allSlots[ci - 1].start).getHours() : -1;
                  if (d.getHours() === prevHour) return null;
                  return (
                    <Typography
                      key={ci}
                      variant="caption"
                      sx={{
                        position: "absolute",
                        left: ci * CELL_W,
                        top: 0,
                        fontSize: 9,
                        color: "text.secondary",
                        whiteSpace: "nowrap",
                        lineHeight: 1,
                        userSelect: "none",
                      }}
                    >
                      {String(d.getHours()).padStart(2, "0")}:00
                    </Typography>
                  );
                })}
              </Box>

              {/* Tariff header row */}
              <Box data-testid="matrix-tariff-header" sx={{ display: "flex" }}>
                {allSlots.map((slot, ci) => (
                  <Tooltip
                    key={ci}
                    title={`${new Date(slot.start).toLocaleTimeString()} — ${slot.import_tariff_eur_kwh.toFixed(3)} €/kWh`}
                  >
                    <Box
                      sx={{
                        width: CELL_W,
                        height: CELL_H,
                        bgcolor: tariffColor(slot.import_tariff_eur_kwh, tariffMin, tariffMax),
                        flexShrink: 0,
                        border: "1px solid rgba(0,0,0,0.06)",
                      }}
                    />
                  </Tooltip>
                ))}
              </Box>

              {/* Asset rows — color by allocation */}
              {assetIds.map((assetId) => (
                <Box key={assetId} sx={{ display: "flex" }}>
                  {allSlots.map((slot, ci) => {
                    const step = stepMap.get(`${assetId}:${ci}`);
                    const power_kw = allocMap.get(`${assetId}:${ci}`) ?? 0;
                    const color = allocationColor(power_kw, maxAllocPower);
                    return (
                      <Tooltip
                        key={ci}
                        title={`${assetId} @ ${new Date(slot.start).toLocaleTimeString()}: ${power_kw.toFixed(2)} kW`}
                      >
                        <Box
                          data-testid={`matrix-cell-${assetId}-${ci}`}
                          data-power={power_kw.toFixed(2)}
                          onClick={() => setSelectedStep(step ?? null)}
                          sx={{
                            width: CELL_W,
                            height: CELL_H,
                            bgcolor: color,
                            flexShrink: 0,
                            border: "1px solid rgba(0,0,0,0.08)",
                            cursor: "pointer",
                            userSelect: "none",
                            "&:hover": { outline: "2px solid rgba(0,0,0,0.4)" },
                          }}
                        />
                      </Tooltip>
                    );
                  })}
                </Box>
              ))}

              {/* PV forecast row — grey→yellow heatmap from pv_forecast_kw in slot */}
              {(() => {
                const pvMax = Math.max(...allSlots.map((s) => s.pv_forecast_kw), 0.01);
                return (
                  <Box data-testid="matrix-row-pv-cells" sx={{ display: "flex" }}>
                    {allSlots.map((slot, ci) => {
                      const frac = slot.pv_forecast_kw / pvMax;
                      const r = Math.round(158 + (255 - 158) * frac);
                      const g = Math.round(158 + (235 - 158) * frac);
                      const b = Math.round(158 + ( 59 - 158) * frac);
                      return (
                        <Tooltip
                          key={ci}
                          title={`pv @ ${new Date(slot.start).toLocaleTimeString()}: ${slot.pv_forecast_kw.toFixed(2)} kW forecast`}
                        >
                          <Box
                            data-testid={`matrix-cell-pv-${ci}`}
                            sx={{
                              width: CELL_W,
                              height: CELL_H,
                              bgcolor: `rgb(${r},${g},${b})`,
                              flexShrink: 0,
                              border: "1px solid rgba(0,0,0,0.08)",
                            }}
                          />
                        </Tooltip>
                      );
                    })}
                  </Box>
                );
              })()}

              {/* Baseline load row — grey→orange heatmap from baseline_kw in slot */}
              {(() => {
                const blMax = Math.max(...allSlots.map((s) => s.baseline_kw), 0.01);
                return (
                  <Box data-testid="matrix-row-baseline-cells" sx={{ display: "flex" }}>
                    {allSlots.map((slot, ci) => {
                      const frac = slot.baseline_kw / blMax;
                      // grey (#9e9e9e) → orange (#ff9800)
                      const r = Math.round(158 + (255 - 158) * frac);
                      const g = Math.round(158 + (152 - 158) * frac);
                      const b = Math.round(158 + (  0 - 158) * frac);
                      return (
                        <Tooltip
                          key={ci}
                          title={`base @ ${new Date(slot.start).toLocaleTimeString()}: ${slot.baseline_kw.toFixed(2)} kW forecast`}
                        >
                          <Box
                            data-testid={`matrix-cell-baseline-${ci}`}
                            sx={{
                              width: CELL_W,
                              height: CELL_H,
                              bgcolor: `rgb(${r},${g},${b})`,
                              flexShrink: 0,
                              border: "1px solid rgba(0,0,0,0.08)",
                            }}
                          />
                        </Tooltip>
                      );
                    })}
                  </Box>
                );
              })()}
            </Box>
          </Box>

        <MatrixLegend />
      </Box>}

      {/* Step detail drawer */}
      <Drawer
        anchor="right"
        open={selectedStep !== null}
        onClose={() => setSelectedStep(null)}
        PaperProps={{ sx: { width: 340, p: 2 } }}
      >
        <Box data-testid="matrix-drawer">
          <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ mb: 2 }}>
            <Typography variant="h6">Step Detail</Typography>
            <IconButton onClick={() => setSelectedStep(null)}>
              <ChevronLeftIcon />
            </IconButton>
          </Stack>

          {selectedStep ? (
            <Table size="small">
              <TableBody>
                <TableRow>
                  <TableCell>Time</TableCell>
                  <TableCell>{new Date(selectedStep.ts).toLocaleString()}</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>Asset</TableCell>
                  <TableCell>{selectedStep.asset_id}</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>Setpoint</TableCell>
                  <TableCell>{selectedStep.setpoint_kw} kW</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>Actual</TableCell>
                  <TableCell>{selectedStep.actual_power_kw} kW</TableCell>
                </TableRow>
              </TableBody>
            </Table>
          ) : (
            <Typography color="text.secondary">No step selected</Typography>
          )}
        </Box>
      </Drawer>
    </Box>
  );
}
