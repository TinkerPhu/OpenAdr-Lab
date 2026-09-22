import { useState } from "react";
import {
  Alert, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import { useFleetReactions } from "../api/hooks";
import { formatKw } from "../utils/power";

/**
 * "Who saw this event, and what changed?" for one event id.
 *
 * The table reports what each VEN said and what its power did; it draws no
 * conclusion about whether that counts as reacting. The operator looking at a
 * fleet of twenty knows what their sites are for; a threshold hard-coded here
 * would not.
 */
export function FleetReactions() {
  const [eventId, setEventId] = useState("");
  const reactions = useFleetReactions(eventId);

  return (
    <Paper sx={{ p: 2 }} data-testid="fleet-reactions-card">
      <Stack spacing={1}>
        <Typography variant="h6">Reactions to an event</Typography>
        <TextField
          size="small"
          label="Event ID"
          value={eventId}
          onChange={(e) => setEventId(e.target.value)}
          inputProps={{ "data-testid": "fleet-reactions-input" }}
        />

        {reactions.isError && (
          <Alert severity="info" data-testid="fleet-reactions-error">
            No reaction history — the telemetry store is not connected.
          </Alert>
        )}

        {reactions.data && (
          <>
            {/* The list holds only the VENs that said they saw it, so this is
                a count of evidence and not of the fleet. */}
            <Typography variant="body2" data-testid="fleet-reactions-count">
              {reactions.data.vensSeen} VEN(s) saw this event
            </Typography>
            {reactions.data.vensSeen === 0 ? (
              <Typography variant="body2" data-testid="fleet-reactions-none">
                No VEN has reported seeing it.
              </Typography>
            ) : (
              <Table size="small">
                <TableHead>
                  <TableRow>
                    <TableCell>VEN</TableCell>
                    <TableCell>Saw it</TableCell>
                    <TableCell>Replanned</TableCell>
                    <TableCell align="right">Before</TableCell>
                    <TableCell align="right">After</TableCell>
                    <TableCell align="right">Change</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {reactions.data.vens.map((v) => (
                    <TableRow key={v.venName} data-testid={`fleet-reaction-${v.venName}`}>
                      <TableCell>{v.venName}</TableCell>
                      <TableCell>{new Date(v.seenAt).toLocaleTimeString()}</TableCell>
                      {/* A VEN that saw the event and never replanned is a
                          real outcome, not missing data. And a replan the VEN
                          itself attributed to this event is a different claim
                          from one we inferred from a 15-minute window — the
                          table says which, because only the first answers
                          "did the event work". */}
                      <TableCell>
                        {v.replannedAt ? (
                          <>
                            {new Date(v.replannedAt).toLocaleTimeString()}
                            <Typography
                              variant="caption"
                              display="block"
                              color={v.replanAttributed ? "success.main" : "text.secondary"}
                            >
                              {v.replanAttributed ? "named this event" : "inferred from timing"}
                            </Typography>
                          </>
                        ) : (
                          "never"
                        )}
                      </TableCell>
                      <TableCell align="right">{formatKw(v.powerBeforeW)}</TableCell>
                      <TableCell align="right">{formatKw(v.powerAfterW)}</TableCell>
                      <TableCell align="right">{formatKw(v.deltaW)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </>
        )}
      </Stack>
    </Paper>
  );
}
