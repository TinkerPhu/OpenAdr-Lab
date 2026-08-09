import { Box, FormControl, InputLabel, MenuItem, Select } from "@mui/material";
import { SERIES_COLORS, type AssetTimelinePoint } from "../controller/types";
import { minSpanDomain, MIN_POWER_SPAN_KW, formatPowerTick } from "../charts/axisDomain";
import { formatPowerValue } from "../charts/unitFormat";
import { DIAGNOSTIC_CHART_HEIGHT } from "../charts/chartLayout";
import { EmptyState } from "../charts/EmptyState";
import { TimeSeriesChart } from "../charts/TimeSeriesChart";
import type { TimestampedRow } from "../charts/mergeSeries";

interface TimelineSeriesChartProps {
  data: Record<string, AssetTimelinePoint[]>;
  selectedSeries: string;
  onSeriesChange: (series: string) => void;
}

export function TimelineSeriesChart({ data, selectedSeries, onSeriesChange }: TimelineSeriesChartProps) {
  const seriesKeys = Object.keys(data);
  const points: TimestampedRow[] = (data[selectedSeries] ?? []).map((p) => ({
    ts: p.ts,
    values: { power_kw: p.values?.power_kw ?? null },
  }));
  const powerDomain = minSpanDomain(
    points.map((p) => p.values?.power_kw ?? null),
    MIN_POWER_SPAN_KW
  );

  return (
    <Box>
      <FormControl size="small" sx={{ mb: 2, minWidth: 160 }}>
        <InputLabel>Series</InputLabel>
        <Select
          value={seriesKeys.includes(selectedSeries) ? selectedSeries : (seriesKeys[0] ?? "")}
          label="Series"
          onChange={(e) => onSeriesChange(e.target.value)}
          data-testid="timeline-series-select"
        >
          {seriesKeys.map((key) => (
            <MenuItem key={key} value={key}>
              {key}
            </MenuItem>
          ))}
        </Select>
      </FormControl>

      {points.length === 0 ? (
        <EmptyState
          testId="timeline-series-chart"
          message="No data for selected series"
          height={DIAGNOSTIC_CHART_HEIGHT}
        />
      ) : (
        <TimeSeriesChart
          testId="timeline-series-chart"
          data={points}
          xAxisTickFormatter={(v) => new Date(v).toLocaleTimeString()}
          axes={[{ id: "power", domain: powerDomain, tickFormatter: formatPowerTick }]}
          series={[
            {
              key: "power_kw",
              axisId: "power",
              dataKey: (row) => row.values?.power_kw ?? null,
              color: SERIES_COLORS.power,
              type: "monotone",
              formatter: formatPowerValue,
            },
          ]}
          height={DIAGNOSTIC_CHART_HEIGHT}
          margin={{ top: 4, right: 16, left: 0, bottom: 4 }}
          legend={false}
        />
      )}
    </Box>
  );
}
