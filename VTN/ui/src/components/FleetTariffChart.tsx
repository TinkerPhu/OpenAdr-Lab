import { useMemo, type ReactElement } from "react";
import { Alert, Typography } from "@mui/material";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { EmptyState } from "@lab/ui/EmptyState";
import { COLOR_IMPORT_TARIFF, COLOR_EXPORT_TARIFF, TARIFF_LINE_STYLE } from "@lab/ui/types";
import type { FleetSignals, SignalBand } from "../api/types";
import {
  tariffGroups,
  tariffRows,
  IMPORT_KEY,
  EXPORT_KEY,
  DRAFT_IMPORT_KEY,
  DRAFT_EXPORT_KEY,
} from "../utils/fleetTariff";
import { fleetChartWindow } from "./fleetChartWindow";
import { TariffLinesChart, TARIFF_AXIS_ID, TARIFF_CHART_HEIGHT } from "./TariffLinesChart";

export { TARIFF_AXIS_ID };

/**
 * The live tariff of the largest group of VENs with an unpublished draft beside it: the signal
 * workbench's chart. The Fleet page, which shows every tariff, uses `FleetTariffGroupsChart`.
 *
 * The workbench edits one tariff, so this draws one live import/export pair (dashed) and the
 * draft (solid); when VENs are on other tariffs it says so and points at the Fleet page.
 */
export function FleetTariffChart({
  signals,
  windowMinutes,
  tickMinutes,
  nowMs,
  aheadMinutes = 0,
  draftBands = [],
  overlay,
}: {
  signals?: FleetSignals;
  windowMinutes: number;
  /** Tick spacing, chosen per window by the page so both charts agree. */
  tickMinutes: number;
  nowMs: number;
  /** Minutes past now the axis runs; see `TariffLinesChart`. */
  aheadMinutes?: number;
  /** An unpublished tariff (the signal workbench's draft), drawn solid beside the live one. */
  draftBands?: SignalBand[];
  /** Painted over the plot (recharts `<Customized>`): the workbench's pointer-editing layer. */
  overlay?: ReactElement;
}) {
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes, aheadMinutes);

  const { rows, live, otherTariffs, knownVens } = useMemo(() => {
    const { groups, knownVens } = tariffGroups(signals, nowMs);
    const live = groups[0];
    return {
      rows: tariffRows(live?.bands ?? [], tMin, tMax, draftBands),
      live,
      otherTariffs: groups.length - 1,
      knownVens,
    };
  }, [signals, nowMs, tMin, tMax, draftBands]);

  const line = (key: string, color: string, solid: boolean): TimeSeriesSeriesSpec => ({
    key,
    axisId: TARIFF_AXIS_ID,
    dataKey: (row) => row.values?.[key] ?? null,
    color,
    // `type` and `strokeWidth` are deliberately unset for the live pair: inheriting
    // TimeSeriesChart's stepAfter/1.5 defaults is what keeps this identical to the VEN
    // Controller tab without restating them here. The draft is solid where the live tariff
    // is dashed, so the two read apart even where they share a colour and a value.
    ...(solid
      ? {
          strokeWidth: 2.5,
          connectNulls: TARIFF_LINE_STYLE.connectNulls,
          formatter: TARIFF_LINE_STYLE.formatter,
        }
      : TARIFF_LINE_STYLE),
  });
  const series = [
    line(IMPORT_KEY, COLOR_IMPORT_TARIFF, false),
    line(EXPORT_KEY, COLOR_EXPORT_TARIFF, false),
    ...(draftBands.length > 0
      ? [
          line(DRAFT_IMPORT_KEY, COLOR_IMPORT_TARIFF, true),
          line(DRAFT_EXPORT_KEY, COLOR_EXPORT_TARIFF, true),
        ]
      : []),
  ];

  if (rows.length === 0) {
    return (
      <EmptyState
        message={`No tariff was announced to the fleet in the last ${windowMinutes} minutes.`}
        testId="fleet-tariff-empty"
        height={TARIFF_CHART_HEIGHT}
      />
    );
  }

  return (
    <>
      {otherTariffs > 0 && (
        <Alert severity="info" data-testid="fleet-tariff-other-groups">
          {otherTariffs} other tariff(s) are in force for other VENs. The dashed line is{" "}
          {live.label}; the Fleet page shows all of them.
        </Alert>
      )}
      <TariffLinesChart
        rows={rows}
        series={series}
        windowMinutes={windowMinutes}
        tickMinutes={tickMinutes}
        nowMs={nowMs}
        aheadMinutes={aheadMinutes}
        overlay={overlay}
        testId="fleet-tariff-chart"
      />
      <Typography variant="caption" color="text.secondary" data-testid="fleet-tariff-source">
        VTN events as of now · {live?.label ?? "no tariff"} · {knownVens} VENs known
      </Typography>
    </>
  );
}
