import { useState } from "react";
import {
  Box,
  Button,
  Card,
  CardActions,
  CardContent,
  CardHeader,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Alert,
  Divider,
  FormControlLabel,
  Slider,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import type {
  CreateUserRequestBody,
  EvSettings,
  EvUsageMode,
  EvUsageSimState,
  UpdateEvSettingsBody,
  UserRequestMode,
  UserRequestWithSession,
} from "../../api/types";
import { ModeSelect } from "./ModeSelect";
import { useSubmitRequest } from "./useSubmitRequest";
import { dateToLocalInputValue } from "../../utils/datetimeLocal";

// ── Helpers ──────────────────────────────────────────────────────────────────

function defaultDateTime(hoursOffset: number): string {
  const d = new Date();
  d.setHours(d.getHours() + hoursOffset);
  return dateToLocalInputValue(d);
}

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/** What each usage class means on screen, declared per case beside the case
 * itself (`declare-dont-branch`) rather than as `if (mode === ...)` chains:
 * `simulated` runs the trip physically and only tells the planner by writing a
 * charge session; `forecast` hands the planner the schedule itself. */
const USAGE_MODE_DISPLAY: Record<EvUsageMode, { label: string; planningChip: string; departurePrefix: string }> = {
  simulated: {
    label: "Usage simulated",
    planningChip: "Charge planning engaged (session)",
    departurePrefix: "Next departure (simulated)",
  },
  forecast: {
    label: "Usage forecast to planner",
    planningChip: "Charge planning engaged (forecast)",
    departurePrefix: "Next departure (forecast)",
  },
};

// ── Props ────────────────────────────────────────────────────────────────────

export type EvCardProps = {
  /** Every active EV request, in window order. An EV may hold several queued
   *  sessions; showing only the first would hide plans the planner is serving. */
  requests: UserRequestWithSession[];
  evSettings: EvSettings | undefined;
  /** ev-usage-simulation / ev-usage-forecast diagnostics; undefined while
   * loading, null when the EV has no usage schedule configured (the common
   * case). */
  usageSim: EvUsageSimState | null | undefined;
  postRequest: (body: CreateUserRequestBody) => Promise<unknown>;
  deleteRequest: (id: string) => Promise<unknown>;
  putEvSettings: (body: UpdateEvSettingsBody) => void;
  isPosting: boolean;
  isDeleting: boolean;
};

// ── Component ────────────────────────────────────────────────────────────────

export function EvCard(props: EvCardProps) {
  const { requests, evSettings, usageSim, postRequest, deleteRequest, putEvSettings, isPosting, isDeleting } = props;

  const [dialogOpen, setDialogOpen] = useState(false);
  const [targetSoc, setTargetSoc] = useState(80);
  const [departure, setDeparture] = useState(defaultDateTime(8));
  const [softDeadline, setSoftDeadline] = useState(false);
  const [mode, setMode] = useState<UserRequestMode>("BY_DEADLINE");
  const [budgetEur, setBudgetEur] = useState("2.00");
  // Empty = "available now" and "use the EV's own default distance" - the two
  // things a user may state but usually will not.
  const [availableFrom, setAvailableFrom] = useState("");
  const [tripDistanceKm, setTripDistanceKm] = useState("");
  // A submission can fail in two quite different ways: a plain error to report,
  // or a clash only the user can resolve. The shared hook keeps both, and makes
  // closing the dialog something success does rather than something the click does.
  const { submit: submitRequest, error: submitError, conflict, clear: clearSubmit } =
    useSubmitRequest(postRequest);

  const queued = requests.filter((r) => r.session?.type === "ev");
  const paused = evSettings?.paused_by_active_session ?? false;
  const oppEnabled = evSettings?.opportunistic_charging_enabled ?? false;

  /** The draft as a request body; `replaceIds` is set only on a confirmed replace. */
  function draftBody(replaceIds?: string[]): CreateUserRequestBody {
    const dt = new Date(departure);
    return {
      asset_id: "ev",
      target_soc: targetSoc / 100,
      target_energy_kwh: null,
      desired_power_kw: 7.0,
      soft_deadline: softDeadline,
      mode,
      budget_eur: mode === "MAX_COST" ? Number(budgetEur) : undefined,
      completion_policy: "CONTINUE",
      earliest_start: availableFrom ? new Date(availableFrom).toISOString() : undefined,
      expected_trip_distance_km: tripDistanceKm ? Number(tripDistanceKm) : undefined,
      deadlines: [{
        latest_end: dt.toISOString(),
        max_total_cost_eur: null,
        max_marginal_rate_eur_kwh: null,
        min_completion: targetSoc / 100,
      }],
      comfort_rates: null,
      replace_session_ids: replaceIds,
    };
  }

  /**
   * Submit, and keep the dialog open on anything that needs the user.
   *
   * The dialog used to close the instant Confirm was pressed, with the promise
   * unobserved — so a refusal was an unhandled rejection and the user was told
   * nothing at all. Closing is now what success does, not what clicking does.
   */
  async function submit(replaceIds?: string[]) {
    await submitRequest(draftBody(replaceIds), () => setDialogOpen(false));
  }

  function handleConfirm() {
    void submit();
  }

  return (
    <Card sx={{ height: "100%" }} data-testid="ev-card">
      <CardHeader title="EV Charging" />
      <CardContent>
        {queued.length > 0 ? (
          <Box data-testid="ev-active-view">
            {queued.map((req) => {
              const session = req.session?.type === "ev" ? req.session : null;
              if (!session) return null;
              return (
                <Box
                  key={req.id}
                  data-testid={`ev-session-${session.id}`}
                  sx={{ mb: 1.5, pb: 1.5, borderBottom: "1px solid", borderColor: "divider" }}
                >
                  <Chip label="ACTIVE" color="success" size="small" data-testid="ev-status-chip" sx={{ mb: 1 }} />
                  <Typography data-testid="ev-target-soc">
                    → {(session.target_soc * 100).toFixed(0)}% SoC
                  </Typography>
                  {/* The window, not just the deadline: with several sessions queued
                      "depart 08:00" alone does not say which one a reader is looking
                      at, nor when the car is free to charge for it. */}
                  <Typography data-testid="ev-window" variant="body2" color="text.secondary">
                    {fmtDate(session.window_start)} → {fmtDate(session.departure_time)}
                  </Typography>
                  <Typography data-testid="ev-departure">
                    Depart: {fmtDate(session.departure_time)}
                  </Typography>
                  {session.expected_trip_distance_km != null && (
                    <Typography data-testid="ev-trip-distance" variant="body2" color="text.secondary">
                      Trip after: {session.expected_trip_distance_km} km
                    </Typography>
                  )}
                  {session.soft_deadline && (
                    <Chip label="Soft deadline" size="small" data-testid="ev-soft-deadline-chip" sx={{ mt: 0.5 }} />
                  )}
                  {session.mode !== "BY_DEADLINE" && (
                    <Chip label={session.mode} size="small" data-testid="ev-mode-chip" sx={{ mt: 0.5, ml: 0.5 }} />
                  )}
                  <Typography data-testid="ev-estimated-cost" sx={{ mt: 0.5 }}>
                    Est. €{req.estimated_cost_eur.toFixed(2)}
                  </Typography>
                  <Button
                    variant="outlined"
                    color="warning"
                    size="small"
                    sx={{ mt: 1 }}
                    data-testid="ev-unplan-btn"
                    disabled={isDeleting}
                    onClick={() => deleteRequest(req.id)}
                  >
                    Unplan
                  </Button>
                </Box>
              );
            })}
            {/* Queueing another session is the capability itself; without this the
                queue would be reachable only through the API. */}
            <Button
              variant="outlined"
              size="small"
              data-testid="ev-plan-another-btn"
              disabled={isPosting}
              onClick={() => setDialogOpen(true)}
            >
              Plan another
            </Button>
          </Box>
        ) : (
          <Box data-testid="ev-idle-view">
            <Typography color="text.secondary">No session planned</Typography>
            <Button
              variant="contained"
              size="small"
              sx={{ mt: 1 }}
              data-testid="ev-plan-btn"
              disabled={isPosting}
              onClick={() => setDialogOpen(true)}
            >
              Plan Charging
            </Button>
          </Box>
        )}
      </CardContent>

      <Divider />

      <CardActions data-testid="ev-settings-section" sx={{ px: 2, flexDirection: "column", alignItems: "flex-start" }}>
        <FormControlLabel
          label="Automatic surplus charging"
          control={
            <Switch
              checked={oppEnabled}
              disabled={paused}
              data-testid="ev-opportunistic-charging-switch"
              onChange={() => putEvSettings({ opportunistic_charging_enabled: !oppEnabled })}
            />
          }
        />
        {paused && (
          <Chip label="Paused — active charging session" size="small" data-testid="ev-opportunistic-paused-chip" />
        )}
      </CardActions>

      {usageSim && (
        <>
          <Divider />
          <CardActions data-testid="ev-usage-sim-section" sx={{ px: 2, flexDirection: "column", alignItems: "flex-start" }}>
            <Chip
              label={USAGE_MODE_DISPLAY[usageSim.mode].label}
              size="small"
              variant="outlined"
              data-testid="ev-usage-mode-chip"
              sx={{ mb: 0.5 }}
            />
            {usageSim.engage_charge_planning && (
              <Chip
                label={USAGE_MODE_DISPLAY[usageSim.mode].planningChip}
                size="small"
                color="info"
                data-testid="ev-plan-ahead-chip"
                sx={{ mb: 0.5 }}
              />
            )}
            {usageSim.next_trip ? (
              <Typography variant="body2" color="text.secondary" data-testid="ev-next-departure-chip">
                {USAGE_MODE_DISPLAY[usageSim.mode].departurePrefix}: {fmtDate(usageSim.next_trip.leave_at)} → back {fmtDate(usageSim.next_trip.return_at)}
                {" "}(−{usageSim.next_trip.expected_soc_drop_pct.toFixed(0)}% SoC)
              </Typography>
            ) : (
              <Typography variant="body2" color="text.secondary">
                No trip currently scheduled
              </Typography>
            )}
          </CardActions>
        </>
      )}

      {/* Plan EV Dialog */}
      <Dialog
        open={dialogOpen}
        onClose={() => {
          setDialogOpen(false);
          clearSubmit();
        }}
        data-testid="ev-dialog"
      >
        <DialogTitle>Plan EV Charging</DialogTitle>
        <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 300, pt: 2 }}>
          {submitError && (
            <Alert severity="error" data-testid="ev-submit-error">
              {submitError}
            </Alert>
          )}
          {conflict && (
            <Alert severity="warning" data-testid="ev-conflict-prompt">
              <Typography variant="body2" sx={{ mb: 1 }}>
                {conflict.conflicts.length === 0
                  ? "The plan you were replacing is no longer there. Nothing needs removing — submit again to queue this one."
                  : conflict.isStaleConfirmation
                    ? "Your charging plans changed while you were deciding. This request now conflicts with:"
                    : "This charging request conflicts with a plan you already have:"}
              </Typography>
              {conflict.conflicts.map((c) => (
                <Typography
                  key={c.id}
                  variant="body2"
                  data-testid={`ev-conflict-${c.id}`}
                  sx={{ fontWeight: 500 }}
                >
                  {fmtDate(c.window_start)} → {fmtDate(c.departure_time)} — to{" "}
                  {Math.round(c.target_soc * 100)}%
                </Typography>
              ))}
              <Box sx={{ display: "flex", gap: 1, mt: 1.5 }}>
                {/* Nothing clashes any more - the plan expired or was deleted while the
                    prompt was open - so the action is a plain resubmission, not a
                    removal. Offering "remove it" here would name nothing and be
                    refused again. */}
                <Button
                  size="small"
                  variant="contained"
                  color="warning"
                  data-testid="ev-conflict-replace-btn"
                  disabled={isPosting}
                  onClick={() =>
                    void submit(
                      conflict.conflicts.length === 0
                        ? undefined
                        : conflict.replaceableSessionIds,
                    )
                  }
                >
                  {conflict.conflicts.length === 0
                    ? "Submit again"
                    : conflict.conflicts.length > 1
                      ? `Remove all ${conflict.conflicts.length} and continue`
                      : "Remove it and continue"}
                </Button>
                <Button
                  size="small"
                  data-testid="ev-conflict-keep-btn"
                  onClick={clearSubmit}
                >
                  {conflict.conflicts.length === 0
                    ? "Cancel"
                    : "Keep it, let me change this"}
                </Button>
              </Box>
            </Alert>
          )}
          <Typography gutterBottom>Target SoC: {targetSoc}%</Typography>
          <Slider
            value={targetSoc}
            onChange={(_, v) => setTargetSoc(v as number)}
            min={20}
            max={100}
            step={5}
            data-testid="ev-soc-slider"
          />
          <TextField
            label="Available from"
            type="datetime-local"
            value={availableFrom}
            onChange={(e) => setAvailableFrom(e.target.value)}
            InputLabelProps={{ shrink: true }}
            helperText="When the car is back and can charge. Leave empty for now."
            inputProps={{ lang: "de", "data-testid": "ev-available-from-input" }}
          />
          <TextField
            label="Departure"
            type="datetime-local"
            value={departure}
            onChange={(e) => setDeparture(e.target.value)}
            InputLabelProps={{ shrink: true }}
            inputProps={{ lang: "de" }}
            data-testid="ev-departure-input"
          />
          <TextField
            label="Trip after departure (km)"
            type="number"
            value={tripDistanceKm}
            onChange={(e) => setTripDistanceKm(e.target.value)}
            InputLabelProps={{ shrink: true }}
            helperText="How far you will drive. Leave empty to use this car's usual distance."
            inputProps={{ min: 0, step: 10, "data-testid": "ev-trip-distance-input" }}
          />
          <FormControlLabel
            label="Soft deadline"
            control={
              <Switch
                checked={softDeadline}
                onChange={(_, v) => setSoftDeadline(v)}
                data-testid="ev-soft-deadline-switch"
              />
            }
          />
          <ModeSelect value={mode} onChange={setMode} testId="ev-mode-select" />
          {mode === "MAX_COST" && (
            <TextField
              label="Budget (€)"
              type="number"
              value={budgetEur}
              onChange={(e) => setBudgetEur(e.target.value)}
              inputProps={{ min: 0, step: 0.5, "data-testid": "ev-budget-input" }}
            />
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialogOpen(false)} data-testid="ev-dialog-cancel">Cancel</Button>
          <Button variant="contained" onClick={handleConfirm} data-testid="ev-dialog-confirm" disabled={isPosting}>
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
