import assert from "node:assert/strict";
import { test } from "node:test";
import { addStream, classifyMedia, isVideoSite, postUrl, streamKey, streamLabel } from "../src/media.ts";

const MB = 1024 * 1024;

test("knows video sites", () => {
  assert.ok(isVideoSite("https://www.youtube.com/watch?v=x"));
  assert.ok(isVideoSite("https://m.facebook.com/reel/1"));
  assert.ok(!isVideoSite("https://notyoutube.com/"));
  assert.ok(!isVideoSite("not a url"));
});

test("finds single posts", () => {
  const base = "https://www.instagram.com/";
  assert.equal(postUrl("/reel/Cx1-2/", base), "https://www.instagram.com/reel/Cx1-2/");
  assert.equal(postUrl("/someone/p/Abc/", base), "https://www.instagram.com/someone/p/Abc/");
  assert.equal(postUrl("/explore/", base), undefined);
  assert.equal(
    postUrl("https://www.youtube.com/watch?v=abc&list=PL1&t=42#x", base),
    "https://www.youtube.com/watch?v=abc&t=42",
  );
  assert.equal(postUrl("https://www.youtube.com/watch?list=PL1", base), undefined);
  assert.equal(postUrl("https://x.com/user/status/123/photo/1", base), "https://x.com/user/status/123/photo/1");
  assert.equal(postUrl("https://www.tiktok.com/@a.b/video/77", base), "https://www.tiktok.com/@a.b/video/77");
  assert.equal(postUrl("javascript:alert(1)", base), undefined);
  assert.equal(postUrl(undefined, base), undefined);
});

test("classifies media responses", () => {
  assert.equal(classifyMedia({ url: "https://cdn.x/master.m3u8" }), "hls");
  assert.equal(classifyMedia({ url: "https://cdn.x/a", contentType: "application/vnd.apple.mpegurl" }), "hls");
  assert.equal(classifyMedia({ url: "https://cdn.x/manifest.mpd" }), "dash");
  assert.equal(classifyMedia({ url: "https://cdn.x/v.mp4", size: 30 * MB }), "file");
  assert.equal(classifyMedia({ url: "https://cdn.x/v", contentType: "video/mp4", size: 30 * MB }), "file");
  assert.equal(classifyMedia({ url: "https://cdn.x/ping.mp3", size: 20_000 }), null);
  assert.equal(classifyMedia({ url: "https://cdn.x/seg-12.ts", contentType: "video/mp2t" }), null);
  assert.equal(classifyMedia({ url: "https://cdn.x/page", contentType: "text/html" }), null);
  assert.equal(classifyMedia({ url: "blob:https://x/1" }), null);
});

test("keeps one entry per stream", () => {
  const a = { url: "https://cdn.x/v.mp4?range=0-100&sig=1", kind: "file" as const, seen: 1 };
  const b = { url: "https://cdn.x/v.mp4?range=100-200&sig=2", kind: "file" as const, seen: 2 };
  assert.equal(streamKey(a.url), streamKey(b.url));
  const list = addStream(addStream([], a), b);
  assert.equal(list.length, 1);
  assert.equal(list[0].seen, 2);
  assert.deepEqual(streamLabel({ url: "https://cdn.x/720p/v.mp4", kind: "file", size: 5 * MB, mime: "video/mp4", seen: 0 }), {
    label: "MP4",
    detail: "720p · 5.0 MB",
  });
});
