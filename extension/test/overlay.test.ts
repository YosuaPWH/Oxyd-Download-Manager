import assert from "node:assert/strict";
import { test } from "node:test";
import { effectiveLayout, overlaySettings, placeOverlay, siteHost } from "../src/overlay.ts";

test("existing profiles retain the standard top-right button", () => {
  assert.deepEqual(effectiveLayout(overlaySettings(undefined), "example.net"), {
    compact: false, corner: "top-right",
  });
});

test("site overrides apply to the browser tab's hostname and inherit omitted values", () => {
  const prefs = overlaySettings({
    defaults: { compact: true, corner: "top-left" },
    sites: {
      "example.net": { compact: false, corner: "bottom-right" },
      "video.example.net": { corner: "bottom-left" },
      "broken.example.net": { compact: 1, corner: "middle" },
    },
  });
  assert.equal(siteHost("https://EXAMPLE.NET.:1234/player"), "example.net");
  assert.equal(siteHost("chrome://extensions"), undefined);
  assert.deepEqual(effectiveLayout(prefs, "example.net"), { compact: false, corner: "bottom-right" });
  assert.deepEqual(effectiveLayout(prefs, "video.example.net"), { compact: true, corner: "bottom-left" });
  assert.deepEqual(effectiveLayout(prefs, "broken.example.net"), prefs.defaults);
  assert.deepEqual(effectiveLayout(prefs, "other.net"), prefs.defaults);
});

test("each corner places the controls at the matching edge of the player", () => {
  const player = { left: 50, top: 60, right: 350, bottom: 210 };
  const viewport = { width: 800, height: 650 };
  const bar = { width: 56, height: 26 };
  for (const [corner, x, y] of [["top-left", 60, 70], ["top-right", 284, 70],
    ["bottom-left", 60, 174], ["bottom-right", 284, 174]] as const) {
    const layout = placeOverlay(corner, player, viewport, bar);
    assert.deepEqual([layout.x, layout.y], [x, y]);
  }
});

test("the menu opens upward and remains usable in a narrow viewport", () => {
  const layout = placeOverlay("bottom-left", { left: 6, top: 90, right: 258, bottom: 280 },
    { width: 260, height: 280 }, { width: 56, height: 26 });
  assert.equal(layout.up, true);
  assert.equal(layout.x + layout.menuLeft, 8);
  assert.equal(layout.width, 244);
  assert.ok(layout.maxHeight > 0);
  assert.equal(layout.y - 6 - layout.maxHeight, 8);
});
