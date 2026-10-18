import { useState } from "react";
import { Button, Stack, TextField } from "@mui/material";
import type { SignalDraft } from "../utils/signalDraft";
import { SIGNAL_OPERATORS, applyOperator } from "../utils/signalOperators";
import type { SignalRegistry } from "../utils/signalRegistry";

export interface OperatorToolbarProps {
  draft: SignalDraft;
  registry: SignalRegistry;
  onChange: (draft: SignalDraft) => void;
}

const defaultsOf = (id: string) =>
  Object.fromEntries(SIGNAL_OPERATORS[id].params.map((p) => [p.key, String(p.default)]));

/**
 * Whole-series edits (scale, shift, cap, paint a range, templates), rendered from
 * `SIGNAL_OPERATORS`: a new operator is a row in that table, not a change here.
 */
export function OperatorToolbar({ draft, registry, onChange }: OperatorToolbarProps) {
  const ids = Object.keys(SIGNAL_OPERATORS);
  const [id, setId] = useState(ids[0]);
  const [type, setType] = useState(draft.types[0]);
  const [values, setValues] = useState<Record<string, string>>(() => defaultsOf(ids[0]));

  const op = SIGNAL_OPERATORS[id];
  const signal = draft.types.includes(type) ? type : draft.types[0];
  const parsed = Object.fromEntries(
    op.params.map((p) => [p.key, values[p.key]?.trim() === "" ? NaN : Number(values[p.key])]),
  );
  const valid = op.params.every((p) => Number.isFinite(parsed[p.key]));

  function chooseOperator(next: string) {
    setId(next);
    setValues(defaultsOf(next));
  }

  return (
    <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap data-testid="operator-toolbar">
      <TextField
        select
        SelectProps={{ native: true }}
        size="small"
        label="Operator"
        value={id}
        onChange={(e) => chooseOperator(e.target.value)}
        inputProps={{ "data-testid": "operator-select" }}
      >
        {ids.map((k) => (
          <option key={k} value={k}>
            {SIGNAL_OPERATORS[k].label}
          </option>
        ))}
      </TextField>
      {draft.types.length > 1 && (
        <TextField
          select
          SelectProps={{ native: true }}
          size="small"
          label="Signal"
          value={signal}
          onChange={(e) => setType(e.target.value)}
          inputProps={{ "data-testid": "operator-signal" }}
        >
          {draft.types.map((t) => (
            <option key={t} value={t}>
              {registry[t]?.label ?? t}
            </option>
          ))}
        </TextField>
      )}
      {op.params.map((p) => (
        <TextField
          key={`${id}-${p.key}`}
          size="small"
          label={p.label}
          value={values[p.key] ?? ""}
          onChange={(e) => setValues({ ...values, [p.key]: e.target.value })}
          error={!Number.isFinite(parsed[p.key])}
          inputProps={{ "data-testid": `op-param-${p.key}`, inputMode: "decimal" }}
          sx={{ width: 120 }}
        />
      ))}
      <Button
        size="small"
        variant="outlined"
        disabled={!valid}
        onClick={() => valid && onChange(applyOperator(id, draft, registry, signal, parsed))}
        data-testid="operator-apply"
      >
        Apply
      </Button>
    </Stack>
  );
}
