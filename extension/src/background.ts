// Service worker: hands browser downloads to Oxyd and keeps them in the browser when the
// app does not take them.
//
// MV3 service workers are stopped when idle, so state that must outlive a few seconds
// (the "skip next download" switch, URLs re-started in the browser) lives in
// chrome.storage.session instead of module variables.
import { shouldCapture } from "./filters.ts";
import { handOffDownload } from "./handoff.ts";
import type { ContentMessage } from "./messages.ts";
import { browserName, cookieHeader, sendNative, type AddDownload } from "./native.ts";
import { loadSettings } from "./settings.ts";
import { initSniffer } from "./sniffer.ts";
import { pageVideo, storeIdFor, videoDownload, videoOptions } from "./video.ts";

const MENU_ID = "oxyd-download-link";
const VIDEO_MENU_ID = "oxyd-download-video";

initSniffer();
const SKIP_MS = 30_000;
const BYPASS_MS = 60_000;

interface SessionState {
  /** Epoch ms until which the next download goes to the browser. */
  skipNextUntil: number;
  /** URLs re-started in the browser after a failed hand-off, with expiry (epoch ms). */
  bypass: Record<string, number>;
}

async function session(): Promise<SessionState> {
  const s = await chrome.storage.session.get({ skipNextUntil: 0, bypass: {} });
  const now = Date.now();
  const bypass = Object.fromEntries(
    Object.entries(s.bypass as Record<string, number>).filter(([, until]) => until > now),
  );
  return { skipNextUntil: s.skipNextUntil as number, bypass };
}

async function armSkipNext() {
  await chrome.storage.session.set({ skipNextUntil: Date.now() + SKIP_MS });
  void chrome.action.setBadgeText({ text: "skip" });
  void chrome.action.setBadgeBackgroundColor({ color: "#b45309" });
  // Alarms survive the worker being stopped, unlike timers.
  await chrome.alarms.create("skip-expired", { when: Date.now() + SKIP_MS + 500 });
}

async function clearSkipNext() {
  await chrome.storage.session.set({ skipNextUntil: 0 });
  void chrome.action.setBadgeText({ text: "" });
}

async function addBypass(url: string) {
  const s = await session();
  s.bypass[url] = Date.now() + BYPASS_MS;
  await chrome.storage.session.set({ bypass: s.bypass });
}

function flashBadge(text: string, color: string) {
  void chrome.action.setBadgeText({ text });
  void chrome.action.setBadgeBackgroundColor({ color });
  setTimeout(() => void chrome.action.setBadgeText({ text: "" }), 4000);
}

async function offerToOxyd(msg: AddDownload): Promise<boolean> {
  try {
    const reply = await sendNative(msg);
    if (reply.type === "ack" && reply.status === "accepted") {
      flashBadge("✓", "#15803d");
      return true;
    }
    console.warn("Oxyd refused download", reply);
  } catch (e) {
    console.warn("Oxyd unreachable", e);
  }
  flashBadge("!", "#c62828");
  return false;
}

/** Start `url` in the browser and remember not to capture it again. */
async function browserDownload(url: string, filename?: string) {
  await addBypass(url);
  try {
    await chrome.downloads.download({ url, filename: filename || undefined, conflictAction: "uniquify" });
  } catch (e) {
    console.error("browser download failed", e);
  }
}

function baseName(path: string): string | undefined {
  return path.split(/[\\/]/).pop() || undefined;
}

