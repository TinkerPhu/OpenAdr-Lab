import type { EventInput, HealthStatus, Program, ProgramInput, VtnEvent, Ven } from "./types";

export class BffApi {
  constructor(public baseUrl: string = "") {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async jsonReq(method: string, path: string, body?: unknown): Promise<Response> {
    const opts: RequestInit = { method, headers: { "Content-Type": "application/json" } };
    if (body !== undefined) opts.body = JSON.stringify(body);
    return fetch(this.url(path), opts);
  }

  async health(): Promise<HealthStatus> {
    const r = await fetch(this.url("/api/health"));
    if (!r.ok) throw new Error(`health ${r.status}`);
    return r.json();
  }

  async programs(): Promise<Program[]> {
    const r = await fetch(this.url("/api/programs"));
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

  async events(): Promise<VtnEvent[]> {
    const r = await fetch(this.url("/api/events"));
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
    const r = await fetch(this.url("/api/vens"));
    if (!r.ok) throw new Error(`vens ${r.status}`);
    return r.json();
  }

  async deleteVen(id: string): Promise<void> {
    const r = await this.jsonReq("DELETE", `/api/vens/${id}`);
    if (!r.ok) throw new Error(`deleteVen ${r.status}`);
  }
}
