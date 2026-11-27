import { strict as assert } from "node:assert";
import { test } from "node:test";
import { attachmentName } from "../src/firefox-capture.ts";

test("recovers raw UTF-8 names Firefox exposes as Latin-1", () => {
  const name = "อัพเดท; 日本語.zip";
  const headerBytes = Buffer.from(name, "utf8").toString("latin1");
  assert.equal(attachmentName(`attachment; filename="${headerBytes}"`), name);
  assert.equal(attachmentName(`attachment; filename="${name}"`), name);
  assert.equal(attachmentName('attachment; filename="café.zip"'), "café.zip");
});

test("prefers a valid extended name and falls back from invalid UTF-8", () => {
  assert.equal(attachmentName("attachment; filename=x.zip; filename*=UTF-8''caf%C3%A9.zip"), "café.zip");
  assert.equal(attachmentName("attachment; filename=x.zip; filename*=UTF-8''%FF.zip"), "x.zip");
});
