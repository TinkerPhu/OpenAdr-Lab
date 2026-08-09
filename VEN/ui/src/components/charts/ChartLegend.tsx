export interface ChartLegendEntry {
  key: string;
  label: string;
  color: string;
}

interface ChartLegendProps {
  entries: ChartLegendEntry[];
  isHidden: (key: string) => boolean;
  toggle: (key: string) => void;
  /** When false, renders the same row layout with no checkbox — visually a plain
   * legend. Used by StackedTimeSeriesChart, whose one-entry-per-asset grouping applies
   * unconditionally even when the toggle itself isn't enabled. */
  interactive: boolean;
}

/**
 * Shared legend row for TimeSeriesChart/StackedTimeSeriesChart: one `[checkbox] [color
 * swatch] label` entry per series. Rendered via recharts' `<Legend content={...} />`,
 * ignoring recharts' own auto-generated payload — the caller already knows its own
 * series identities/colors, so this renders directly from `entries` instead of
 * re-deriving them from recharts' internal series bookkeeping.
 */
export function ChartLegend({ entries, isHidden, toggle, interactive }: ChartLegendProps) {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        justifyContent: "center",
        gap: 8,
        fontSize: 10,
        paddingTop: 2,
      }}
    >
      {entries.map((entry) => {
        const hidden = isHidden(entry.key);
        return (
          <label
            key={entry.key}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 3,
              cursor: interactive ? "pointer" : "default",
              opacity: hidden ? 0.5 : 1,
            }}
          >
            {interactive && (
              <input
                type="checkbox"
                checked={!hidden}
                onChange={() => toggle(entry.key)}
                data-testid={`legend-toggle-${entry.key}`}
                style={{ width: 10, height: 10, margin: 0, accentColor: entry.color, cursor: "pointer" }}
              />
            )}
            <span
              style={{
                display: "inline-block",
                width: 10,
                height: 10,
                borderRadius: 2,
                background: entry.color,
                flexShrink: 0,
              }}
            />
            <span style={{ color: entry.color }}>{entry.label}</span>
          </label>
        );
      })}
    </div>
  );
}
