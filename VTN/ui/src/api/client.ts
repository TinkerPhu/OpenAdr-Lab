import type { EventInput, FleetHistory, FleetLive, FleetReactions, FleetSignals, HealthStatus, Program, ProgramInput, Report, VtnEvent, Ven } from "./types";
import { debugLog } from "../utils/debugLog";

let reqCounter = 0;
function requestId(): string {
  return `${Date.now()}-${++reqCounter}-${Math.random().toString(36).slice(2, 8)}`;
}

export class BffApi {
  constructor(public baseUrl: string = "") {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async jsonReq(method: string, path: string, body?: unknown): Promise<Response> {
    const url = this.url(path);
    debugLog(`[BFF] ${method} ${url}`);
    const opts: RequestInit = {
      method,
      headers: {
        "Content-Type": "application/json",
        "X-Request-ID": requestId(),
      },
    };
    if (body !== undefined) opts.body = JSON.stringify(body);
    try {
      const r = await fetch(url, opts);
      debugLog(`[BFF] ${method} ${url} → ${r.status}`);
      return r;
    } catch (err) {
      console.error(`[BFF] ${method} ${url} → network error:`, err);
      throw err;
    }
  }

  private async getReq(path: string): Promise<Response> {
    const url = this.url(path);
    debugLog(`[BFF] GET ${url}`);
    try {
      const r = await fetch(url, {
        headers: { "X-Request-ID": requestId() },
      });
      debugLog(`[BFF] GET ${url} → ${r.status}`);
      return r;
    } catch (err) {
      console.error(`[BFF] GET ${url} → network error:`, err);
      throw err;
    }
  }

  async health(): Promise<HealthStatus> {
    const r = await this.getReq("/api/health");
    if (!r.ok) throw new Error(`health ${r.status}`);
    return r.json();
  }

  /** The fleet's latest reading per VEN. */
  async fleetPower(): Promise<FleetLive> {
    const r = await this.getReq("/api/fleet/power");
    if (!r.ok) throw new Error(`fleet power ${r.status}`);
    return r.json();
  }

  /** The fleet's stored telemetry over a window. */
  async fleetHistory(fromIso: string, stepSeconds: number): Promise<FleetHistory> {
    const r = await this.getReq(
      `/api/fleet/power?from=${encodeURIComponent(fromIso)}&stepSeconds=${stepSeconds}`,
    );
    if (!r.ok) throw new Error(`fleet history ${r.status}`);
    return r.json();
  }

  /** Who saw one event, and what their power did around it. */
  async fleetReactions(eventId: string): Promise<FleetReactions> {
    const r = await this.getReq(`/api/fleet/reactions?eventID=${encodeURIComponent(eventId)}`);
    if (!r.ok) throw new Error(`fleet reactions ${r.status}`);
    return r.json();
  }

  /** What each VEN was being told over a window. */
  async fleetSignals(fromIso: string, toIso?: string): Promise<FleetSignals> {
    const to = toIso ? `&to=${encodeURIComponent(toIso)}` : "";
    const r = await this.getReq(`/api/fleet/signals?from=${encodeURIComponent(fromIso)}${to}`);
    if (!r.ok) throw new Error(`fleet signals ${r.status}`);
    return r.json();
  }

  async programs(): Promise<Program[]> {
    const r = await this.getReq("/api/programs");
    if (!r.ok) throw new Error(`programs ${r.status}`);
    return r.json();
  }

  async createProgram(input: ProgramInput): Promise<Program> {
    const r = await this.jsonReq("POST", "/api/programs", input);
    if (!r.ok) throw new Error(`createProgram ${r.status}`);
    return r.json();
  }

  async updateProgram(id: string, input: ProgramInput): Promise<Program> {
    const r = await this.jsonReq("PUT", `/api/programs/${id}`, input);
    if (!r.ok) throw new Error(`updateProgram ${r.status}`);
    return r.json();
  }

  async deleteProgram(id: string): Promise<void> {
    const r = await this.jsonReq("DELETE", `/api/programs/${id}`);
    if (!r.ok) throw new Error(`deleteProgram ${r.status}`);
  }

  async events(active?: boolean): Promise<VtnEvent[]> {
    const path = active === undefined ? "/api/events" : `/api/events?active=${active}`;
    const r = await this.getReq(path);
    if (!r.ok) throw new Error(`events ${r.status}`);
    return r.json();
  }

  async createEvent(input: EventInput): Promise<VtnEvent> {
    const r = await this.jsonReq("POST", "/api/events", input);
    if (!r.ok) throw new Error(`createEvent ${r.status}`);
    return r.json();
  }

  async updateEvent(id: string, input: EventInput): Promise<VtnEvent> {
    const r = await this.jsonReq("PUT", `/api/events/${id}`, input);
    if (!r.ok) throw new Error(`updateEvent ${r.status}`);
    return r.json();
  }

  async deleteEvent(id: string): Promise<void> {
    const r = await this.jsonReq("DELETE", `/api/events/${id}`);
    if (!r.ok) throw new Error(`deleteEvent ${r.status}`);
  }

  async vens(): Promise<Ven[]> {
    const r = await this.getReq("/api/vens");
    if (!r.ok) throw new Error(`vens ${r.status}`);
    return r.json();
  }

  async deleteVen(id: string): Promise<void> {
    const r = await this.jsonReq("DELETE", `/api/vens/${id}`);
    if (!r.ok) throw new Error(`deleteVen ${r.status}`);
  }

  async reports(): Promise<Report[]> {
    const r = await this.getReq("/api/reports");
    if (!r.ok) throw new Error(`reports ${r.status}`);
    return r.json();
  }

  async deleteReport(id: string): Promise<void> {
    const r = await this.jsonReq("DELETE", `/api/reports/${id}`);
    if (!r.ok) throw new Error(`deleteReport ${r.status}`);
  }

  async metrics(): Promise<string> {
    const r = await this.getReq("/api/metrics");
    if (!r.ok) throw new Error(`metrics ${r.status}`);
    return r.text();
  }
}
