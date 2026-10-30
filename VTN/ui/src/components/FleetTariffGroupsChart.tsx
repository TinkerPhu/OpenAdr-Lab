import { useMemo } from "react";
import { Alert, Stack, Typography } from "@mui/material";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { EmptyState } from "@lab/ui/EmptyState";
import { TARIFF_LINE_STYLE } from "@lab/ui/types";
import type { FleetSignals } from "../api/types";
import { tariffGroups, tariffLineRows, type TariffGroup } from "../utils/fleetTariff";
import { venColor } from "../utils/venColor";
import { fleetChartWindow } from "./fleetChartWindow";
import { TariffLinesChart, TARIFF_AXIS_ID, TARIFF_CHART_HEIGHT } from "./TariffLinesChart";

/** More lines than this stop being readable; the rest are named in a note. */
export const MAX_TARIFF_GROUPS = 8;

/** A group keeps its colour while its VENs do, across refetches and pages. */
const groupColor = (group: TariffGroup) => venColor(group.vens.join(","));

/**
 * Every tariff the fleet is under, one line each: import above, export below, a group in one
 * colour in both. Each line is labelled with the short name of its event and the number of VENs;
 * hovering the label lists them. Several tariffs are normal here — the lab sends targeted events
 * to test staggering — so a split is shown, never reported as disagreement.
 *
 * What it draws is the VTN's events as they stand now, resolved per VEN by the BFF with the
 * ranking the VEN uses. It does not know what a VEN adopted at a past time.
 */
export function FleetTariffGroupsChart({
  signals,
  windowMinutes,
  tickMinutes,
  nowMs,
}: {
  signals?: FleetSignals;
  windowMinutes: number;
  tickMinutes: number;
  nowMs: number;
}) {
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes, 0);
  const { drawn, hidden, knownVens, unpricedVens, importRows, exportRows } = useMemo(() => {
    const { groups, knownVens, unpricedVens } = tariffGroups(signals, nowMs);
    const drawn = groups.slice(0, MAX_TARIFF_GROUPS);
    const lines = (payloadType: string) =>
      drawn.map((g) => ({
        key: g.label,
        bands: g.bands.filter((b) => b.payloadType === payloadType),
      }));
    return {
      drawn,
      hidden: groups.slice(MAX_TARIFF_GROUPS),
      knownVens,
      unpricedVens,
      importRows: tariffLineRows(lines("PRICE"), tMin, tMax),
      exportRows: tariffLineRows(lines("EXPORT_PRICE"), tMin, tMax),
    };
  }, [signals, nowMs, tMin, tMax]);

  if (importRows.length === 0 && exportRows.length === 0) {
    return (
      <EmptyState
        message={`No tariff was announced to the fleet in the last ${windowMinutes} minutes.`}
        testId="fleet-tariff-empty"
        height={TARIFF_CHART_HEIGHT}
      />
    );
  }

  const seriesOf = (group: TariffGroup): TimeSeriesSeriesSpec => ({
    key: group.label,
    axisId: TARIFF_AXIS_ID,
    dataKey: (row) => row.values?.[group.label] ?? null,
    color: groupColor(group),
    ...TARIFF_LINE_STYLE,
  });
  const chart = (rows: typeof importRows, testId: string) => (
    <TariffLinesChart
      rows={rows}
      series={drawn.map(seriesOf)}
      windowMinutes={windowMinutes}
      tickMinutes={tickMinutes}
      nowMs={nowMs}
      testId={testId}
    />
  );

  return (
    <>
      <Typography variant="caption" color="text.secondary">
        Import
      </Typography>
      {chart(importRows, "fleet-tariff-import-chart")}
      <Typography variant="caption" color="text.secondary">
        Export
      </Typography>
      {chart(exportRows, "fleet-tariff-export-chart")}
      {/* The hover is the VEN list: the legend inside the chart cannot carry it. */}
      <Stack direction="row" flexWrap="wrap" gap={1.5} data-testid="fleet-tariff-groups">
        {drawn.map((g) => (
          <Typography
            key={g.key}
            variant="caption"
            title={g.hover}
            data-testid="fleet-tariff-group"
            sx={{ borderLeft: 4, borderColor: groupColor(g), pl: 0.75 }}
          >
            {g.label}
          </Typography>
        ))}
      </Stack>
      {hidden.length > 0 && (
        <Alert severity="info" data-testid="fleet-tariff-more">
          {hidden.length} more tariff(s) not drawn: {hidden.map((g) => g.hover).join("; ")}
        </Alert>
      )}
      <Typography variant="caption" color="text.secondary" data-testid="fleet-tariff-source">
        VTN events as of now, resolved per VEN · {knownVens - unpricedVens.length} of {knownVens}{" "}
        VENs priced
      </Typography>
    </>
  );
}
