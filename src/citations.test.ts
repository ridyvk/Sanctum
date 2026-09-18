import { describe, expect, it } from "vitest";
import { normalizeDoi, parseBibTeX } from "./citations";

describe("citation import", () => {
  it("normalizes DOI URLs and prefixes", () => {
    expect(normalizeDoi("https://doi.org/10.1000/XYZ")).toBe("10.1000/XYZ");
    expect(normalizeDoi("doi: 10.5555/test")).toBe("10.5555/test");
  });

  it("parses multiple BibTeX entries with nested braces", () => {
    const records = parseBibTeX(`
      @article{smith2026,
        title = {A {Durable} Research Record},
        author = {Smith, Jane and Doe, John},
        year = {2026},
        doi = {10.1000/example}
      }
      @misc{dataset,
        title = "Replication Data",
        url = {https://example.test/data}
      }
    `);
    expect(records).toHaveLength(2);
    expect(records[0]).toMatchObject({
      citationKey: "smith2026",
      title: "A Durable Research Record",
      year: 2026,
      doi: "10.1000/example",
    });
    expect(records[1].url).toBe("https://example.test/data");
  });
});
