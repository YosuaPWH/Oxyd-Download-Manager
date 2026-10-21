// Service worker: hands browser downloads to Oxyd and keeps them in the browser when the
// app does not take them.
//
// MV3 service workers are stopped when idle, so state that must outlive a few seconds
// (the "skip next download" switch, URLs re-started in the browser) lives in
// chrome.storage.session instead of module variables.
import { shouldCapture } from "./filters.ts";
import { handOffDownload } from "./handoff.ts";
import { browserName, cookieHeader, sendNative, type AddDownload } from "./native.ts";
import { loadSettings } from "./settings.ts";

const MENU_ID = "oxyd-download-link";
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

/** Cookie store of the incognito profile when `incognito`, default store otherwise. */
async function storeIdFor(incognito: boolean): Promise<string | undefined> {
  if (!incognito) return undefined;
  try {
    const stores = await chrome.cookies.getAllCookieStores();
    return stores.find((s) => s.id !== "0")?.id;
  } catch {
    return undefined;
  }
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
  chrome.contextMenus.create({
    id: MENU_ID,
    title: chrome.i18n.getUILanguage().startsWith("id")
      ? "Download dengan Oxyd"
      : "Download with Oxyd",
    contexts: ["link"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
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

chrome.runtime.onMessage.addListener((msg, _sender, respond) => {
  if (msg?.type === "skip-next") {
    void armSkipNext().then(() => respond(true));
    return true; // respond asynchronously
  }
});
