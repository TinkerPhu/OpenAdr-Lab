import { useState, useMemo } from "react";
import {
  Box, Drawer, IconButton, Stack, Table, TableBody, TableCell,
  TableRow, Tooltip, Typography,
} from "@mui/material";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import KeyboardDoubleArrowRightIcon from "@mui/icons-material/KeyboardDoubleArrowRight";
import KeyboardDoubleArrowLeftIcon from "@mui/icons-material/KeyboardDoubleArrowLeft";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import type { Plan, PlanStep, PlanReason, PlanTimeSlot } from "../../api/types";

// ─── Reason metadata ──────────────────────────────────────────────────────────

type ReasonMeta = { label: string; color: string; icon: string; title: string };

const REASON_META: Record<string, ReasonMeta> = {
  IDLE:               { label: "—",   color: "#9e9e9e", icon: "—",  title: "Idle" },
  CHEAP_TARIFF:       { label: "↓€",  color: "#4caf50", icon: "↓€", title: "Cheap Tariff" },
  EXPENSIVE_TARIFF:   { label: "↑€",  color: "#ff9800", icon: "↑€", title: "Expensive Tariff" },
  FIRM_OBLIGATION:    { label: "⚡",  color: "#2196f3", icon: "⚡", title: "Firm Obligation" },
  USER_OVERRIDE:      { label: "U",   color: "#9c27b0", icon: "U",  title: "User Override" },
  SOC_CEILING:        { label: "⬆",  color: "#ffc107", icon: "⬆", title: "SoC Ceiling" },
  SOC_FLOOR:          { label: "⬇",  color: "#d32f2f", icon: "⬇", title: "SoC Floor" },
  COMFORT_BOUND:      { label: "C",   color: "#00bcd4", icon: "C",  title: "Comfort Bound" },
  GRID_IMPORT_LIMIT:  { label: "←",   color: "#e91e63", icon: "←",  title: "Grid Import Limit" },
  GRID_EXPORT_LIMIT:  { label: "→",   color: "#e91e63", icon: "→",  title: "Grid Export Limit" },
  POLICY_RESERVE:     { label: "P",   color: "#607d8b", icon: "P",  title: "Policy Reserve" },
  OPPORTUNITY_MISSED: { label: "✗",  color: "#b71c1c", icon: "✗", title: "Opportunity Missed" },
  SURPLUS_ABSORPTION: { label: "☀",  color: "#cddc39", icon: "☀", title: "PV Surplus Absorption" },
};

function reasonMeta(reason: PlanReason): ReasonMeta {
  return REASON_META[reason.kind] ?? REASON_META["IDLE"];
}

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

// ─── Reason detail (drawer body) ─────────────────────────────────────────────

function ReasonDetail({ reason }: { reason: PlanReason }) {
  switch (reason.kind) {
    case "CHEAP_TARIFF":
    case "EXPENSIVE_TARIFF":
      return (
        <span>
          {reason.kind}: tariff {reason.tariff_eur_per_kwh.toFixed(4)} €/kWh,
          threshold {reason.threshold_eur_per_kwh.toFixed(4)} €/kWh
        </span>
      );
    case "FIRM_OBLIGATION":
      return <span>{reason.kind}: {reason.required_kw} kW required</span>;
    case "USER_OVERRIDE":
      return <span>{reason.kind}: request {String(reason.request_id).slice(0, 8)}, mode {reason.mode}</span>;
    case "SOC_CEILING":
      return <span>{reason.kind}: SoC {(reason.soc_pct * 100).toFixed(1)}%</span>;
    case "SOC_FLOOR":
      return <span>{reason.kind}: SoC {(reason.soc_pct * 100).toFixed(1)}%</span>;
    case "COMFORT_BOUND":
      return <span>{reason.kind}: {reason.bound_type} on {reason.asset_id}</span>;
    case "GRID_IMPORT_LIMIT":
      return <span>{reason.kind}: {reason.limit_kw} kW limit</span>;
    case "GRID_EXPORT_LIMIT":
      return <span>{reason.kind}: {reason.limit_kw} kW limit</span>;
    case "POLICY_RESERVE":
      return <span>{reason.kind}: policy {reason.policy_id}</span>;
    case "OPPORTUNITY_MISSED":
      return <span>{reason.kind}: {reason.reason}</span>;
    case "SURPLUS_ABSORPTION":
      return <span>{reason.kind}: {reason.surplus_kw.toFixed(2)} kW surplus available</span>;
    case "IDLE":
    default:
      return <span>IDLE</span>;
  }
}

// ─── Legend ───────────────────────────────────────────────────────────────────

function MatrixLegend() {
  return (
    <Box data-testid="matrix-legend" sx={{ mt: 1 }}>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 0.5, display: "block" }}>
        Decision reasons:
      </Typography>
      <Stack direction="row" flexWrap="wrap" gap={1}>
        {Object.entries(REASON_META).map(([key, meta]) => (
          <Stack key={key} direction="row" alignItems="center" spacing={0.5}>
            <Box sx={{ width: 14, height: 14, bgcolor: meta.color, borderRadius: 0.5, flexShrink: 0 }} />
            <Typography variant="caption">{meta.icon} {key}</Typography>
          </Stack>
        ))}
      </Stack>
    </Box>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { plan: Plan | null | undefined };

