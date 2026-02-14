import type { VtnEvent, Program, Report, SensorSnapshot, SimSnapshot, TraceEntry } from "./types";

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
    const r = await this.getReq(`/trace?limit=${limit}`);
    if (!r.ok) throw new Error(`trace ${r.status}`);
    return r.json();
  }

  async metrics(): Promise<string> {
    const r = await this.getReq("/metrics");
    if (!r.ok) throw new Error(`metrics ${r.status}`);
    return r.text();
  }
}
