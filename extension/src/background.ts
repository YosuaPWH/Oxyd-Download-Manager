import { api, firefox } from "./browser-api.ts";
// Service worker: hands browser downloads to Oxyd and keeps them in the browser when the
// app does not take them.
//
// MV3 service workers are stopped when idle, so state that must outlive a few seconds
// (the "skip next download" switch, URLs re-started in the browser) lives in
// api.storage.session instead of module variables.
import { shouldCapture } from "./filters.ts";
import { handOffDownload } from "./handoff.ts";
import type { ContentMessage } from "./messages.ts";
import { browserName, cookieHeader, sendNative, type AddDownload } from "./native.ts";
import { siteHost } from "./overlay.ts";
import { DESKTOP_CAPTURE_KEY, loadSettings, type EffectiveSettings, type ExtSettings } from "./settings.ts";
import { initSniffer } from "./sniffer.ts";
import { pageVideo, storeIdFor, videoDownload, videoOptions } from "./video.ts";

const MENU_ID = "oxyd-download-link";
const VIDEO_MENU_ID = "oxyd-download-video";

initSniffer();
let captureProfile: Promise<ExtSettings | null> | undefined;
let captureProfileUntil = 0;

function desktopProfile(force = false): Promise<ExtSettings | null> {
  if (!force && captureProfile && Date.now() < captureProfileUntil) return captureProfile;
  captureProfileUntil = Date.now() + 15_000;
  captureProfile = (async () => {
    const stored = (await api.storage.local.get(DESKTOP_CAPTURE_KEY))[DESKTOP_CAPTURE_KEY] as ExtSettings | undefined;
    try {
      const reply = await sendNative({ v: 1, type: "ping" }, 600);
      if (reply.type === "pong") {
        const next = reply.captureSettings ?? null;
        if (JSON.stringify(stored ?? null) !== JSON.stringify(next)) {
          await api.storage.local.set({ [DESKTOP_CAPTURE_KEY]: next });
        }
        return next;
      }
    } catch {
      // The desktop can be closed; use the last profile it supplied.
    }
    return stored ?? null;
  })();
  return captureProfile;
}

async function effectiveSettings(force = false): Promise<EffectiveSettings> {
  const desktop = await desktopProfile(force);
  return { settings: desktop ?? await loadSettings(), managed: desktop !== null };
}

const SKIP_MS = 30_000;
const BYPASS_MS = 60_000;

interface SessionState {
  /** Epoch ms until which the next download goes to the browser. */
  skipNextUntil: number;
  /** URLs re-started in the browser after a failed hand-off, with expiry (epoch ms). */
  bypass: Record<string, number>;
}

async function session(): Promise<SessionState> {
  const s = await api.storage.session.get({ skipNextUntil: 0, bypass: {} });
  const now = Date.now();
  const bypass = Object.fromEntries(
    Object.entries(s.bypass as Record<string, number>).filter(([, until]) => until > now),
  );
  return { skipNextUntil: s.skipNextUntil as number, bypass };
}

async function armSkipNext() {
  await api.storage.session.set({ skipNextUntil: Date.now() + SKIP_MS });
  void api.action.setBadgeText({ text: "skip" });
  void api.action.setBadgeBackgroundColor({ color: "#b45309" });
  // Alarms survive the worker being stopped, unlike timers.
  await api.alarms.create("skip-expired", { when: Date.now() + SKIP_MS + 500 });
}

async function clearSkipNext() {
  await api.storage.session.set({ skipNextUntil: 0 });
  void api.action.setBadgeText({ text: "" });
}

async function addBypass(url: string) {
  const s = await session();
  s.bypass[url] = Date.now() + BYPASS_MS;
  await api.storage.session.set({ bypass: s.bypass });
}

function flashBadge(text: string, color: string) {
  void api.action.setBadgeText({ text });
  void api.action.setBadgeBackgroundColor({ color });
  setTimeout(() => void api.action.setBadgeText({ text: "" }), 4000);
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
    await api.downloads.download({ url, filename: filename || undefined, conflictAction: "uniquify" });
  } catch (e) {
    console.error("browser download failed", e);
  }
}

function baseName(path: string): string | undefined {
  return path.split(/[\\/]/).pop() || undefined;
}

