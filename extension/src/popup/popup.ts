import { ping } from "../native.ts";
import { layoutOverride, OVERLAY_KEY, overlaySettings, siteHost, siteLayout } from "../overlay.ts";
import { DEFAULTS, loadSettings, parseList, saveSettings } from "../settings.ts";

const ID: Record<string, string> = {
  capture: "Tangkap download",
  skip: "Biarkan download berikutnya ke browser",
  filters: "Filter",
  minSize: "Ukuran minimum (MB)",
  extensions: "Jenis file",
  excluded: "Jangan tangkap dari domain ini",
  reset: "Reset",
  save: "Simpan",
  connected: "Terhubung",
  offline: "Aplikasi tidak berjalan",
  saved: "Tersimpan",
  skipArmed: "Download berikutnya ke browser",
  videoButton: "Tampilkan tombol unduh di video",
  videoOverlay: "Tampilan tombol video",
  defaultSize: "Ukuran bawaan",
  defaultCorner: "Sudut bawaan",
  siteSize: "Ukuran di situs ini",
  siteCorner: "Sudut di situs ini",
  standard: "Standar",
  compact: "Hanya ikon",
  useDefault: "Pakai bawaan",
  topRight: "Kanan atas",
  topLeft: "Kiri atas",
  bottomRight: "Kanan bawah",
  bottomLeft: "Kiri bawah",
  saveLayout: "Simpan tampilan",
  layoutFailed: "Gagal menyimpan tampilan",
  loadLayoutFailed: "Gagal memuat tampilan",
  pageVideo: "Unduh video di halaman ini",
  sent: "Dikirim ke Oxyd",
  failed: "Gagal mengirim ke Oxyd",
};
const EN: Record<string, string> = {
  connected: "Connected",
  offline: "App not running",
  saved: "Saved",
  skipArmed: "Next download goes to the browser",
  sent: "Sent to Oxyd",
  failed: "Could not send it to Oxyd",
};

const indonesian = chrome.i18n.getUILanguage().startsWith("id");
const tr = (key: string, fallback: string) => (indonesian ? ID[key] : EN[key]) ?? fallback;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const enabled = $<HTMLInputElement>("enabled");
const minSize = $<HTMLInputElement>("minSize");
const extensions = $<HTMLTextAreaElement>("extensions");
const excluded = $<HTMLTextAreaElement>("excluded");
const videoButton = $<HTMLInputElement>("videoButton");
const status = $("status");
const defaultSize = $<HTMLSelectElement>("defaultSize");
const defaultCorner = $<HTMLSelectElement>("defaultCorner");
const siteSize = $<HTMLSelectElement>("siteSize");
const siteCorner = $<HTMLSelectElement>("siteCorner");
const saveOverlay = $<HTMLButtonElement>("saveOverlay");
const overlayStatus = $("overlayStatus");
let activeHost: string | undefined;

for (const el of document.querySelectorAll<HTMLElement>("[data-i18n]")) {
  el.textContent = tr(el.dataset.i18n!, el.textContent ?? "");
}

function fill(s: typeof DEFAULTS) {
  enabled.checked = s.enabled;
  videoButton.checked = s.videoButton;
  minSize.value = String(Math.round((s.minSize / 1024 / 1024) * 10) / 10);
  extensions.value = s.extensions.join(" ");
  excluded.value = s.excludedDomains.join("\n");
}

async function init() {
  fill(await loadSettings());
  const version = await ping();
  status.className = `status ${version ? "ok" : "bad"}`;
  status.textContent = version ? `${tr("connected", "Connected")} · v${version}` : tr("offline", "App not running");
}

async function initOverlay() {
  const [stored, [tab]] = await Promise.all([
    chrome.storage.local.get(OVERLAY_KEY),
    chrome.tabs.query({ active: true, currentWindow: true }),
  ]);
  const prefs = overlaySettings(stored[OVERLAY_KEY]);
  defaultSize.value = prefs.defaults.compact ? "compact" : "standard";
  defaultCorner.value = prefs.defaults.corner;
  activeHost = siteHost(tab?.url);
  if (activeHost) {
    const rule = siteLayout(prefs, activeHost);
    siteSize.value = rule.compact === undefined ? "" : rule.compact ? "compact" : "standard";
    siteCorner.value = rule.corner ?? "";
    $("overlayHost").textContent = activeHost;
    $("siteOverlay").hidden = false;
  }
  saveOverlay.disabled = false;
}

$("overlayForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  saveOverlay.disabled = true;
  try {
    const stored = await chrome.storage.local.get(OVERLAY_KEY);
    const prefs = overlaySettings(stored[OVERLAY_KEY]);
    prefs.defaults = { ...prefs.defaults, ...layoutOverride({ compact: defaultSize.value === "compact", corner: defaultCorner.value }) };
    if (activeHost) {
      const rule = layoutOverride({ compact: siteSize.value ? siteSize.value === "compact" : undefined, corner: siteCorner.value });
      if (rule.compact !== undefined || rule.corner) prefs.sites = { ...prefs.sites, [activeHost]: rule };
      else delete prefs.sites[activeHost];
    }
    await chrome.storage.local.set({ [OVERLAY_KEY]: prefs });
    overlayStatus.textContent = tr("saved", "Saved");
  } catch {
    overlayStatus.textContent = tr("layoutFailed", "Could not save the layout");
  } finally {
    saveOverlay.disabled = false;
  }
});

enabled.addEventListener("change", () => void saveSettings({ enabled: enabled.checked }));
videoButton.addEventListener("change", () => void saveSettings({ videoButton: videoButton.checked }));

$("pageVideo").addEventListener("click", async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab?.id === undefined) return;
  const button = $("pageVideo") as HTMLButtonElement;
  button.disabled = true;
  const r = (await chrome.runtime.sendMessage({ type: "page-video", tabId: tab.id })) as { ok: boolean } | undefined;
  button.textContent = r?.ok ? tr("sent", "Sent to Oxyd") : tr("failed", "Could not send it to Oxyd");
  button.disabled = false;
});

$("save").addEventListener("click", async () => {
  await saveSettings({
    minSize: Math.max(0, Math.round(parseFloat(minSize.value || "0") * 1024 * 1024)),
    extensions: parseList(extensions.value, true),
    excludedDomains: parseList(excluded.value),
  });
  $("save").textContent = tr("saved", "Saved");
  setTimeout(() => ($("save").textContent = tr("save", "Save")), 1200);
});

$("reset").addEventListener("click", async () => {
  await saveSettings(DEFAULTS);
  fill(DEFAULTS);
});

$("skip").addEventListener("click", async () => {
  await chrome.runtime.sendMessage({ type: "skip-next" });
  $("skip").textContent = tr("skipArmed", "Next download goes to the browser");
});

void init();
void initOverlay().catch(() => {
  overlayStatus.textContent = tr("loadLayoutFailed", "Could not load the layout");
});
