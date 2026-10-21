import assert from "node:assert/strict";
import { test } from "node:test";
import { extensionOf, hostMatches, shouldCapture } from "../src/filters.ts";
import { DEFAULTS, parseList } from "../src/settings.ts";

const ctx = { selfId: "self", bypass: new Set<string>(), skipNext: false };
const MB = 1024 * 1024;

test("captures matching file types", () => {
  const d = shouldCapture({ url: "https://x.org/setup.exe", totalBytes: 50 * MB }, DEFAULTS, ctx);
  assert.deepEqual(d, { capture: true });
});

test("uses the browser-determined filename", () => {
  const item = { url: "https://x.org/dl?id=1", filename: "C:\\Users\\me\\Downloads\\movie.mkv" };
  assert.equal(shouldCapture(item, DEFAULTS, ctx).capture, true);
});

test("skips small, unknown and excluded downloads", () => {
  const small = shouldCapture({ url: "https://x.org/a.zip", totalBytes: 1000 }, DEFAULTS, ctx);
  assert.deepEqual(small, { capture: false, reason: "too-small" });
  const page = shouldCapture({ url: "https://x.org/index.html" }, DEFAULTS, ctx);
  assert.deepEqual(page, { capture: false, reason: "extension" });
  const excluded = shouldCapture(
    { url: "https://cdn.example.com/a.zip" },
    { ...DEFAULTS, excludedDomains: ["example.com"] },
    ctx,
  );
  assert.deepEqual(excluded, { capture: false, reason: "excluded-domain" });
});

test("never captures blob/data URLs, its own downloads or bypassed URLs", () => {
  assert.equal(shouldCapture({ url: "blob:https://x.org/1" }, DEFAULTS, ctx).capture, false);
  assert.equal(shouldCapture({ url: "data:text/plain,hi" }, DEFAULTS, ctx).capture, false);
  assert.equal(
    shouldCapture({ url: "https://x.org/a.zip", byExtensionId: "self" }, DEFAULTS, ctx).capture,
    false,
  );
  const bypass = new Set(["https://x.org/a.zip"]);
  assert.equal(shouldCapture({ url: "https://x.org/a.zip" }, DEFAULTS, { ...ctx, bypass }).capture, false);
  assert.equal(shouldCapture({ url: "https://x.org/a.zip" }, DEFAULTS, { ...ctx, skipNext: true }).capture, false);
  assert.equal(shouldCapture({ url: "https://x.org/a.zip" }, { ...DEFAULTS, enabled: false }, ctx).capture, false);
});

test("empty extension list captures everything", () => {
  const s = { ...DEFAULTS, extensions: [], minSize: 0 };
  assert.equal(shouldCapture({ url: "https://x.org/page" }, s, ctx).capture, true);
});

test("helpers", () => {
  assert.equal(extensionOf("C:\\a\\b.TAR.GZ"), "gz");
  assert.equal(extensionOf("file"), "");
  assert.equal(extensionOf("a.zip?x=1"), "zip");
  assert.ok(hostMatches("https://a.b.example.com/x", ["example.com"]));
  assert.ok(!hostMatches("https://badexample.com/x", ["example.com"]));
  assert.ok(hostMatches("https://example.com/x", ["*.example.com"]));
  assert.deepEqual(parseList(".ZIP, rar  mp4 zip", true), ["zip", "rar", "mp4"]);
});
