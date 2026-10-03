// ── Submitting a user request, with the failure actually surfaced ─────────────
// Every device card had the same three lines: call `postRequest`, ignore the
// promise, close the dialog. So every refusal was an unhandled rejection and the
// user saw a dialog close as though it had worked — including the EV's 409 clash,
// which exists precisely to tell them something.
//
// Generalised here rather than fixed three times: closing the dialog is what
// *success* does, and that rule belongs in one place instead of in each card's
// memory. Cards keep their own rendering — the EV is the only one that can get a
// session clash, so it is the only one that renders that branch.

import { useState } from "react";

import type { CreateUserRequestBody } from "../../api/types";
import { EvSessionConflictError } from "../../api/evSessionConflict";

type PostRequest = (body: CreateUserRequestBody) => Promise<unknown>;

export type SubmitRequestState = {
  /**
   * Submit `body`; call `onSuccess` only if it succeeded. Any failure is captured
   * below instead of escaping, so the caller's dialog can stay open with the
   * user's draft intact.
   */
  submit: (body: CreateUserRequestBody, onSuccess: () => void) => Promise<void>;
  /** A plain failure to report. */
  error: string | null;
  /**
   * A clash the user has to resolve — a question, not a failure. Only ever set for
   * the EV, the only device whose sessions form a queue.
   */
  conflict: EvSessionConflictError | null;
  /** Forget both, e.g. when the dialog closes or the draft is edited. */
  clear: () => void;
};

export function useSubmitRequest(postRequest: PostRequest): SubmitRequestState {
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<EvSessionConflictError | null>(null);

  function clear() {
    setError(null);
    setConflict(null);
  }

  async function submit(body: CreateUserRequestBody, onSuccess: () => void) {
    clear();
    try {
      await postRequest(body);
      onSuccess();
    } catch (e) {
      if (e instanceof EvSessionConflictError) {
        setConflict(e);
      } else {
        setError(e instanceof Error ? e.message : String(e));
      }
    }
  }

  return { submit, error, conflict, clear };
}
