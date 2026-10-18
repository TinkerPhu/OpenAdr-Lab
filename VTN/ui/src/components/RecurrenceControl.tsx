import { Stack, TextField, Typography } from "@mui/material";
import {
  describePeriod,
  recurrenceFromDuration,
  type Recurrence,
} from "../utils/recurrence";

export interface RecurrenceControlProps {
  recurrence: Recurrence;
  /** First interval's start and last interval's end: the pattern a repetition shifts by. */
  patternStart: Date;
  patternEnd: Date;
  /** The preview window crosses a clock change in the viewer's zone (see `crossesUtcOffsetChange`). */
  dstNote: boolean;
  onChange: (r: Recurrence) => void;
}

const DEFAULT_SPAN = "P7D";
type Kind = "once" | "span" | "forever";

/** The three things a person can choose; truncated and invalid are readings of "span". */
const kindOf = (r: Recurrence): Kind =>
  r.kind === "once" || r.kind === "forever" ? r.kind : "span";

/**
 * How the series repeats, as `event.duration` (see `utils/recurrence`). Says what a
 * receiver will do with the choice — the period it shifts by, what it drops, and the
 * limit of fixed-period repetition — rather than leaving it in an ISO string.
 */
export function RecurrenceControl(props: RecurrenceControlProps) {
  const { recurrence, patternStart, patternEnd, dstNote, onChange } = props;
  const kind = kindOf(recurrence);
  const repeating = recurrence.kind === "forever" || recurrence.kind === "span";
  const duration = "duration" in recurrence ? recurrence.duration : "";

  function chooseKind(next: Kind) {
    if (next === "once") onChange({ kind: "once" });
    else if (next === "forever") onChange({ kind: "forever" });
    else onChange(recurrenceFromDuration(DEFAULT_SPAN, patternStart, patternEnd));
  }

  return (
    <Stack spacing={1} data-testid="recurrence-control">
      <Stack direction="row" spacing={1} alignItems="center">
        <TextField
          select
          SelectProps={{ native: true }}
          size="small"
          label="Repeats"
          value={kind}
          onChange={(e) => chooseKind(e.target.value as Kind)}
          inputProps={{ "data-testid": "recurrence-kind" }}
        >
          <option value="once">once</option>
          <option value="span">for a span</option>
          <option value="forever">forever</option>
        </TextField>
        {kind === "span" && (
          <TextField
            size="small"
            label="Duration (ISO 8601)"
            value={duration}
            error={recurrence.kind === "invalid"}
            helperText={recurrence.kind === "invalid" ? "Not an ISO 8601 duration" : undefined}
            onChange={(e) =>
              onChange(recurrenceFromDuration(e.target.value, patternStart, patternEnd))
            }
            inputProps={{
              "data-testid": "recurrence-duration",
              "aria-invalid": recurrence.kind === "invalid" ? "true" : undefined,
            }}
          />
        )}
      </Stack>

      {repeating && (
        <Typography variant="body2" data-testid="recurrence-period">
          repeats every {describePeriod(patternEnd.getTime() - patternStart.getTime())}
          {recurrence.kind === "forever" ? ", forever" : `, for ${duration}`}
        </Typography>
      )}
      {recurrence.kind === "truncated" && (
        <Typography variant="body2" color="warning.main" data-testid="recurrence-truncated">
          Truncated to {recurrence.duration}: receivers ignore the intervals after it.
        </Typography>
      )}
      {repeating && dstNote && (
        <Typography variant="body2" color="warning.main" data-testid="recurrence-dst-note">
          The preview crosses a clock change. A repeating series repeats by elapsed time, so it
          does not follow local clock time: after the change it runs an hour off local midnight.
        </Typography>
      )}
    </Stack>
  );
}
