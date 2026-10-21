import assert from "node:assert/strict";
import { test } from "node:test";
import { handOffDownload, type DownloadOps } from "../src/handoff.ts";

function fakeOps(over: Partial<DownloadOps> & { accepted: boolean }) {
  const calls: string[] = [];
  const ops: DownloadOps = {
    pause: async (id) => void calls.push(`pause ${id}`),
    resume: async (id) => void calls.push(`resume ${id}`),
    cancel: async (id) => void calls.push(`cancel ${id}`),
    erase: async (id) => void calls.push(`erase ${id}`),
    redownload: async (url) => void calls.push(`redownload ${url}`),
    offer: async () => {
      calls.push("offer");
      return over.accepted;
    },
    ...over,
  };
  return { ops, calls };
}

const item = { id: 7, url: "https://x.org/a.zip", filename: "a.zip" };

test("cancels the browser copy only after Oxyd accepted", async () => {
  const { ops, calls } = fakeOps({ accepted: true });
  assert.equal(await handOffDownload(item, ops), "oxyd");
  assert.deepEqual(calls, ["pause 7", "offer", "cancel 7", "erase 7"]);
});

test("resumes in the browser when Oxyd refuses or is unreachable", async () => {
  const { ops, calls } = fakeOps({ accepted: false });
  assert.equal(await handOffDownload(item, ops), "browser");
  assert.deepEqual(calls, ["pause 7", "offer", "resume 7"]);
});

test("downloads again only if the paused download cannot resume", async () => {
  const { ops, calls } = fakeOps({
    accepted: false,
    resume: async () => {
      throw new Error("interrupted");
    },
  });
  assert.equal(await handOffDownload(item, ops), "browser");
  assert.deepEqual(calls, ["pause 7", "offer", "redownload https://x.org/a.zip"]);
});

test("leaves downloads alone that already finished", async () => {
  const { ops, calls } = fakeOps({
    accepted: true,
    pause: async () => {
      throw new Error("not in progress");
    },
  });
  assert.equal(await handOffDownload(item, ops), "gone");
  assert.deepEqual(calls, []);
});
