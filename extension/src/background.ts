// Service worker: hands browser downloads to Oxyd and falls back to the browser when the
// app does not answer.
import { shouldCapture } from "./filters.ts";
import { browserName, cookieHeader, sendNative, type AddDownload } from "./native.ts";
import { loadSettings } from "./settings.ts";

const MENU_ID = "oxyd-download-link";
/** URLs we re-started in the browser after a failed hand-off; never capture them again. */
const bypass = new Set<string>();
/** Set by the keyboard shortcut or popup: let the next download through. */
let skipNextUntil = 0;

function skipNextActive(): boolean {
  return Date.now() < skipNextUntil;
}

function armSkipNext() {
  skipNextUntil = Date.now() + 30_000;
  void chrome.action.setBadgeText({ text: "skip" });
  void chrome.action.setBadgeBackgroundColor({ color: "#b45309" });
  setTimeout(() => {
    if (!skipNextActive()) void chrome.action.setBadgeText({ text: "" });
  }, 30_500);
}

function flashBadge(text: string, color: string) {
  void chrome.action.setBadgeText({ text });
  void chrome.action.setBadgeBackgroundColor({ color });
  setTimeout(() => void chrome.action.setBadgeText({ text: "" }), 4000);
}

async function handOff(msg: AddDownload): Promise<boolean> {
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
  return false;
}

/** Restart a download in the browser and remember not to capture it again. */
async function fallback(url: string, filename?: string) {
  bypass.add(url);
  setTimeout(() => bypass.delete(url), 60_000);
  flashBadge("!", "#c62828");
  try {
    await chrome.downloads.download({
      url,
      filename: filename || undefined,
      conflictAction: "uniquify",
    });
  } catch (e) {
    console.error("browser fallback failed", e);
  }
}

function baseName(path: string): string | undefined {
  return path.split(/[\\/]/).pop() || undefined;
}

async function capture(item: chrome.downloads.DownloadItem) {
  const settings = await loadSettings();
  const decision = shouldCapture(item, settings, {
    selfId: chrome.runtime.id,
    bypass,
    skipNext: skipNextActive(),
  });
  if (!decision.capture) {
    if (decision.reason === "skip-next") {
      skipNextUntil = 0;
      void chrome.action.setBadgeText({ text: "" });
    }
    return;
  }

  // Stop the browser download first so it does not compete for bandwidth.
  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    return; // Already finished or removed: leave it alone.
  }

  const url = item.finalUrl || item.url;
  const filename = baseName(item.filename);
  const msg: AddDownload = {
    v: 1,
    type: "add_download",
    id: crypto.randomUUID(),
    url,
    filename,
    size: item.totalBytes > 0 ? item.totalBytes : undefined,
    mime: item.mime || undefined,
    referrer: item.referrer || undefined,
    userAgent: navigator.userAgent,
    cookies: await cookieHeader(url),
    source: browserName(),
  };
  if (!(await handOff(msg))) await fallback(url, filename);
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
  const msg: AddDownload = {
    v: 1,
    type: "add_download",
    id: crypto.randomUUID(),
    url: info.linkUrl,
    referrer: info.pageUrl || tab?.url,
    userAgent: navigator.userAgent,
    cookies: await cookieHeader(info.linkUrl),
    source: browserName(),
  };
  if (!(await handOff(msg))) await fallback(info.linkUrl);
});

chrome.commands.onCommand.addListener((command) => {
  if (command === "skip-next") armSkipNext();
});

chrome.runtime.onMessage.addListener((msg, _sender, respond) => {
  if (msg?.type === "skip-next") {
    armSkipNext();
    respond(true);
  }
});