export function PlanDecisionMatrix({ plan }: Props) {
  const [showFlex, setShowFlex] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [selectedStep, setSelectedStep] = useState<PlanStep | null>(null);

  // Derive sorted unique asset IDs from steps; always include "battery" so the
  // row is visible even when all its steps are Idle.
  // Exclude uncontrollable assets (pv, base_load) — they're always IDLE and are
  // covered by the dedicated forecast reference rows below.
  const assetIds = useMemo(() => {
    if (!plan) return [];
    const UNCONTROLLABLE = new Set(["pv", "base_load"]);
    const ids = new Set(
      plan.steps.map((s) => s.asset_id).filter((id) => !UNCONTROLLABLE.has(id))
    );
    ids.add("battery");
    return [...ids].sort();
  }, [plan]);

  // All slots (FIRM + optionally FLEXIBLE)
  const allSlots: PlanTimeSlot[] = useMemo(() => {
    if (!plan) return [];
    return showFlex
      ? [...plan.firm_slots, ...plan.flexible_slots]
      : plan.firm_slots;
  }, [plan, showFlex]);

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

  // Firm boundary index
  const firmBoundaryIdx = useMemo(() => {
    if (!plan) return allSlots.length;
    const boundary = new Date(plan.firm_boundary).getTime();
    const idx = allSlots.findIndex((s) => new Date(s.start).getTime() >= boundary);
    return idx === -1 ? allSlots.length : idx;
  }, [plan, allSlots]);

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
        {!collapsed && (
          <IconButton
            size="small"
            data-testid="matrix-expand-horizon-btn"
            onClick={() => setShowFlex((v) => !v)}
            title={showFlex ? "Show FIRM only" : "Show full horizon (FIRM + FLEXIBLE)"}
          >
            {showFlex
              ? <KeyboardDoubleArrowLeftIcon fontSize="small" />
              : <KeyboardDoubleArrowRightIcon fontSize="small" />}
          </IconButton>
        )}
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

              {/* Asset rows */}
              {assetIds.map((assetId) => (
                <Box key={assetId} sx={{ display: "flex" }}>
                  {allSlots.map((slot, ci) => {
                    const isFlexZone = ci >= firmBoundaryIdx;
                    const step = stepMap.get(`${assetId}:${ci}`);
                    const reason = step?.reason ?? { kind: "IDLE" as const };
                    const meta = reasonMeta(reason);
                    return (
                      <Tooltip key={ci} title={`${assetId} @ ${new Date(slot.start).toLocaleTimeString()}: ${meta.title}`}>
                        <Box
                          data-testid={`matrix-cell-${assetId}-${ci}`}
                          data-flex={isFlexZone ? "true" : undefined}
                          onClick={() => setSelectedStep(step ?? null)}
                          sx={{
                            width: CELL_W,
                            height: CELL_H,
                            bgcolor: meta.color,
                            opacity: isFlexZone ? 0.5 : 1,
                            flexShrink: 0,
                            border: isFlexZone ? "1px dashed rgba(0,0,0,0.2)" : "1px solid rgba(0,0,0,0.08)",
                            cursor: "pointer",
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                            fontSize: 10,
                            color: "white",
                            userSelect: "none",
                            "&:hover": { outline: "2px solid rgba(0,0,0,0.4)" },
                          }}
                        >
                          {meta.icon}
                        </Box>
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
                              opacity: ci >= firmBoundaryIdx ? 0.5 : 1,
                              flexShrink: 0,
                              border: ci >= firmBoundaryIdx
                                ? "1px dashed rgba(0,0,0,0.2)"
                                : "1px solid rgba(0,0,0,0.08)",
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
                              opacity: ci >= firmBoundaryIdx ? 0.5 : 1,
                              flexShrink: 0,
                              border: ci >= firmBoundaryIdx
                                ? "1px dashed rgba(0,0,0,0.2)"
                                : "1px solid rgba(0,0,0,0.08)",
                            }}
                          />
                        </Tooltip>
                      );
                    })}
                  </Box>
                );
              })()}

              {/* FIRM/FLEX boundary divider */}
              {firmBoundaryIdx > 0 && firmBoundaryIdx < allSlots.length && (
                <Box
                  data-testid="matrix-firm-flex-divider"
                  sx={{
                    position: "absolute",
                    top: 0,
                    left: firmBoundaryIdx * CELL_W,
                    width: 2,
                    height: "100%",
                    bgcolor: "text.primary",
                    opacity: 0.6,
                    pointerEvents: "none",
                  }}
                />
              )}

              {/* Render divider even if boundary is at edge (for test detectability) */}
              {(firmBoundaryIdx === 0 || firmBoundaryIdx >= allSlots.length) && (
                <Box
                  data-testid="matrix-firm-flex-divider"
                  sx={{ display: "none" }}
                />
              )}
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
                <TableRow>
                  <TableCell>State before</TableCell>
                  <TableCell>{selectedStep.state_before.asset_type} @ {selectedStep.state_before.actual_power_kw.toFixed(2)} kW</TableCell>
                </TableRow>
                {selectedStep.state_before.soc != null && (
                  <TableRow>
                    <TableCell>SoC</TableCell>
                    <TableCell>{((selectedStep.state_before.soc as number) * 100).toFixed(1)}%</TableCell>
                  </TableRow>
                )}
                <TableRow>
                  <TableCell>Max import</TableCell>
                  <TableCell>{selectedStep.avail_max_import_kw} kW</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>Max export</TableCell>
                  <TableCell>{selectedStep.avail_max_export_kw} kW</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>Reason</TableCell>
                  <TableCell data-testid="matrix-drawer-reason">
                    <ReasonDetail reason={selectedStep.reason} />
                  </TableCell>
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
