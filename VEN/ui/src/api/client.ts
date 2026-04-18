import type {
  VtnEvent, Program, Report, SensorSnapshot, SimSnapshot, TraceEntry,
  SimInjectState, PlannedRates, OadrCapacityState, EnergyPacket, Plan, AssetLedger,
  UserRequestWithSession, FlexibilityEnvelope, CreateUserRequestBody, ControlDescriptor,
  EvSession, CreateEvSessionBody, EvSettings, UpdateEvSettingsBody,
  HeaterTarget, CreateHeaterTargetBody,
  ShiftableLoad, CreateShiftableLoadBody, BaselineOverride, CreateBaselineOverrideBody,
} from "./types";
import type { AssetTimelinePoint } from "../components/controller/types";

let reqCounter = 0;
function requestId(): string {
  return `${Date.now()}-${++reqCounter}-${Math.random().toString(36).slice(2, 8)}`;
}

export class VenApi {
  constructor(public baseUrl: string) {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async getReq(path: string): Promise<Response> {
    const url = this.url(path);
    console.log(`[VEN] GET ${url}`);
    try {
      const r = await fetch(url, {
        headers: { "X-Request-ID": requestId() },
      });
      console.log(`[VEN] GET ${url} → ${r.status}`);
      return r;
    } catch (err) {
      console.error(`[VEN] GET ${url} → network error:`, err);
      throw err;
    }
  }

  private async jsonReq(method: string, path: string, body: unknown): Promise<Response> {
    const url = this.url(path);
    console.log(`[VEN] ${method} ${url}`);
    try {
      const r = await fetch(url, {
        method,
        headers: {
          "Content-Type": "application/json",
          "X-Request-ID": requestId(),
        },
        body: JSON.stringify(body),
      });
      console.log(`[VEN] ${method} ${url} → ${r.status}`);
      return r;
    } catch (err) {
      console.error(`[VEN] ${method} ${url} → network error:`, err);
      throw err;
    }
  }

  async health(): Promise<string> {
    const r = await this.getReq("/health");
    if (!r.ok) throw new Error(`health ${r.status}`);
    return r.text();
  }

  async programs(): Promise<Program[]> {
    const r = await this.getReq("/programs");
    if (!r.ok) throw new Error(`programs ${r.status}`);
    return r.json();
  }

  async events(limit = 100): Promise<VtnEvent[]> {
    const r = await this.getReq(`/events?limit=${limit}`);
    if (!r.ok) throw new Error(`events ${r.status}`);
    return r.json();
  }

  async sensors(): Promise<SensorSnapshot> {
    const r = await this.getReq("/sensors");
    if (!r.ok) throw new Error(`sensors ${r.status}`);
    return r.json();
  }

  async postSensors(payload: Partial<SensorSnapshot>): Promise<SensorSnapshot> {
    const r = await this.jsonReq("POST", "/sensors", payload);
    if (!r.ok) throw new Error(`post sensors ${r.status}`);
    return r.json();
  }

  async reports(): Promise<Report[]> {
    const r = await this.getReq("/reports");
    if (!r.ok) throw new Error(`reports ${r.status}`);
    return r.json();
  }

  async submitReport(payload: unknown): Promise<Report> {
    const r = await this.jsonReq("POST", "/reports", payload);
    if (!r.ok) throw new Error(`submit report ${r.status}`);
    return r.json();
  }

  async updateReport(id: string, payload: unknown): Promise<Report> {
    const r = await this.jsonReq("PUT", `/reports/${id}`, payload);
    if (!r.ok) throw new Error(`update report ${r.status}`);
    return r.json();
  }

  async sim(): Promise<SimSnapshot> {
    const r = await this.getReq("/sim");
    if (!r.ok) throw new Error(`sim ${r.status}`);
    return r.json();
  }

  async trace(limit = 50): Promise<TraceEntry[]> {
    const r = await this.getReq(`/trace/events?limit=${limit}`);
    if (!r.ok) throw new Error(`trace/events ${r.status}`);
    return r.json();
  }

  async assetHistory(assetId: string, limit = 100): Promise<Record<string, unknown>[]> {
    const r = await this.getReq(`/trace/history?asset=${assetId}&limit=${limit}`);
    if (!r.ok) throw new Error(`trace/history ${r.status}`);
    return r.json();
  }

  async simSchema(): Promise<Record<string, ControlDescriptor[]>> {
    const r = await this.getReq("/sim/schema");
    if (!r.ok) throw new Error(`sim/schema ${r.status}`);
    return r.json();
  }

  async getSimInject(): Promise<SimInjectState> {
    const r = await this.getReq("/sim/inject");
    if (!r.ok) throw new Error(`sim inject ${r.status}`);
    return r.json();
  }

  async postSimInject(patch: Partial<SimInjectState>): Promise<void> {
    const r = await this.jsonReq("POST", "/sim/inject", patch);
    if (!r.ok) throw new Error(`post sim inject ${r.status}`);
  }

  async postSimReset(assetId: string, soc: number): Promise<void> {
    const r = await this.jsonReq("POST", `/sim/reset/${assetId}`, { soc });
    if (!r.ok) throw new Error(`sim reset ${r.status}`);
  }

  async metrics(): Promise<string> {
    const r = await this.getReq("/metrics");
    if (!r.ok) throw new Error(`metrics ${r.status}`);
    return r.text();
  }

  async packets(): Promise<EnergyPacket[]> {
    const r = await this.getReq("/packets");
    if (!r.ok) throw new Error(`packets ${r.status}`);
    return r.json();
  }

  async plan(): Promise<Plan | null> {
    const r = await this.getReq("/plan");
    if (!r.ok) throw new Error(`plan ${r.status}`);
    const data = await r.json();
    if (data === null) return null;
    return data as Plan;
  }

  async rates(): Promise<PlannedRates> {
    const r = await this.getReq("/tariffs");
    if (!r.ok) throw new Error(`tariffs ${r.status}`);
    return r.json();
  }

  async capacity(): Promise<OadrCapacityState> {
    const r = await this.getReq("/capacity");
    if (!r.ok) throw new Error(`capacity ${r.status}`);
    return r.json();
  }

  async ledger(): Promise<AssetLedger[]> {
    const r = await this.getReq("/ledger");
    if (!r.ok) throw new Error(`ledger ${r.status}`);
    const data = await r.json();
    // API returns {assetId: AssetLedger, ...} — convert to array
    if (Array.isArray(data)) return data;
    return Object.values(data) as AssetLedger[];
  }

  async userRequests(): Promise<UserRequestWithSession[]> {
    const r = await this.getReq("/user-requests");
    if (!r.ok) throw new Error(`user-requests ${r.status}`);
    return r.json();
  }

  async timeline(
    assetId: string,
    params?: { hoursBack?: number; hoursForward?: number; maxPoints?: number }
  ): Promise<AssetTimelinePoint[]> {
    const qs = new URLSearchParams();
    if (params?.hoursBack !== undefined) qs.set("hours_back", String(params.hoursBack));
    if (params?.hoursForward !== undefined) qs.set("hours_forward", String(params.hoursForward));
    if (params?.maxPoints !== undefined) qs.set("max_points", String(params.maxPoints));
    const path = `/timeline/${encodeURIComponent(assetId)}${qs.toString() ? `?${qs}` : ""}`;
    const r = await this.getReq(path);
    if (!r.ok) throw new Error(`timeline/${assetId} ${r.status}`);
    const raw: { ts: string; values: Record<string, number> | null }[] = await r.json();
    return raw.map((pt) => ({ ts: new Date(pt.ts).getTime(), values: pt.values }));
  }

  async allTimelines(
    params?: { hoursBack?: number; hoursForward?: number; maxPoints?: number; resolution?: number }
  ): Promise<Record<string, AssetTimelinePoint[]>> {
    const qs = new URLSearchParams();
    if (params?.hoursBack !== undefined) qs.set("hours_back", String(params.hoursBack));
    if (params?.hoursForward !== undefined) qs.set("hours_forward", String(params.hoursForward));
    if (params?.resolution !== undefined) qs.set("resolution", String(params.resolution));
    else if (params?.maxPoints !== undefined) qs.set("max_points", String(params.maxPoints));
    const path = `/timeline/all${qs.toString() ? `?${qs}` : ""}`;
    const r = await this.getReq(path);
    if (!r.ok) throw new Error(`timeline/all ${r.status}`);
    const raw: Record<string, { ts: string; values: Record<string, number> | null }[]> = await r.json();
    return Object.fromEntries(
      Object.entries(raw).map(([id, pts]) => [
        id,
        pts.map((pt) => ({ ts: new Date(pt.ts).getTime(), values: pt.values })),
      ])
    );
  }

  async flexibility(): Promise<FlexibilityEnvelope[]> {
    const r = await this.getReq("/flexibility");
    if (!r.ok) throw new Error(`flexibility ${r.status}`);
    return r.json();
  }

  async postRequest(body: CreateUserRequestBody): Promise<UserRequestWithSession> {
    const r = await this.jsonReq("POST", "/user-requests", body);
    if (!r.ok) throw new Error((await r.text()) || `POST /user-requests failed: ${r.status}`);
    return r.json();
  }

  async deleteRequest(id: string): Promise<void> {
    const url = this.url(`/user-requests/${id}`);
    console.log(`[VEN] DELETE ${url}`);
    const r = await fetch(url, { method: "DELETE", headers: { "X-Request-ID": requestId() } });
    console.log(`[VEN] DELETE ${url} → ${r.status}`);
    if (!r.ok) throw new Error((await r.text()) || `DELETE /user-requests/${id} failed: ${r.status}`);
  }

  // ── Device Sessions ───────────────────────────────────────────────────────

  async evSession(): Promise<EvSession | null> {
    const r = await this.getReq("/ev-session");
    if (r.status === 204) return null;
    if (!r.ok) throw new Error(`ev-session ${r.status}`);
    return r.json();
  }

  async postEvSession(body: CreateEvSessionBody): Promise<EvSession> {
    const r = await this.jsonReq("POST", "/ev-session", body);
    if (!r.ok) throw new Error((await r.text()) || `POST /ev-session failed: ${r.status}`);
    return r.json();
  }

  async deleteEvSession(): Promise<void> {
    const url = this.url("/ev-session");
    console.log(`[VEN] DELETE ${url}`);
    const r = await fetch(url, { method: "DELETE", headers: { "X-Request-ID": requestId() } });
    console.log(`[VEN] DELETE ${url} → ${r.status}`);
    if (!r.ok) throw new Error(`DELETE /ev-session failed: ${r.status}`);
  }

  async evSettings(): Promise<EvSettings> {
    const r = await this.getReq("/ev-settings");
    if (!r.ok) throw new Error(`ev-settings ${r.status}`);
    return r.json();
  }

  async putEvSettings(body: UpdateEvSettingsBody): Promise<EvSettings> {
    const r = await this.jsonReq("PUT", "/ev-settings", body);
    if (!r.ok) throw new Error(`PUT /ev-settings ${r.status}`);
    return r.json();
  }

  async heaterTarget(): Promise<HeaterTarget | null> {
    const r = await this.getReq("/heater-target");
    if (r.status === 204) return null;
    if (!r.ok) throw new Error(`heater-target ${r.status}`);
    return r.json();
  }

  async postHeaterTarget(body: CreateHeaterTargetBody): Promise<HeaterTarget> {
    const r = await this.jsonReq("POST", "/heater-target", body);
    if (!r.ok) throw new Error((await r.text()) || `POST /heater-target failed: ${r.status}`);
    return r.json();
  }

  async deleteHeaterTarget(): Promise<void> {
    const url = this.url("/heater-target");
    console.log(`[VEN] DELETE ${url}`);
    const r = await fetch(url, { method: "DELETE", headers: { "X-Request-ID": requestId() } });
    console.log(`[VEN] DELETE ${url} → ${r.status}`);
    if (!r.ok) throw new Error(`DELETE /heater-target failed: ${r.status}`);
  }

  async shiftableLoads(): Promise<ShiftableLoad[]> {
    const r = await this.getReq("/shiftable-loads");
    if (!r.ok) throw new Error(`shiftable-loads ${r.status}`);
    return r.json();
  }

  async postShiftableLoad(body: CreateShiftableLoadBody): Promise<ShiftableLoad> {
    const r = await this.jsonReq("POST", "/shiftable-loads", body);
    if (!r.ok) throw new Error((await r.text()) || `POST /shiftable-loads failed: ${r.status}`);
    return r.json();
  }

  async deleteShiftableLoad(id: string): Promise<void> {
    const url = this.url(`/shiftable-loads/${id}`);
    console.log(`[VEN] DELETE ${url}`);
    const r = await fetch(url, { method: "DELETE", headers: { "X-Request-ID": requestId() } });
    console.log(`[VEN] DELETE ${url} → ${r.status}`);
    if (!r.ok) throw new Error((await r.text()) || `DELETE /shiftable-loads/${id} failed: ${r.status}`);
  }

  async baselineOverride(): Promise<BaselineOverride | null> {
    const r = await this.getReq("/baseline-override");
    if (r.status === 204) return null;
    if (!r.ok) throw new Error(`baseline-override ${r.status}`);
    return r.json();
  }

  async postBaselineOverride(body: CreateBaselineOverrideBody): Promise<BaselineOverride> {
    const r = await this.jsonReq("POST", "/baseline-override", body);
    if (!r.ok) throw new Error((await r.text()) || `POST /baseline-override failed: ${r.status}`);
    return r.json();
  }

  async deleteBaselineOverride(): Promise<void> {
    const url = this.url("/baseline-override");
    console.log(`[VEN] DELETE ${url}`);
    const r = await fetch(url, { method: "DELETE", headers: { "X-Request-ID": requestId() } });
    console.log(`[VEN] DELETE ${url} → ${r.status}`);
    if (!r.ok) throw new Error(`DELETE /baseline-override failed: ${r.status}`);
  }
}
