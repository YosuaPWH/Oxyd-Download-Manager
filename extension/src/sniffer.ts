// Remembers the media streams each tab loaded (HLS/DASH manifests and video/audio files), for
// pages yt-dlp does not know. Kept in storage.session: the service worker is stopped when idle.
import { addStream, classifyMedia, isVideoSite, type SniffedStream } from "./media.ts";

const key = (tabId: number) => `media:${tabId}`;
const tabs = new Map<number, SniffedStream[]>();
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export async function tabStreams(tabId: number): Promise<SniffedStream[]> {
  const mem = tabs.get(tabId);
  if (mem) return mem;
  const stored = await chrome.storage.session.get(key(tabId)).catch(() => ({}) as Record<string, unknown>);
  const list = (stored[key(tabId)] as SniffedStream[] | undefined) ?? [];
  tabs.set(tabId, list);
  return list;
}

function persist(tabId: number) {
  clearTimeout(timers.get(tabId));
  timers.set(
    tabId,
    setTimeout(() => {
      timers.delete(tabId);
      const list = tabs.get(tabId) ?? [];
      void (list.length
        ? chrome.storage.session.set({ [key(tabId)]: list })
        : chrome.storage.session.remove(key(tabId))
      ).catch(() => {});
    }, 300),
  );
}

function header(h: chrome.webRequest.HttpHeader[] | undefined, name: string): string | undefined {
  return h?.find((x) => x.name.toLowerCase() === name)?.value;
}

/** Full size from `Content-Range: bytes a-b/total`, else `Content-Length` of a 200. */
function totalSize(status: number, h: chrome.webRequest.HttpHeader[] | undefined): number | undefined {
  const range = header(h, "content-range");
  const n = range ? Number(range.split("/")[1]) : status === 200 ? Number(header(h, "content-length")) : NaN;
  return Number.isFinite(n) && n > 0 ? n : undefined;
}

export function initSniffer() {
  chrome.webRequest.onHeadersReceived.addListener(
    (d): undefined => {
      if (d.tabId < 0 || d.statusCode < 200 || d.statusCode >= 300) return;
      // yt-dlp reads these sites itself; their players load hundreds of pieces.
      if (d.initiator && isVideoSite(d.initiator)) return;
      if (/\.googlevideo\.com$/.test(new URL(d.url).hostname)) return;
      const contentType = header(d.responseHeaders, "content-type");
      const size = totalSize(d.statusCode, d.responseHeaders);
      const kind = classifyMedia({ url: d.url, contentType, size });
      if (!kind) return;
      void tabStreams(d.tabId).then((list) => {
        tabs.set(
          d.tabId,
          addStream(list, {
            url: d.url,
            kind,
            size: kind === "file" ? size : undefined,
            mime: contentType?.split(";")[0].trim(),
            seen: Date.now(),
          }),
        );
        persist(d.tabId);
      });
    },
    { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other"] },
    ["responseHeaders"],
  );
  // A new page in the tab: forget the old page's streams.
  chrome.webRequest.onBeforeRequest.addListener(
    (d): undefined => {
      if (d.tabId < 0) return;
      tabs.set(d.tabId, []);
      persist(d.tabId);
    },
    { urls: ["<all_urls>"], types: ["main_frame"] },
  );
  chrome.tabs.onRemoved.addListener((tabId) => {
    tabs.delete(tabId);
    void chrome.storage.session.remove(key(tabId)).catch(() => {});
  });
}
