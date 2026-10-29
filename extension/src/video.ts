// Service worker side of the video button: asks Oxyd (yt-dlp) for a page's qualities, falls
// back to the streams the tab loaded, and sends the chosen one to Oxyd.
import { isVideoSite, streamLabel, type SniffedStream } from "./media.ts";
import type { DownloadResult, MenuEntry, VideoOptions } from "./messages.ts";
import { VIDEO_TIMEOUT_MS, browserName, cookieHeader, sendNative, type Reply } from "./native.ts";
import { tabStreams } from "./sniffer.ts";

const indonesian = chrome.i18n.getUILanguage().startsWith("id");
const TEXT = indonesian
  ? {
      offline: "Oxyd tidak berjalan. Klik lagi untuk memulainya.",
      unreachable: "Oxyd tidak bisa dihubungi. Pastikan aplikasinya terpasang.",
      live: "Siaran langsung tidak bisa diunduh.",
      tool: "Oxyd belum bisa menyiapkan yt-dlp",
      slow: "Oxyd terlalu lama membaca halaman ini.",
      fallback: "Menampilkan stream yang dimuat halaman ini.",
    }
  : {
      offline: "Oxyd is not running. Click again to start it.",
      unreachable: "Oxyd cannot be reached. Make sure the app is installed.",
      live: "Live streams cannot be downloaded.",
      tool: "Oxyd could not prepare yt-dlp",
      slow: "Oxyd took too long to read this page.",
      fallback: "Showing the streams this page loaded.",
    };

/** Cookie store of the incognito profile when `incognito`, default store otherwise. */
export async function storeIdFor(incognito: boolean): Promise<string | undefined> {
  if (!incognito) return undefined;
  try {
    const stores = await chrome.cookies.getAllCookieStores();
    return stores.find((s) => s.id !== "0")?.id;
  } catch {
    return undefined;
  }
}

function streamEntries(streams: SniffedStream[], src: string | undefined): MenuEntry[] {
  const list = [...streams];
  // The hovered video's own file first, also when the page did not load it through the network
  // stack we see (cached).
  if (src && !list.some((s) => s.url === src)) list.unshift({ url: src, kind: "file", seen: Date.now() });
  list.sort((a, b) => Number(b.url === src) - Number(a.url === src) || (b.size ?? 0) - (a.size ?? 0));
  return list.map((s, i) => ({ id: `s${i}`, group: "stream", ...streamLabel(s), stream: s }));
}

/** What the app said, as a line for the menu. */
function notice(r: Reply | Error): { text: string; offline?: boolean } {
  if (r instanceof Error) {
    return /time/i.test(r.message) ? { text: TEXT.slow } : { text: TEXT.unreachable };
  }
  if (r.type !== "ack") return { text: TEXT.unreachable };
  if (/not reachable/i.test(r.message ?? "")) return { text: TEXT.offline, offline: true };
  if (r.code === "tool") return { text: `${TEXT.tool}: ${r.message ?? ""}` };
  return { text: r.message ?? TEXT.unreachable };
}

export async function videoOptions(
  req: { pageUrl: string; title: string; src?: string; launch: boolean },
  tab: chrome.tabs.Tab | undefined,
): Promise<VideoOptions> {
  const streams = tab?.id !== undefined ? await tabStreams(tab.id) : [];
  const page = streamEntries(streams, req.src);
  // Pages of known sites go to yt-dlp; so do others where the page loaded nothing we can use
  // (yt-dlp knows players embedded in many pages).
  if (isVideoSite(req.pageUrl) || !page.length) {
    let reply: Reply | Error;
    try {
      reply = await sendNative(
        {
          v: 1,
          type: "probe_video",
          id: crypto.randomUUID(),
          pageUrl: req.pageUrl,
          cookies: await cookieHeader(req.pageUrl, await storeIdFor(!!tab?.incognito)),
          userAgent: navigator.userAgent,
          referrer: tab?.url,
          launch: req.launch,
        },
        VIDEO_TIMEOUT_MS,
      );
    } catch (e) {
      reply = e as Error;
    }
    if (!(reply instanceof Error) && reply.type === "video_info") {
      const entries: MenuEntry[] = reply.choices.map((c) => ({
        id: c.id,
        group: c.kind,
        label: c.label,
        detail: [c.detail, c.size ? `~${formatSize(c.size)}` : ""].filter(Boolean).join(" · "),
        choice: c.id,
      }));
      if (reply.live) return { title: reply.title, pageUrl: req.pageUrl, entries: [], notice: TEXT.live };
      if (entries.length) return { title: reply.title, pageUrl: req.pageUrl, entries };
    }
    const n = notice(reply);
    return {
      title: req.title,
      pageUrl: req.pageUrl,
      entries: page,
      notice: page.length ? `${n.text} ${TEXT.fallback}` : n.text,
      offline: n.offline,
    };
  }
  return { title: req.title, pageUrl: req.pageUrl, entries: page };
}

function formatSize(n: number): string {
  const mb = n / 1024 / 1024;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb >= 100 ? mb.toFixed(0) : mb.toFixed(1)} MB`;
}

/** Send what was picked in the menu to Oxyd. */
export async function videoDownload(
  req: { pageUrl: string; title: string; entry: MenuEntry },
  tab: chrome.tabs.Tab | undefined,
): Promise<DownloadResult> {
  const store = await storeIdFor(!!tab?.incognito);
  const stream = req.entry.stream;
  let msg: object;
  if (stream?.kind === "file") {
    // A plain file: an ordinary download.
    msg = {
      v: 1,
      type: "add_download",
      id: crypto.randomUUID(),
      url: stream.url,
      size: stream.size,
      mime: stream.mime,
      referrer: req.pageUrl,
      userAgent: navigator.userAgent,
      cookies: await cookieHeader(stream.url, store),
      source: browserName(),
    };
  } else {
    // A quality yt-dlp found, or a manifest for yt-dlp to read (the app asks the quality).
    const pageUrl = stream ? stream.url : req.pageUrl;
    msg = {
      v: 1,
      type: "add_video",
      id: crypto.randomUUID(),
      pageUrl,
      choice: req.entry.choice,
      title: req.title,
      referrer: stream ? req.pageUrl : tab?.url,
      userAgent: navigator.userAgent,
      cookies: await cookieHeader(pageUrl, store),
      source: browserName(),
    };
  }
  try {
    const reply = await sendNative(msg, VIDEO_TIMEOUT_MS);
    if (reply.type === "ack" && reply.status === "accepted") return { ok: true };
    return { ok: false, error: reply.type === "ack" ? reply.message : undefined };
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}

/** The tab's page as a video (popup button, or right-click where no button runs). */
export async function pageVideo(pageUrl: string, tab: chrome.tabs.Tab | undefined): Promise<DownloadResult> {
  return videoDownload(
    { pageUrl, title: tab?.title ?? "", entry: { id: "page", group: "video", label: "", detail: "" } },
    tab,
  );
}
