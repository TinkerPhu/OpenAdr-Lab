import { Box, FormControl, InputLabel, MenuItem, Select, Typography } from "@mui/material";
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from "recharts";
import type { AssetTimelinePoint } from "../controller-v2/types";
import { CHART_COLORS } from "./colors";

interface TimelineSeriesChartProps {
  data: Record<string, AssetTimelinePoint[]>;
  selectedSeries: string;
  onSeriesChange: (series: string) => void;
}

export function TimelineSeriesChart({ data, selectedSeries, onSeriesChange }: TimelineSeriesChartProps) {
  const seriesKeys = Object.keys(data);
  const points = (data[selectedSeries] ?? []).map((p) => ({
    ts: p.ts,
    power_kw: p.values?.power_kw ?? null,
  }));

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
        <Typography variant="body2" color="text.secondary" data-testid="timeline-series-chart">
          No data for selected series
        </Typography>
      ) : (
        <div data-testid="timeline-series-chart">
          <ResponsiveContainer width="100%" height={260}>
            <LineChart data={points} margin={{ top: 4, right: 16, left: 0, bottom: 4 }}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis
                dataKey="ts"
                scale="time"
                type="number"
                domain={["auto", "auto"]}
                tickFormatter={(v: number) => new Date(v).toLocaleTimeString()}
              />
              <YAxis unit=" kW" />
              <Tooltip
                labelFormatter={(v: number) => new Date(v).toLocaleString()}
                formatter={(v: number) => [`${v.toFixed(3)} kW`, "power_kw"]}
              />
              <Line
                type="monotone"
                dataKey="power_kw"
                stroke={CHART_COLORS[3]}
                dot={false}
                connectNulls={false}
                name="power_kw"
                isAnimationActive={false}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      )}
    </Box>
  );
}
