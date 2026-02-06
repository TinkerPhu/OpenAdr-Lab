type EventDraft = {
  programId: string;
  targets: { venIds: string[]; resourceIds?: string[] };
  timing: { start: string; durationMinutes: number };
  payload: Record<string, any>; // later: typed payloads
};
