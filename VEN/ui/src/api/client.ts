import type { VtnEvent, Program, Report, SensorSnapshot } from "./types";

export class VenApi {
  constructor(public baseUrl: string) {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async getReq(path: string): Promise<Response> {
    return fetch(this.url(path), {
      headers: { "X-Request-ID": crypto.randomUUID() },
    });
  }

  private async jsonReq(method: string, path: string, body: unknown): Promise<Response> {
    return fetch(this.url(path), {
      method,
      headers: {
        "Content-Type": "application/json",
        "X-Request-ID": crypto.randomUUID(),
      },
      body: JSON.stringify(body),
    });
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
}