async function capture(item: chrome.downloads.DownloadItem) {
  const [{ settings }, state] = await Promise.all([effectiveSettings(), session()]);
  const decision = shouldCapture(item, settings, {
    selfId: api.runtime.id,
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
      pause: (id) => api.downloads.pause(id),
      resume: (id) => api.downloads.resume(id),
      cancel: (id) => api.downloads.cancel(id),
      erase: async (id) => void (await api.downloads.erase({ id })),
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
          cookies: await cookieHeader(url, (item as chrome.downloads.DownloadItem & { cookieStoreId?: string }).cookieStoreId ?? await storeIdFor(item.incognito)),
          source: browserName(),
        }),
    },
  );
}

// Firefox has no filename hook. Its download stays intact until the app accepts it.
if (firefox) {
  api.downloads.onCreated.addListener((item) => void capture(item).catch(console.warn));
} else {
  api.downloads.onDeterminingFilename.addListener((item, suggest) => {
    suggest();
    void capture(item).catch(console.warn);
  });
}

api.runtime.onInstalled.addListener(() => {
  const id = api.i18n.getUILanguage().startsWith("id");
  api.contextMenus.create({
    id: MENU_ID,
    title: id ? "Download dengan Oxyd" : "Download with Oxyd",
    contexts: ["link"],
  });
  api.contextMenus.create({
    id: VIDEO_MENU_ID,
    title: id ? "Unduh video dengan Oxyd" : "Download video with Oxyd",
    contexts: ["video", "audio"],
  });
});

/** Right-click on a video: the button's menu over it, else the app's video dialog. */
async function videoFromContextMenu(info: chrome.contextMenus.OnClickData, tab?: chrome.tabs.Tab) {
  if (tab?.id !== undefined) {
    const shown = await api.tabs
      .sendMessage(tab.id, { type: "open-video-menu" }, { frameId: info.frameId ?? 0 })
      .catch(() => false);
    if (shown) return;
  }
  const result = await pageVideo(info.frameUrl || info.pageUrl || tab?.url || "", tab);
  flashBadge(result.ok ? "✓" : "!", result.ok ? "#15803d" : "#c62828");
}

api.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === VIDEO_MENU_ID) return void videoFromContextMenu(info, tab);
  if (info.menuItemId !== MENU_ID || !info.linkUrl) return;
  const accepted = await offerToOxyd({
    v: 1,
    type: "add_download",
    id: crypto.randomUUID(),
    url: info.linkUrl,
    referrer: info.pageUrl || tab?.url,
    userAgent: navigator.userAgent,
    cookies: await cookieHeader(info.linkUrl, await storeIdFor(!!tab?.incognito, tab)),
    source: browserName(),
  });
  if (!accepted) await browserDownload(info.linkUrl);
});

api.commands.onCommand.addListener((command) => {
  if (command === "skip-next") void armSkipNext();
});

api.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name !== "skip-expired") return;
  if (Date.now() >= (await session()).skipNextUntil) void api.action.setBadgeText({ text: "" });
});

api.runtime.onMessage.addListener((msg, sender, respond) => {
  if (msg?.type === "skip-next") {
    void armSkipNext().then(() => respond(true));
    return true; // respond asynchronously
  }
  // Only this extension's own pages and content scripts send these.
  if (sender.id !== api.runtime.id) return false;
  if (msg?.type === "capture-settings") {
    void effectiveSettings(msg.refresh === true).then(respond, () => respond(undefined));
    return true;
  }
  const m = msg as ContentMessage | { type: "page-video"; tabId: number };
  if (m?.type === "video-overlay-site") {
    respond({ host: siteHost(sender.tab?.url) });
    return false;
  }
  if (m?.type === "video-options") {
    void videoOptions(m, sender.tab).then(respond);
    return true;
  }
  if (m?.type === "video-download") {
    void videoDownload(m, sender.tab).then(respond);
    return true;
  }
  if (m?.type === "page-video") {
    void api.tabs
      .get(m.tabId)
      .then((tab) => pageVideo(tab.url ?? "", tab))
      .then(respond, () => respond({ ok: false }));
    return true;
  }
  return false;
});
