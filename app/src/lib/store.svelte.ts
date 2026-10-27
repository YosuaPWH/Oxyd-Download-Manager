import { api, errorMessage, onClipboard, onEngineEvent, onPending, onRefresh, onResync } from "./api";
import { errorText, i18n, t } from "./i18n.svelte";
import type { Category, DownloadInfo, EngineEvent, PendingAdd, Settings, Status } from "./types";

/** Status tabs of the download list. */
export type View = "all" | "active" | "paused" | "failed" | "completed";

/** Ticks every second for countdowns. */
export const clock = $state({ now: Math.floor(Date.now() / 1000) });

export const app = $state({
  downloads: [] as DownloadInfo[],
  settings: null as Settings | null,
  selected: null as string | null,
  page: "downloads" as "downloads" | "settings",
  view: "all" as View,
  /** File type filter next to the search box; null = every type. */
  category: null as Category | null,
  /** Text typed in the search box. */
  search: "",
  pending: [] as PendingAdd[],
  /** URL for a manual "New download" dialog; null = closed. */
  manualUrl: null as string | null,
  clipboardUrl: null as string | null,
  removing: null as DownloadInfo | null,
  error: null as string | null,
  /** Short confirmation shown at the bottom right. */
  notice: null as string | null,
  /** Download waiting for its new link from the browser. */
  refreshing: null as string | null,
  version: "",
});

const ACTIVE: Status[] = ["queued", "probing", "downloading"];

export function isActive(d: DownloadInfo): boolean {
  return ACTIVE.includes(d.status);
}

export function visible(d: DownloadInfo, view: View): boolean {
  switch (view) {
    case "all":
      return true;
    case "active":
      return isActive(d);
    case "paused":
      return d.status === "paused";
    case "failed":
      return d.status === "failed" || d.status === "cancelled";
    case "completed":
      return d.status === "completed";
  }
}

/** Whether `d` matches the search text (file name or URL). */
export function matchesSearch(d: DownloadInfo, text: string): boolean {
  const q = text.trim().toLowerCase();
  return !q || d.filename.toLowerCase().includes(q) || d.url.toLowerCase().includes(q);
}

export function selected(): DownloadInfo | undefined {
  return app.downloads.find((d) => d.id === app.selected);
}

const byQueueOrder = (a: DownloadInfo, b: DownloadInfo) =>
  a.position - b.position || a.createdAt - b.createdAt;

function upsert(d: DownloadInfo) {
  const i = app.downloads.findIndex((x) => x.id === d.id);
  // Status updates are frequent; only re-sort when the queue order actually changed.
  if (i >= 0 && app.downloads[i].position === d.position) {
    app.downloads[i] = d;
    return;
  }
  if (i >= 0) app.downloads[i] = d;
  else app.downloads.push(d);
  app.downloads.sort(byQueueOrder);
}

function apply(e: EngineEvent) {
  switch (e.type) {
    case "added":
    case "updated":
    case "completed":
    case "failed":
      upsert(e.download);
      break;
    case "progress": {
      const d = app.downloads.find((x) => x.id === e.id);
      if (d) {
        d.downloaded = e.downloaded;
        d.totalSize = e.total;
        d.speed = e.speed;
        d.eta = e.eta;
        d.activeConnections = e.activeConnections;
        d.segments = e.segments;
      }
      break;
    }
    case "removed":
      app.downloads = app.downloads.filter((x) => x.id !== e.id);
      if (app.selected === e.id) app.selected = null;
      break;
    case "queueFinished":
      break;
  }
}

export async function refresh() {
  app.downloads = await api.list();
}

export function applyTheme(s: Settings) {
  const root = document.documentElement;
  if (s.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = s.theme;
  i18n.lang = s.language;
  root.lang = s.language;
}

export async function saveSettings(s: Settings) {
  app.settings = await api.setSettings(s);
  applyTheme(app.settings);
}

export async function init() {
  setInterval(() => (clock.now = Math.floor(Date.now() / 1000)), 1000);
  await Promise.all([
    onEngineEvent(apply),
    onPending((p) => {
      if (!app.pending.some((x) => x.key === p.key)) app.pending.push(p);
    }),
    onClipboard((url) => (app.clipboardUrl = url)),
    onResync(() => void refresh()),
    onRefresh((r) => {
      app.refreshing = null;
      if (r.ok) app.notice = t("refresh.done");
      else app.error = errorText(r.code) ?? r.message ?? t("err.title");
    }),
  ]);
  const [settings, list, pending, info, refreshing] = await Promise.all([
    api.getSettings(),
    api.list(),
    api.pending(),
    api.appInfo(),
    api.refreshTarget(),
  ]);
  app.refreshing = refreshing;
  app.settings = settings;
  applyTheme(settings);
  app.downloads = list;
  app.version = info.version;
  for (const p of pending) if (!app.pending.some((x) => x.key === p.key)) app.pending.push(p);
}

/** Wait for the next link clicked in the browser and use it for download `d`. */
export async function beginRefresh(d: DownloadInfo) {
  await run(async () => {
    await api.beginRefresh(d.id);
    app.refreshing = d.id;
  });
}

export async function cancelRefresh() {
  await run(api.cancelRefresh);
  app.refreshing = null;
}

/** Run an action and surface its error in the UI. */
export async function run(action: () => Promise<unknown>) {
  try {
    await action();
  } catch (e) {
    app.error = errorMessage(e);
  }
}
