import { describe, it, expect } from "vitest";
import { formatLabels, parsePrometheusText } from "@lab/charts/prometheus";

// The parser behind both UIs' Metrics pages. These pin what it does today (it was two verbatim
// copies), so moving it to one place cannot change a page.
describe("parsePrometheusText", () => {
  it("returns nothing for empty text", () => {
    expect(parsePrometheusText("")).toEqual([]);
  });

  it("skips comment and blank lines", () => {
    const text = "# HELP up whether up\n# TYPE up gauge\n\n   \nup 1\n";
    expect(parsePrometheusText(text)).toEqual([{ name: "up", labels: {}, value: 1 }]);
  });

  it("reads an unlabelled sample", () => {
    expect(parsePrometheusText("process_open_fds 42")).toEqual([
      { name: "process_open_fds", labels: {}, value: 42 },
    ]);
  });

  it("skips an unlabelled line that has no value", () => {
    expect(parsePrometheusText("lonely_name")).toEqual([]);
  });

  it("reads labels and the value after the closing brace", () => {
    expect(parsePrometheusText('http_requests_total{method="GET",code="200"} 12')).toEqual([
      { name: "http_requests_total", labels: { method: "GET", code: "200" }, value: 12 },
    ]);
  });

  it("keeps an empty label value", () => {
    expect(parsePrometheusText('x{a=""} 1')[0].labels).toEqual({ a: "" });
  });

  it("reads NaN as NaN rather than dropping the row", () => {
    const [row] = parsePrometheusText('broken{a="b"} NaN');
    expect(row.name).toBe("broken");
    expect(Number.isNaN(row.value)).toBe(true);
  });

  it("tolerates CRLF line ends and surrounding whitespace", () => {
    const rows = parsePrometheusText('  a 1  \r\nb{k="v"} 2\r\n');
    expect(rows.map((r) => [r.name, r.value])).toEqual([
      ["a", 1],
      ["b", 2],
    ]);
  });

  it("keeps rows of one metric in the order they appear", () => {
    const rows = parsePrometheusText('m{i="1"} 1\nm{i="2"} 2\nm{i="3"} 3');
    expect(rows.map((r) => r.labels.i)).toEqual(["1", "2", "3"]);
  });
});

describe("formatLabels", () => {
  it("is empty without labels", () => {
    expect(formatLabels({})).toBe("");
  });

  it("joins labels as key=\"value\" pairs", () => {
    expect(formatLabels({ method: "GET", code: "200" })).toBe('method="GET", code="200"');
  });
});
