import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Paper, Stack, TextField, Typography } from "@mui/material";
import {
  useEventPattern,
  useEvents,
  useFleetHistory,
  useFleetPower,
  useFleetSignals,
  useSignalPreview,
  useSignalRegistry,
} from "../api/hooks";
import type { PatternInterval, VtnEvent } from "../api/types";
import { fleetChartWindow } from "../components/fleetChartWindow";
import { FleetPowerChart } from "../components/FleetPowerChart";
import { FleetTariffChart } from "../components/FleetTariffChart";
import { PublishDialog } from "../components/PublishDialog";
import { RecurrenceControl } from "../components/RecurrenceControl";
import { TariffGrid } from "../components/TariffGrid";
import { crossesUtcOffsetChange } from "../utils/recurrence";
import {
  draftFromPattern,
  eventPayloadTypes,
  toEventInput,
  validateDraft,
  type SignalDraft,
} from "../utils/signalDraft";

/** Minutes of history and of look-ahead on both charts: the day the tariff is about to shape. */
const BACK_MINUTES = 360;
const AHEAD_MINUTES = 1080;
const TICK_MINUTES = 180;
const POWER_STEP_SECONDS = 300;

/** The clock, refreshed each minute, so a query keyed on it does not refetch on every render. */
function useMinuteClock(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 60_000) * 60_000);
  useEffect(() => {
    const id = setInterval(() => setNow(Math.floor(Date.now() / 60_000) * 60_000), 60_000);
    return () => clearInterval(id);
  }, []);
  return now;
}

/**
 * The signal workbench for tariffs: pick the event whose tariff you want to change, edit the
 * draft in the grid, watch it against the live tariff and the fleet's power on one time axis,
 * and publish it as a new event.
 *
 * The page owns no signal knowledge. Units come from the registry (`useSignalRegistry`), timing
 * and repetition from the BFF's preview, validation and the event body from `signalDraft`.
 */
