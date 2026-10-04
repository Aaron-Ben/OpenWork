import { describe, expect, it } from "vitest";
import { externalUrl } from "../src/main/navigation";

const origin = "http://localhost:5173";

describe("externalUrl", () => {
  it("opens web links in the system browser", () => {
    expect(externalUrl("https://www.postgresql.org/docs/", origin)).toBe("https://www.postgresql.org/docs/");
    expect(externalUrl("http://example.com", origin)).toBe("http://example.com/");
  });

  it("keeps the renderer's own pages in the window", () => {
    expect(externalUrl("http://localhost:5173/index.html", origin)).toBeUndefined();
  });

  it("drops other protocols and malformed links", () => {
    for (const url of ["file:///etc/passwd", "javascript:alert(1)", "crew://x", "not a url"]) {
      expect(externalUrl(url, origin)).toBeUndefined();
    }
  });
});
