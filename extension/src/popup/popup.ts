import { ping } from "../native.ts";
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
};
const EN: Record<string, string> = {
  connected: "Connected",
  offline: "App not running",
  saved: "Saved",
  skipArmed: "Next download goes to the browser",
};

const indonesian = chrome.i18n.getUILanguage().startsWith("id");
const tr = (key: string, fallback: string) => (indonesian ? ID[key] : EN[key]) ?? fallback;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const enabled = $<HTMLInputElement>("enabled");
const minSize = $<HTMLInputElement>("minSize");
const extensions = $<HTMLTextAreaElement>("extensions");
const excluded = $<HTMLTextAreaElement>("excluded");
const status = $("status");

for (const el of document.querySelectorAll<HTMLElement>("[data-i18n]")) {
  el.textContent = tr(el.dataset.i18n!, el.textContent ?? "");
}

function fill(s: typeof DEFAULTS) {
  enabled.checked = s.enabled;
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

enabled.addEventListener("change", () => void saveSettings({ enabled: enabled.checked }));

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
