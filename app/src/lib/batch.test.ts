import { describe, expect, it } from "vitest";
import { expandPattern, linkName, parseBatch } from "./batch";

describe("expandPattern", () => {
  it("returns a plain link unchanged", () => {
    expect(expandPattern("https://x.org/a.zip")).toEqual(["https://x.org/a.zip"]);
  });

  it("keeps leading zeros of the first bound", () => {
    expect(expandPattern("https://x.org/p-[008-011].jpg")).toEqual([
      "https://x.org/p-008.jpg",
      "https://x.org/p-009.jpg",
      "https://x.org/p-010.jpg",
      "https://x.org/p-011.jpg",
    ]);
    expect(expandPattern("f[9-11]")).toEqual(["f9", "f10", "f11"]);
  });

  it("supports steps, descending ranges and letters", () => {
    expect(expandPattern("f[0-10:5]")).toEqual(["f0", "f5", "f10"]);
    expect(expandPattern("f[3-1]")).toEqual(["f3", "f2", "f1"]);
    expect(expandPattern("d-[a-c].iso")).toEqual(["d-a.iso", "d-b.iso", "d-c.iso"]);
    expect(expandPattern("d-[C-A]")).toEqual(["d-C", "d-B", "d-A"]);
  });

  it("combines several ranges, the left one slowest", () => {
    expect(expandPattern("s[1-2]e[1-2]")).toEqual(["s1e1", "s1e2", "s2e1", "s2e2"]);
  });

  it("leaves other brackets alone", () => {
    expect(expandPattern("http://[::1]:8080/a")).toEqual(["http://[::1]:8080/a"]);
    expect(expandPattern("https://x.org/a[draft].pdf")).toEqual(["https://x.org/a[draft].pdf"]);
  });

  it("stops just past the limit", () => {
    expect(expandPattern("f[1-9999999]", 50)).toHaveLength(51);
    expect(expandPattern("f[1-100][1-100]", 50)).toHaveLength(51);
  });
});

describe("parseBatch", () => {
  it("reads links separated by lines and spaces", () => {
    const b = parseBatch("https://x.org/a.zip\n\n  https://x.org/b.zip https://x.org/c.zip\r\n");
    expect(b.urls).toEqual(["https://x.org/a.zip", "https://x.org/b.zip", "https://x.org/c.zip"]);
    expect(b).toMatchObject({ invalid: 0, duplicates: 0, truncated: false });
  });

  it("counts text that is not a link and repeated links", () => {
    const b = parseBatch("see https://x.org/a.zip mailto:me@x.org https://x.org/a.zip https://x.org/f[1-2] https://x.org/f1");
    expect(b.urls).toEqual(["https://x.org/a.zip", "https://x.org/f1", "https://x.org/f2"]);
    expect(parseBatch("ftp://x.org/pub/a.iso").urls).toEqual(["ftp://x.org/pub/a.iso"]);
    expect(b.invalid).toBe(2);
    expect(b.duplicates).toBe(2);
  });

  it("reports a batch that is too large", () => {
    const b = parseBatch("https://x.org/f[1-500].bin", 100);
    expect(b.urls).toHaveLength(100);
    expect(b.truncated).toBe(true);
  });
});

describe("linkName", () => {
  it("decodes the last path segment", () => {
    expect(linkName("https://x.org/a/My%20File.iso?sig=1")).toBe("My File.iso");
    expect(linkName("https://x.org/")).toBe("x.org");
  });
});