export function TariffPage() {
  const registryQuery = useSignalRegistry();
  const eventsQuery = useEvents();
  const loadPattern = useEventPattern();
  const nowMs = useMinuteClock();

  const [loaded, setLoaded] = useState<{ event: VtnEvent; pattern: PatternInterval[] } | null>(null);
  const [draft, setDraft] = useState<SignalDraft | null>(null);
  const [publishOpen, setPublishOpen] = useState(false);
  const [publishedAt, setPublishedAt] = useState<number | null>(null);

  const registry = registryQuery.data;
  const events = useMemo(() => eventsQuery.data ?? [], [eventsQuery.data]);

  const editable = useMemo(
    () =>
      registry
        ? events.filter((e) => {
            const types = eventPayloadTypes(e.intervals);
            return types.length > 0 && types.every((t) => t in registry);
          })
        : [],
    [events, registry],
  );

  const { tMin, tMax } = fleetChartWindow(nowMs, BACK_MINUTES, AHEAD_MINUTES);
  const issues = useMemo(
    () => (draft && registry ? validateDraft(draft, registry) : []),
    [draft, registry],
  );

  // The preview is asked for only while the draft is publishable: a half-typed cell has no
  // event to expand, and the last good preview stays on screen meanwhile.
  const previewEvent = useMemo(
    () => (draft && registry && issues.length === 0 ? toEventInput(draft, registry, []) : null),
    [draft, registry, issues],
  );
  const preview = useSignalPreview(
    previewEvent,
    new Date(tMin).toISOString(),
    new Date(tMax).toISOString(),
  );
  const draftBands = useMemo(
    () => (preview.data?.bands ?? []).filter((b) => b.value !== null),
    [preview.data],
  );

  const signals = useFleetSignals(BACK_MINUTES, AHEAD_MINUTES);
  const history = useFleetHistory(BACK_MINUTES, POWER_STEP_SECONDS);
  const live = useFleetPower();

  const dstNote = useMemo(() => crossesUtcOffsetChange(tMin, tMax), [tMin, tMax]);

  function selectEvent(id: string) {
    const event = editable.find((e) => e.id === id);
    if (!event || !registry) return;
    loadPattern.mutate(event, {
      onSuccess: (pattern) => {
        setLoaded({ event, pattern });
        setDraft(draftFromPattern(event, pattern, registry));
        setPublishedAt(null);
      },
    });
  }

  const replacedPriorities = useMemo(
    () =>
      draft
        ? events
            .filter((e) => eventPayloadTypes(e.intervals).some((t) => draft.types.includes(t)))
            .map((e) => e.priority)
        : [],
    [draft, events],
  );

  const patternStart = draft?.intervals[0] ? new Date(draft.intervals[0].start) : null;
  const patternEnd = draft?.intervals.length
    ? new Date(draft.intervals[draft.intervals.length - 1].end)
    : null;

  return (
    <Stack spacing={2}>
      <Typography variant="h5" data-testid="tariff-heading">
        Tariff editor
      </Typography>

      {registryQuery.isError && (
        <Alert severity="error" data-testid="tariff-registry-error">
          The signal registry could not be read from the BFF, so no signal can be edited.
        </Alert>
      )}

      <Paper sx={{ p: 2 }}>
        <Stack direction="row" spacing={2} alignItems="center">
          <TextField
            select
            SelectProps={{ native: true }}
            size="small"
            label="Start from event"
            value={loaded?.event.id ?? ""}
            onChange={(e) => selectEvent(e.target.value)}
            inputProps={{ "data-testid": "tariff-event-select" }}
            sx={{ minWidth: 320 }}
            InputLabelProps={{ shrink: true }}
          >
            <option value="" disabled>
              {editable.length === 0 ? "no tariff events on the VTN" : "choose an event"}
            </option>
            {editable.map((e) => (
              <option key={e.id} value={e.id}>
                {e.eventName} (priority {e.priority ?? "none"})
              </option>
            ))}
          </TextField>
          {loadPattern.isError && (
            <Alert severity="error" data-testid="tariff-load-error">
              That event could not be read as a series.
            </Alert>
          )}
        </Stack>
      </Paper>

      {draft && registry && patternStart && patternEnd && (
        <>
          <Paper sx={{ p: 2 }} data-testid="tariff-chart-card">
            <Stack direction="row" spacing={2} alignItems="center" mb={1}>
              <Typography variant="h6">Tariff and fleet response</Typography>
              <Typography variant="caption" color="text.secondary">
                solid = draft, dashed = live
              </Typography>
            </Stack>
            {preview.data?.truncated && (
              <Alert severity="warning" data-testid="tariff-preview-truncated">
                Receivers expand at most {preview.data.maxLoopedIntervals} intervals, so the
                repetition stops before the end of this window.
              </Alert>
            )}
            <FleetTariffChart
              signals={signals.data}
              windowMinutes={BACK_MINUTES}
              aheadMinutes={AHEAD_MINUTES}
              tickMinutes={TICK_MINUTES}
              nowMs={nowMs}
              draftBands={draftBands}
            />
            <Typography variant="subtitle2" color="text.secondary" sx={{ mt: 1 }}>
              Fleet power
            </Typography>
            {history.data && (
              <FleetPowerChart
                history={history.data}
                windowMinutes={BACK_MINUTES}
                aheadMinutes={AHEAD_MINUTES}
                tickMinutes={TICK_MINUTES}
                nowMs={nowMs}
                reportingVens={live.data?.fleet.contributingVens ?? 0}
              />
            )}
            {publishedAt !== null && (
              <Alert severity="success" sx={{ mt: 1 }} data-testid="tariff-published-note">
                Published at {new Date(publishedAt).toLocaleTimeString()}. The VENs pick it up at
                their next poll and replan; the fleet power above shows the response as it arrives.
              </Alert>
            )}
          </Paper>

          <Paper sx={{ p: 2 }}>
            <Stack spacing={2}>
              <Stack direction="row" spacing={2} alignItems="flex-start">
                <TextField
                  size="small"
                  label="Event name"
                  value={draft.eventName}
                  onChange={(e) => setDraft({ ...draft, eventName: e.target.value })}
                  inputProps={{ "data-testid": "tariff-event-name" }}
                />
                <RecurrenceControl
                  recurrence={draft.recurrence}
                  patternStart={patternStart}
                  patternEnd={patternEnd}
                  dstNote={dstNote}
                  onChange={(recurrence) => setDraft({ ...draft, recurrence })}
                />
              </Stack>

              {issues.length > 0 && (
                <Alert severity="warning" data-testid="tariff-issues">
                  {issues.slice(0, 3).map((i, n) => (
                    <div key={n}>{i.message}</div>
                  ))}
                  {issues.length > 3 && <div>… and {issues.length - 3} more</div>}
                </Alert>
              )}

              <TariffGrid draft={draft} registry={registry} issues={issues} onChange={setDraft} />

              <Stack direction="row" spacing={1}>
                <Button
                  variant="contained"
                  disabled={issues.length > 0}
                  onClick={() => setPublishOpen(true)}
                  data-testid="tariff-publish-btn"
                >
                  Publish…
                </Button>
                <Button
                  onClick={() => loaded && setDraft(draftFromPattern(loaded.event, loaded.pattern, registry))}
                  data-testid="tariff-discard-btn"
                >
                  Discard changes
                </Button>
              </Stack>
            </Stack>
          </Paper>

          <PublishDialog
            open={publishOpen}
            draft={draft}
            registry={registry}
            replacedPriorities={replacedPriorities}
            onClose={() => setPublishOpen(false)}
            onPublished={() => setPublishedAt(Date.now())}
          />
        </>
      )}
    </Stack>
  );
}
