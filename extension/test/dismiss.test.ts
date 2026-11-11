import assert from "node:assert/strict";
import { test } from "node:test";
import { Dismissals, dismissalKey } from "../src/dismiss.ts";

test("a post identifies its video before the source does", () => {
  assert.equal(dismissalKey("https://x.com/a/status/1", "blob:1"), "post:https://x.com/a/status/1");
  assert.equal(dismissalKey(null, "https://cdn.example/v.mp4"), "source:https://cdn.example/v.mp4");
  assert.equal(dismissalKey(null, ""), undefined);
});

test("a dismissed post stays dismissed when the feed draws it with a new element", () => {
  const d = new Dismissals<object>();
  const first = {};
  const redrawn = {};
  const key = dismissalKey("https://x.com/a/status/1", "blob:1");
  d.add(first, key);
  assert.ok(d.has(redrawn, dismissalKey("https://x.com/a/status/1", "blob:2")));
  // Other videos keep their button.
  assert.ok(!d.has({}, dismissalKey("https://x.com/a/status/2", "blob:3")));
});

test("a video without post or source is remembered by its element", () => {
  const d = new Dismissals<object>();
  const v = {};
  d.add(v, undefined);
  assert.ok(d.has(v, undefined));
  assert.ok(!d.has({}, undefined));
  // A recycled element that now plays something else is a different video.
  assert.ok(!d.has(v, "source:https://cdn.example/other.mp4"));
});
