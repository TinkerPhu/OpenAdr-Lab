import { useState } from "react";
import {
  Button,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  TextField,
} from "@mui/material";
import {
  fillRange,
  pasteRows,
  setValue,
  type DraftIssue,
  type SignalDraft,
} from "../utils/signalDraft";
import type { SignalRegistry } from "../utils/signalRegistry";

/** "2026-10-18T06:00:00.000Z" → "2026-10-18 06:00"; the wire is UTC and so is the grid. */
const utcMinute = (iso: string) => iso.slice(0, 16).replace("T", " ");

const fmt = (v: number | null) => (v === null ? "" : String(v));
function parse(text: string): number | null {
  const t = text.trim();
  if (t === "") return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/**
 * One editable number. It keeps its own text so that "0." survives being parsed to 0 on
 * the way to the draft; the draft's value wins again as soon as it differs from the text.
 */
function ValueCell(props: {
  testId: string;
  value: number | null;
  invalid: boolean;
  step: number;
  onValue: (v: number | null) => void;
  onPasteBlock: (text: string) => void;
}) {
  const [text, setText] = useState(fmt(props.value));
  const [seen, setSeen] = useState(props.value);
  // Adjusted during render (react.dev "you might not need an effect"): only a change of the
  // draft's value re-syncs the text, so typing "0." is not rewritten to "0" on the way through.
  if (props.value !== seen) {
    setSeen(props.value);
    if (parse(text) !== props.value) setText(fmt(props.value));
  }

  return (
    <TextField
      size="small"
      variant="standard"
      value={text}
      error={props.invalid}
      onChange={(e) => {
        setText(e.target.value);
        props.onValue(parse(e.target.value));
      }}
      onPaste={(e) => {
        const pasted = e.clipboardData.getData("text");
        // A single number pastes as typed; a block is spread over rows and columns.
        if (/[\n\t,;]/.test(pasted.trim())) {
          e.preventDefault();
          props.onPasteBlock(pasted);
        }
      }}
      inputProps={{
        "data-testid": props.testId,
        "aria-invalid": props.invalid ? "true" : undefined,
        inputMode: "decimal",
        step: props.step,
      }}
    />
  );
}

export interface TariffGridProps {
  draft: SignalDraft;
  registry: SignalRegistry;
  issues: DraftIssue[];
  onChange: (draft: SignalDraft) => void;
}

/**
 * The grid input mode of the signal workbench: one row per interval, one column per
 * signal. Every edit is a call into `signalDraft`, so the chart and the operators that
 * come next edit the same state.
 */
export function TariffGrid({ draft, registry, issues, onChange }: TariffGridProps) {
  const [selection, setSelection] = useState<{ anchor: number; head: number } | null>(null);
  const [selectionType, setSelectionType] = useState(draft.types[0]);
  const [selectionValue, setSelectionValue] = useState("");

  const first = selection ? Math.min(selection.anchor, selection.head) : -1;
  const last = selection ? Math.max(selection.anchor, selection.head) : -1;
  const type = draft.types.includes(selectionType) ? selectionType : draft.types[0];

  const cellInvalid = (row: number, t: string) =>
    issues.some((i) => i.row === row && i.type === t);
  const rowInvalid = (row: number) =>
    issues.some((i) => i.row === row && i.type === undefined);

  function select(row: number, extend: boolean) {
    setSelection((s) => (extend && s ? { ...s, head: row } : { anchor: row, head: row }));
  }

  return (
    <Stack spacing={1}>
      <Stack direction="row" spacing={1} alignItems="center" data-testid="selection-toolbar">
        <TextField
          select
          SelectProps={{ native: true }}
          size="small"
          label="Signal"
          value={type}
          onChange={(e) => setSelectionType(e.target.value)}
          disabled={draft.types.length < 2}
        >
          {draft.types.map((t) => (
            <option key={t} value={t}>
              {registry[t]?.label ?? t}
            </option>
          ))}
        </TextField>
        <TextField
          size="small"
          label="Value"
          value={selectionValue}
          onChange={(e) => setSelectionValue(e.target.value)}
          inputProps={{ "data-testid": "selection-value" }}
          sx={{ width: 110 }}
        />
        <Button
          size="small"
          variant="outlined"
          disabled={!selection}
          data-testid="selection-set"
          onClick={() => onChange(fillRange(draft, type, first, last, parse(selectionValue)))}
        >
          Set selection
        </Button>
        <Button
          size="small"
          variant="outlined"
          disabled={!selection}
          data-testid="selection-fill-down"
          onClick={() =>
            onChange(fillRange(draft, type, first, last, draft.intervals[first].values[type]))
          }
        >
          Fill down
        </Button>
      </Stack>

      <Table size="small" data-testid="tariff-grid">
        <TableHead>
          <TableRow>
            <TableCell>#</TableCell>
            <TableCell>Start (UTC)</TableCell>
            <TableCell>End (UTC)</TableCell>
            {draft.types.map((t) => (
              <TableCell key={t}>
                {registry[t]?.label ?? t} ({registry[t]?.unitLabel})
              </TableCell>
            ))}
          </TableRow>
        </TableHead>
        <TableBody>
          {draft.intervals.map((row, i) => (
            <TableRow key={i} selected={i >= first && i <= last}>
              <TableCell
                data-testid={`row-select-${i}`}
                onClick={(e) => select(i, e.shiftKey)}
                sx={{ cursor: "pointer", userSelect: "none" }}
              >
                {i + 1}
              </TableCell>
              <TableCell data-testid={rowInvalid(i) ? "cell-invalid" : undefined}>
                {utcMinute(row.start)}
              </TableCell>
              <TableCell>{utcMinute(row.end)}</TableCell>
              {draft.types.map((t, col) => (
                <TableCell key={t} data-testid={cellInvalid(i, t) ? "cell-invalid" : undefined}>
                  <ValueCell
                    testId={`cell-${t}-${i}`}
                    value={row.values[t]}
                    invalid={cellInvalid(i, t)}
                    step={registry[t]?.step ?? 0.01}
                    onValue={(v) => onChange(setValue(draft, i, t, v))}
                    onPasteBlock={(text) => onChange(pasteRows(draft, i, col, text))}
                  />
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </Stack>
  );
}
