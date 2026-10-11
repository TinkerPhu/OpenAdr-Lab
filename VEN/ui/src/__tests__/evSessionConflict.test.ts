import { describe, expect, it } from "vitest";
import { parseEvSessionConflict } from "../api/evSessionConflict";
import type { EvConflictBody } from "../api/types";

const clash = {
  id: "11111111-1111-1111-1111-111111111111",
  window_start: "2026-10-04T06:00:00Z",
  departure_time: "2026-10-04T08:00:00Z",
  target_soc: 0.8,
};

describe("parseEvSessionConflict", () => {
  it("reads a plain clash: its plans and the ids a confirmation must quote", () => {
    const body: EvConflictBody = {
      kind: "ev_session_conflict",
      error: "conflicts with 1 queued session",
      conflicts: [clash],
      replaceable_session_ids: [clash.id],
    };
    const e = parseEvSessionConflict(JSON.stringify(body));
    expect(e?.message).toBe("conflicts with 1 queued session");
    expect(e?.conflicts).toEqual([clash]);
    expect(e?.replaceableSessionIds).toEqual([clash.id]);
    expect(e?.isStaleConfirmation).toBe(false);
  });

  it("reads a refused confirmation with the reason it was refused", () => {
    const body: EvConflictBody = {
      kind: "ev_replace_rejected",
      error: "replace instruction refused",
      conflicts: [clash],
      replaceable_session_ids: [clash.id],
      rejection: { reason: "not_queued", ids: ["22222222-2222-2222-2222-222222222222"] },
    };
    const e = parseEvSessionConflict(JSON.stringify(body));
    expect(e?.isStaleConfirmation).toBe(true);
    expect(e?.rejection).toEqual(body.rejection);
  });

  it("is null for another kind of refusal, or for a body that is not JSON", () => {
    expect(parseEvSessionConflict(JSON.stringify({ error: "duplicate asset_id" }))).toBeNull();
    expect(parseEvSessionConflict("Bad Gateway")).toBeNull();
  });
});
