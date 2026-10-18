import { useState, type PointerEvent as ReactPointerEvent } from "react";
import type { SignalBand } from "../api/types";
import { moveBoundary, patternIndexOfBand, snapValue } from "../utils/chartEdit";
import { setValue, type SignalDraft } from "../utils/signalDraft";
import type { SignalRegistry } from "../utils/signalRegistry";

/** What recharts hands a `<Customized>` component; only the scales and plot area are used. */
type Scale = ((v: number) => number) & { invert: (p: number) => number };
export interface ChartGeometry {
  xAxisMap: Record<string, { scale: Scale }>;
  yAxisMap: Record<string, { scale: Scale }>;
  offset: { left: number; top: number; width: number; height: number };
}

/** The geometry is injected by recharts when it renders the layer, so it is absent from the caller's props. */
export interface DraftEditLayerProps extends Partial<ChartGeometry> {
  /** The y axis the draft is drawn against. */
  axisId: string;
  draft: SignalDraft;
  registry: SignalRegistry;
  /** The previewed draft, as the BFF resolved it: one band per drawn interval, repetitions included. */
  bands: SignalBand[];
  onChange: (draft: SignalDraft) => void;
}

/** A pointer position in the chart's own svg coordinates, which is what its scales speak. */
function svgPoint(target: Element, clientX: number, clientY: number) {
  const svg = (target as SVGElement).ownerSVGElement ?? (target as SVGSVGElement);
  const ctm = svg.getScreenCTM?.();
  if (ctm && svg.createSVGPoint) {
    const p = svg.createSVGPoint();
    p.x = clientX;
    p.y = clientY;
    const q = p.matrixTransform(ctm.inverse());
    return { x: q.x, y: q.y };
  }
  const r = svg.getBoundingClientRect();
  return { x: clientX - r.left, y: clientY - r.top };
}

const decimalsOf = (step: number) => (String(step).split(".")[1] ?? "").length;

/**
 * Pointer editing of the draft, drawn over the chart's plot area.
 *
 * Every band of the previewed draft gets a handle to drag its value, and every row boundary one to
 * drag its time. Neither edits the band it sits on: each is mapped back to the row of the draft's
 * pattern it is a drawing of (`patternIndexOfBand`), so a drag on the third repetition of a daily
 * tariff changes the day, and all repetitions move with it. The edit itself is `setValue` /
 * `moveBoundary`, the same functions the grid and the operators use.
 */
export function DraftEditLayer(props: DraftEditLayerProps) {
  const { xAxisMap, yAxisMap, offset, axisId, draft, registry, bands, onChange } = props;
  const [active, setActive] = useState<{ id: string; label: string; x: number; y: number } | null>(
    null,
  );

  const xScale = xAxisMap && Object.values(xAxisMap)[0]?.scale;
  const yScale = yAxisMap?.[axisId]?.scale;
  if (!xScale || !yScale || !offset || draft.intervals.length === 0) return null;

  const period =
    Date.parse(draft.intervals[draft.intervals.length - 1].end) - Date.parse(draft.intervals[0].start);
  const top = offset.top;
  const bottom = offset.top + offset.height;
  const firstType = draft.types[0];

  const begin = (id: string) => (e: ReactPointerEvent<SVGElement>) => {
    e.currentTarget.setPointerCapture?.(e.pointerId);
    setActive({ id, label: "", x: 0, y: 0 });
  };
  const end = () => setActive(null);

  return (
    <g data-testid="draft-edit-layer">
      {bands.map((band, i) => {
        const entry = registry[band.payloadType];
        const fromMs = Date.parse(band.from);
        const row = patternIndexOfBand(draft, fromMs);
        if (band.value === null || !entry || !draft.types.includes(band.payloadType) || row === null) {
          return null;
        }
        const x1 = xScale(fromMs);
        const x2 = xScale(Date.parse(band.to));
        const y = yScale(band.value);
        const id = `value-${i}`;
        const rowStart = Date.parse(draft.intervals[row].start);
        const shift = fromMs - rowStart;
        // An edge handle only where this drawing starts exactly where its row does: a band cut at
        // the window's left edge starts mid-row and has no edge of its own there.
        const startsRow = row > 0 && period > 0 && shift % period === 0 && band.payloadType === firstType;

        return (
          <g key={i}>
            <line
              data-testid={`handle-value-${i}`}
              x1={x1}
              x2={x2}
              y1={y}
              y2={y}
              stroke={active?.id === id ? "#1976d2" : "transparent"}
              strokeWidth={active?.id === id ? 3 : 14}
              style={{ cursor: "ns-resize", touchAction: "none" }}
              onPointerDown={begin(id)}
              onPointerMove={(e) => {
                if (active?.id !== id) return;
                const p = svgPoint(e.currentTarget, e.clientX, e.clientY);
                const next = snapValue(yScale.invert(p.y), entry);
                setActive({
                  id,
                  label: `${next.toFixed(decimalsOf(entry.step))} ${entry.unitLabel}`,
                  x: p.x,
                  y: p.y,
                });
                onChange(setValue(draft, row, band.payloadType, next));
              }}
              onPointerUp={end}
              onPointerCancel={end}
            />
            {startsRow && (
              <line
                data-testid={`handle-edge-${i}`}
                x1={x1}
                x2={x1}
                y1={top}
                y2={bottom}
                stroke={active?.id === `edge-${i}` ? "#1976d2" : "transparent"}
                strokeWidth={active?.id === `edge-${i}` ? 2 : 10}
                style={{ cursor: "ew-resize", touchAction: "none" }}
                onPointerDown={begin(`edge-${i}`)}
                onPointerMove={(e) => {
                  if (active?.id !== `edge-${i}`) return;
                  const p = svgPoint(e.currentTarget, e.clientX, e.clientY);
                  // Back onto the pattern: undo this repetition's offset from the first.
                  const patternMs = xScale.invert(p.x) - shift;
                  setActive({ id: `edge-${i}`, label: new Date(patternMs).toISOString().slice(11, 16) + " UTC", x: p.x, y: p.y });
                  const next = moveBoundary(draft, row - 1, patternMs, entry.intervalMinutes);
                  if (next !== draft) onChange(next);
                }}
                onPointerUp={end}
                onPointerCancel={end}
              />
            )}
          </g>
        );
      })}
      {active && active.label && (
        <text
          data-testid="drag-readout"
          x={active.x + 8}
          y={active.y - 8}
          fontSize={12}
          fill="#1976d2"
          style={{ pointerEvents: "none" }}
        >
          {active.label}
        </text>
      )}
    </g>
  );
}
