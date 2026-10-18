import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  Typography,
} from "@mui/material";
import { useCreateEvent } from "../api/hooks";
import { describePeriod } from "../utils/recurrence";
import {
  supersedePriority,
  toEventInput,
  validateDraft,
  type SignalDraft,
} from "../utils/signalDraft";
import type { SignalRegistry } from "../utils/signalRegistry";

export interface PublishDialogProps {
  open: boolean;
  draft: SignalDraft;
  registry: SignalRegistry;
  /** Priorities of the events this one must beat: every event carrying a signal this draft carries. */
  replacedPriorities: (number | null | undefined)[];
  onClose: () => void;
  /** Called once the VTN has accepted the event, before `onClose`. */
  onPublished?: () => void;
}

/**
 * Says what publishing will do before it does it: who it reaches, the priority it takes
 * and why, and that nothing already on the VTN is edited. The event itself is built only by
 * `toEventInput`, so it cannot go out without its payload descriptors.
 */
export function PublishDialog({
  open,
  draft,
  registry,
  replacedPriorities,
  onClose,
  onPublished,
}: PublishDialogProps) {
  const create = useCreateEvent();
  const valid = validateDraft(draft, registry).length === 0;
  const priority = supersedePriority(replacedPriorities);
  const tiesAtTop = priority === 0 && replacedPriorities.some((p) => p === 0);
  const first = draft.intervals[0];
  const last = draft.intervals[draft.intervals.length - 1];

  function publish() {
    create.mutate(toEventInput(draft, registry, replacedPriorities), {
      onSuccess: () => {
        onPublished?.();
        onClose();
      },
    });
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>Publish tariff</DialogTitle>
      <DialogContent>
        <Stack spacing={1}>
          <Typography>
            Event <strong>{draft.eventName}</strong>, {draft.intervals.length} intervals
            {first && last && (
              <>
                {" "}
                over {describePeriod(Date.parse(last.end) - Date.parse(first.start))}
              </>
            )}
            {draft.recurrence.kind === "forever" && ", repeating forever"}
            {(draft.recurrence.kind === "span" || draft.recurrence.kind === "truncated") &&
              `, for ${draft.recurrence.duration}`}
            .
          </Typography>
          <Typography data-testid="publish-targets">
            Reaches:{" "}
            {draft.targets.length === 0 ? "all VENs (no targets)" : draft.targets.join(", ")}
          </Typography>
          <Typography data-testid="publish-priority">
            Priority {priority}
            {" "}(a lower number wins; this beats {replacedPriorities.length} existing event
            {replacedPriorities.length === 1 ? "" : "s"} carrying the same signal).
          </Typography>
          {tiesAtTop && (
            <Alert severity="info" data-testid="publish-tie-note">
              An existing event already has priority 0, the highest. Priority cannot go lower, so the
              newer event wins by its creation time.
            </Alert>
          )}
          <Typography variant="body2" color="text.secondary" data-testid="publish-supersede-note">
            Existing events are left unchanged; the VENs pick up this one at their next poll.
          </Typography>
          {create.isError && (
            <Alert severity="error" data-testid="publish-error">
              The VTN refused the event.
            </Alert>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="contained"
          onClick={publish}
          disabled={!valid || create.isPending}
          data-testid="publish-confirm"
        >
          Publish
        </Button>
      </DialogActions>
    </Dialog>
  );
}
