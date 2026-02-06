import type { Event, Program, SensorSnapshot } from "./types";

export class VenApi {
  constructor(public baseUrl: string) {}

  private url(path: string) {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  async health(): Promise<string> {
    const r = await fetch(this.url("/health"));
    if (!r.ok) throw new Error(`health ${r.status}`);
    return r.text();
  }

  async programs(): Promise<Program[]> {
    const r = await fetch(this.url("/programs"));
    if (!r.ok) throw new Error(`programs ${r.status}`);
    return r.json();
  }

  async events(limit = 100): Promise<Event[]> {
    const r = await fetch(this.url(`/events?limit=${limit}`));
    if (!r.ok) throw new Error(`events ${r.status}`);
    return r.json();
  }

  async sensors(): Promise<SensorSnapshot> {
    const r = await fetch(this.url("/sensors"));
    if (!r.ok) throw new Error(`sensors ${r.status}`);
    return r.json();
  }

  async postSensors(payload: Partial<SensorSnapshot>): Promise<SensorSnapshot> {
    const r = await fetch(this.url("/sensors"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!r.ok) throw new Error(`post sensors ${r.status}`);
    return r.json();
  }
}