async function capture(item: chrome.downloads.DownloadItem) {
  const [settings, state] = await Promise.all([loadSettings(), session()]);
  const decision = shouldCapture(item, settings, {
    selfId: chrome.runtime.id,
    bypass: new Set(Object.keys(state.bypass)),
    skipNext: Date.now() < state.skipNextUntil,
  });
  if (!decision.capture) {
    if (decision.reason === "skip-next") await clearSkipNext();
    return;
  }

  const url = item.finalUrl || item.url;
  const filename = baseName(item.filename);
  await handOffDownload(
    { id: item.id, url, filename },
    {
      pause: (id) => chrome.downloads.pause(id),
      resume: (id) => chrome.downloads.resume(id),
      cancel: (id) => chrome.downloads.cancel(id),
      erase: async (id) => void (await chrome.downloads.erase({ id })),
      redownload: (u, name) => browserDownload(u, name),
      offer: async () =>
        offerToOxyd({
          v: 1,
          type: "add_download",
          id: crypto.randomUUID(),
          url,
          filename,
          size: item.totalBytes > 0 ? item.totalBytes : undefined,
          mime: item.mime || undefined,
          referrer: item.referrer || undefined,
          userAgent: navigator.userAgent,
          cookies: await cookieHeader(url, await storeIdFor(item.incognito)),
          source: browserName(),
        }),
    },
  );
}

// Wait until Chrome has worked out the file name (Content-Disposition) before deciding.
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  suggest();
  void capture(item);
});

chrome.runtime.onInstalled.addListener(() => {
  const id = chrome.i18n.getUILanguage().startsWith("id");
  chrome.contextMenus.create({
    id: MENU_ID,
    title: id ? "Download dengan Oxyd" : "Download with Oxyd",
    contexts: ["link"],
  });
  chrome.contextMenus.create({
    id: VIDEO_MENU_ID,
    title: id ? "Unduh video dengan Oxyd" : "Download video with Oxyd",
    contexts: ["video", "audio"],
  });
});

/** Right-click on a video: the button's menu over it, else the app's video dialog. */
async function videoFromContextMenu(info: chrome.contextMenus.OnClickData, tab?: chrome.tabs.Tab) {
  if (tab?.id !== undefined) {
    const shown = await chrome.tabs
      .sendMessage(tab.id, { type: "open-video-menu" }, { frameId: info.frameId ?? 0 })
      .catch(() => false);
    if (shown) return;
  }
  const result = await pageVideo(info.frameUrl || info.pageUrl || tab?.url || "", tab);
  flashBadge(result.ok ? "✓" : "!", result.ok ? "#15803d" : "#c62828");
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === VIDEO_MENU_ID) return void videoFromContextMenu(info, tab);
  if (info.menuItemId !== MENU_ID || !info.linkUrl) return;
  const accepted = await offerToOxyd({
    v: 1,
    type: "add_download",
    id: crypto.randomUUID(),
    url: info.linkUrl,
    referrer: info.pageUrl || tab?.url,
    userAgent: navigator.userAgent,
    cookies: await cookieHeader(info.linkUrl, await storeIdFor(!!tab?.incognito)),
    source: browserName(),
  });
  if (!accepted) await browserDownload(info.linkUrl);
});

chrome.commands.onCommand.addListener((command) => {
  if (command === "skip-next") void armSkipNext();
});

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name !== "skip-expired") return;
  if (Date.now() >= (await session()).skipNextUntil) void chrome.action.setBadgeText({ text: "" });
});

chrome.runtime.onMessage.addListener((msg, sender, respond) => {
  if (msg?.type === "skip-next") {
    void armSkipNext().then(() => respond(true));
    return true; // respond asynchronously
  }
  // Only this extension's own pages and content scripts send these.
  if (sender.id !== chrome.runtime.id) return false;
  const m = msg as ContentMessage | { type: "page-video"; tabId: number };
  if (m?.type === "video-options") {
    void videoOptions(m, sender.tab).then(respond);
    return true;
  }
  if (m?.type === "video-download") {
    void videoDownload(m, sender.tab).then(respond);
    return true;
  }
  if (m?.type === "page-video") {
    void chrome.tabs
      .get(m.tabId)
      .then((tab) => pageVideo(tab.url ?? "", tab))
      .then(respond, () => respond({ ok: false }));
    return true;
  }
  return false;
});
