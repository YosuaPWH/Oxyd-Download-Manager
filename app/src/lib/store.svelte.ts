import { api, errorMessage, onClipboard, onEngineEvent, onPending, onResync } from "./api";
import { i18n } from "./i18n.svelte";
import type { Category, DownloadInfo, EngineEvent, PendingAdd, Settings, Status } from "./types";

export type View = "all" | "active" | "unfinished" | "completed" | Category;

/** Ticks every second for countdowns. */
export const clock = $state({ now: Math.floor(Date.now() / 1000) });

export const app = $state({
  downloads: [] as DownloadInfo[],
  settings: null as Settings | null,
  selected: null as string | null,
  view: "all" as View,
  pending: [] as PendingAdd[],
  /** URL for a manual "New download" dialog; null = closed. */
  manualUrl: null as string | null,
  clipboardUrl: null as string | null,
  settingsOpen: false,
  removing: null as DownloadInfo | null,
  error: null as string | null,
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
    case "unfinished":
      return d.status !== "completed";
    case "completed":
      return d.status === "completed";
    default:
      return d.category === view;
  }
}

export function selected(): DownloadInfo | undefined {
  return app.downloads.find((d) => d.id === app.selected);
}

function upsert(d: DownloadInfo) {
  const i = app.downloads.findIndex((x) => x.id === d.id);
  if (i >= 0) app.downloads[i] = d;
  else app.downloads.push(d);
  app.downloads.sort((a, b) => a.position - b.position || a.createdAt - b.createdAt);
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
  ]);
  const [settings, list, pending, info] = await Promise.all([
    api.getSettings(),
    api.list(),
    api.pending(),
    api.appInfo(),
  ]);
  app.settings = settings;
  applyTheme(settings);
  app.downloads = list;
  app.version = info.version;
  for (const p of pending) if (!app.pending.some((x) => x.key === p.key)) app.pending.push(p);
}

/** Run an action and surface its error in the UI. */
export async function run(action: () => Promise<unknown>) {
  try {
    await action();
  } catch (e) {
    app.error = errorMessage(e);
  }
}
