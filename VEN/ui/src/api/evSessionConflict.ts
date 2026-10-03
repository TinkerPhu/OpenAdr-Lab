// ── The EV session conflict, as an error the UI can act on ────────────────────
// A clashing charging plan is not a malformed request: the user has to choose
// which of two plans survives. So it reaches the caller as its own error type
// carrying the clashing plans, rather than as a string nobody can render.
//
// The server declares which case it is in `kind` (`ev-session-conflict-resolution`).
// We branch on that declared value — never on the HTTP status alone, and never by
// matching the human-readable message, which is free to change.

/** One queued plan a submission clashes with, as the prompt renders it. */
export type ClashingSession = {
  id: string;
  window_start: string;
  departure_time: string;
  target_soc: number;
};

/** Why a stated replace instruction was refused, when one was stated. */
export type ReplaceRejection =
  | { reason: "not_queued"; ids: string[] }
  | { reason: "not_the_conflict_set"; missing: string[]; extra: string[] }
  | { reason: "still_conflicts"; ids: string[] };

export class EvSessionConflictError extends Error {
  /** The plans this submission clashes with, in window order. */
  readonly conflicts: ClashingSession[];
  /**
   * Exactly what a confirmation must echo back as `replace_session_ids`. Quoted
   * from the server rather than derived here: the set is decided in one place, and
   * a client that recomputed it could confirm something the user was never shown.
   */
  readonly replaceableSessionIds: string[];
  /** Present only when an instruction was stated and refused. */
  readonly rejection?: ReplaceRejection;

  constructor(
    message: string,
    conflicts: ClashingSession[],
    replaceableSessionIds: string[],
    rejection?: ReplaceRejection,
  ) {
    super(message);
    this.name = "EvSessionConflictError";
    this.conflicts = conflicts;
    this.replaceableSessionIds = replaceableSessionIds;
    this.rejection = rejection;
  }

  /** True when a confirmation was sent but no longer matched the queue. */
  get isStaleConfirmation(): boolean {
    return this.rejection !== undefined;
  }
}

/**
 * Parse a refusal body into a conflict error, or return null when the body is
 * something else. Returning null rather than throwing keeps the caller's
 * "is this a conflict?" question separate from "did the server send JSON at all".
 */
export function parseEvSessionConflict(text: string): EvSessionConflictError | null {
  let body: unknown;
  try {
    body = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (b.kind !== "ev_session_conflict" && b.kind !== "ev_replace_rejected") return null;
  const conflicts = Array.isArray(b.conflicts) ? (b.conflicts as ClashingSession[]) : [];
  const ids = Array.isArray(b.replaceable_session_ids)
    ? (b.replaceable_session_ids as string[])
    : conflicts.map((c) => c.id);
  return new EvSessionConflictError(
    typeof b.error === "string" ? b.error : "This charging plan conflicts with an existing one.",
    conflicts,
    ids,
    b.rejection as ReplaceRejection | undefined,
  );
}
