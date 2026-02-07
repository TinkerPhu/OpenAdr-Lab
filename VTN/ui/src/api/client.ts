import type { HealthStatus, Program, VtnEvent, Ven } from "./types";

export class BffApi {
  constructor(public baseUrl: string = "") {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
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

  async events(): Promise<VtnEvent[]> {
    const r = await fetch(this.url("/api/events"));
    if (!r.ok) throw new Error(`events ${r.status}`);
    return r.json();
  }

  async vens(): Promise<Ven[]> {
    const r = await fetch(this.url("/api/vens"));
    if (!r.ok) throw new Error(`vens ${r.status}`);
    return r.json();
  }
}
