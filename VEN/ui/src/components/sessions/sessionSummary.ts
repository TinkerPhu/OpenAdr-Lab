import type { UserRequestWithSession } from "../../api/types";

export function fmtDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function sessionSummary(req: UserRequestWithSession): string {
  const s = req.session;
  if (!s) return req.asset_id;
  if (s.type === "ev") return `${(s.target_soc * 100).toFixed(0)}% SoC · depart ${fmtDate(s.departure_time)}`;
  if (s.type === "heater") return `${s.target_temp_c}°C · ready ${fmtDate(s.ready_by)}`;
  if (s.type === "shiftable_load") return `${s.power_kw}kW ${s.duration_min}min · by ${fmtDate(s.latest_end)}`;
  return req.asset_id;
}

export function deviceIcon(req: UserRequestWithSession): string {
  if (req.session_type === "ev") return "⚡";
  if (req.session_type === "heater") return "🔥";
  return "⏱";
}

/** Session-specific deadline: EV departure, heater ready-by, shiftable latest-end. */
export function sessionDeadline(req: UserRequestWithSession): Date | null {
  const s = req.session;
  if (s?.type === "ev") return new Date(s.departure_time);
  if (s?.type === "heater") return new Date(s.ready_by);
  if (s?.type === "shiftable_load") return new Date(s.latest_end);
  const last = req.deadlines[req.deadlines.length - 1];
  return last ? new Date(last.latest_end) : null;
}
