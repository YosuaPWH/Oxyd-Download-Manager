import { api, errorMessage, onClipboard, onEngineEvent, onPending, onRefresh, onResync } from "./api";
import { errorText, i18n, t } from "./i18n.svelte";
import { playDone } from "./sound";
import type {
  Category,
  DownloadInfo,
  EngineEvent,
  JobStatus,
  PendingAdd,
  Queue,
  Settings,
  Status,
  VideoProbe,
} from "./types";

/** Status filter above the download list. */
export type View = "all" | "active" | "paused" | "failed" | "completed";
/** Sidebar scope. */
export type Library = "all" | "unfinished" | "finished";
export type SortKey = "queue" | "name" | "size" | "status" | "progress" | "speed" | "eta" | "added";
export type InspectorTab = "connections" | "speed" | "info";

/** False in the mini window and in progress windows, which show no dialogs and make no sound. */
const windowQuery = new URLSearchParams(location.search);
export const isMainWindow = !windowQuery.has("mini") && !windowQuery.has("progress");

/** Ticks every second for countdowns. */
export const clock = $state({ now: Math.floor(Date.now() / 1000) });

// What the window remembers between launches. Stored per machine, not in the settings:
// it is how this window was left, not a preference.
interface Layout {
  sort: { key: SortKey; dir: 1 | -1 };
  inspector: boolean;
  inspectorTab: InspectorTab;
  lastLimit: number;
}
const LAYOUT_KEY = "oxyd.layout";
function loadLayout(): Layout {
  const base: Layout = {
    sort: { key: "queue", dir: 1 },
    inspector: true,
    inspectorTab: "connections",
    lastLimit: 1024 * 1024,
  };
  try {
    return { ...base, ...(JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? "{}") as Partial<Layout>) };
  } catch {
    return base;
  }
}
const layout = loadLayout();

export const app = $state({
  downloads: [] as DownloadInfo[],
  settings: null as Settings | null,
  /** The download the inspector shows and the keyboard moves from. */
  selected: null as string | null,
  /** Every selected row; always contains `selected` when that is set. */
  selection: [] as string[],
  page: "downloads" as "downloads" | "settings",
  library: "all" as Library,
  view: "all" as View,
  /** File type chosen in the sidebar; null = every type. */
  category: null as Category | null,
  /** Queue chosen in the sidebar; null = every queue. */
  queue: null as string | null,
  /** Every queue, the main one first. */
  queues: [] as Queue[],
  /** Tool jobs (convert, shrink, ZIP) that run or just ended. */
  jobs: [] as JobStatus[],
  /** The queue being edited; an empty id makes a new one. */
  queueEdit: null as Queue | null,
  /** Whether the site grabber is open. */
  grabber: false,
  /** Finished downloads the tools dialog works on; null = closed. */
  tools: null as DownloadInfo[] | null,
  /** Whether the mini window is open. */
  mini: false,
  /** Text typed in the search box. */
  search: "",
  sort: layout.sort,
  inspector: layout.inspector,
  inspectorTab: layout.inspectorTab,
  /** Speed limit the quick toggle goes back to. */
  lastLimit: layout.lastLimit,
  pending: [] as PendingAdd[],
  /** URL for a manual "New download" dialog; null = closed. */
  manualUrl: null as string | null,
  /** Text for the batch dialog; null = closed. */
  batch: null as string | null,
  /** Videos of a page typed in "Add URL", waiting for a quality. */
  videoPage: null as VideoProbe | null,
  clipboardUrl: null as string | null,
  removing: null as DownloadInfo[] | null,
  error: null as string | null,
  /** Short confirmation shown at the bottom right. */
  notice: null as string | null,
  /** The download that just finished, offered with "Open". */
  finished: null as DownloadInfo | null,
  /** Download waiting for its new link from the browser. */
  refreshing: null as string | null,
  version: "",
});

export function saveLayout() {
  try {
    const l: Layout = {
      sort: $state.snapshot(app.sort),
      inspector: app.inspector,
      inspectorTab: app.inspectorTab,
      lastLimit: app.lastLimit,
    };
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(l));
  } catch {
    // Storage unavailable: the layout simply starts from the defaults next time.
  }
}

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

/** Whether `d` belongs to the sidebar scope, the chosen file type and the search. */
export function inScope(d: DownloadInfo): boolean {
  if (app.library === "unfinished" && d.status === "completed") return false;
  if (app.library === "finished" && d.status !== "completed") return false;
  if (app.queue && d.queue !== app.queue) return false;
  return (!app.category || d.category === app.category) && matchesSearch(d, app.search);
}

const STATUS_ORDER: Record<Status, number> = {
  downloading: 0,
  probing: 1,
  queued: 2,
  paused: 3,
  failed: 4,
  cancelled: 5,
  completed: 6,
};

function sortValue(d: DownloadInfo, key: SortKey): number | string {
  switch (key) {
    case "name":
      return (d.filename || d.url).toLowerCase();
    case "size":
      return d.totalSize ?? -1;
    case "status":
      return STATUS_ORDER[d.status];
    case "progress":
      return d.totalSize ? d.downloaded / d.totalSize : 0;
    case "speed":
      return d.status === "downloading" ? d.speed : -1;
    case "eta":
      return d.status === "downloading" && d.eta !== null ? d.eta : Number.MAX_SAFE_INTEGER;
    case "added":
      return d.createdAt;
    case "queue":
      return d.position;
  }
}

/** The rows of the list: scope, status filter and sort applied. */
export function listed(): DownloadInfo[] {
  const rows = app.downloads.filter((d) => inScope(d) && visible(d, app.view));
  const { key, dir } = app.sort;
  // `app.downloads` is kept in queue order already.
  if (key === "queue") return dir === 1 ? rows : rows.reverse();
  return rows.sort((a, b) => {
    const [x, y] = [sortValue(a, key), sortValue(b, key)];
    return (x < y ? -1 : x > y ? 1 : a.position - b.position) * dir;
  });
}

export function selected(): DownloadInfo | undefined {
  return app.downloads.find((d) => d.id === app.selected);
}

/** The selected downloads, in list order. */
export function selectedDownloads(): DownloadInfo[] {
  if (!app.selection.length) return [];
  const ids = new Set(app.selection);
  return app.downloads.filter((d) => ids.has(d.id));
}

export function selectOnly(id: string | null) {
  app.selected = id;
  app.selection = id ? [id] : [];
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
    case "completed":
      upsert(e.download);
      if (isMainWindow) {
        app.finished = e.download;
        if (app.settings?.sound) playDone();
      }
      break;
    case "added":
    case "updated":
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
        // The backend omits the segment map on most ticks (see events.rs):
        // only replace it when a map was actually sent, so the inspector
        // keeps showing the last map instead of blanking out.
        if (e.segments) d.segments = e.segments;
      }
      break;
    }
    case "removed":
      app.downloads = app.downloads.filter((x) => x.id !== e.id);
      app.selection = app.selection.filter((id) => id !== e.id);
      if (app.selected === e.id) app.selected = app.selection[0] ?? null;
      if (app.finished?.id === e.id) app.finished = null;
      break;
    case "queues":
      app.queues = e.queues;
      if (app.queue && !e.queues.some((q) => q.id === app.queue)) app.queue = null;
      break;
    case "tool": {
      const i = app.jobs.findIndex((j) => j.id === e.job.id);
      if (i >= 0) app.jobs[i] = e.job;
      else app.jobs.push(e.job);
      // A job that ended stays visible for a moment, a failed one until it is dismissed.
      if (e.job.state === "done" || e.job.state === "cancelled") {
        const id = e.job.id;
        setTimeout(() => dismissJob(id), e.job.state === "done" ? 8000 : 2500);
      }
      break;
    }
    case "queueFinished":
      break;
  }
}

export function dismissJob(id: string) {
  app.jobs = app.jobs.filter((j) => j.id !== id);
}

export async function refresh() {
  app.downloads = await api.list();
}

/** Ask the backend which links wait for their dialog and show the ones this window missed.
 *  A link can arrive while the window is still starting or while events were dropped; the
 *  backend keeps it until it is confirmed or cancelled, so asking again never loses one. */
async function syncPending() {
  const waiting = await api.pending().catch(() => null);
  if (!waiting) return;
  for (const p of waiting) if (!app.pending.some((x) => x.key === p.key)) app.pending.push(p);
}

// The last minute of speed, one sample per second: the total, and each running download.
const HISTORY = 61;
const perDownload = new Map<string, number[]>();
export const speedLog = $state({ total: [] as number[], peak: 0 });

function sampleSpeeds() {
  let total = 0;
  for (const d of app.downloads) {
    if (d.status !== "downloading") {
      perDownload.delete(d.id);
      continue;
    }
    total += d.speed;
    const h = perDownload.get(d.id) ?? [];
    h.push(d.speed);
    if (h.length > HISTORY) h.shift();
    perDownload.set(d.id, h);
  }
  speedLog.total.push(total);
  if (speedLog.total.length > HISTORY) speedLog.total.shift();
  speedLog.peak = Math.max(...speedLog.total);
}

/** Speed samples of one download, oldest first. Reads the clock, so callers re-run every second. */
export function samplesOf(id: string): number[] {
  void clock.now;
  return [...(perDownload.get(id) ?? [])];
}

const systemDark = matchMedia("(prefers-color-scheme: dark)");

export function applyTheme(s: Settings) {
  const root = document.documentElement;
  root.dataset.theme = s.theme === "system" ? (systemDark.matches ? "dark" : "light") : s.theme;
  root.dataset.accent = s.accent;
  root.dataset.density = s.density;
  root.classList.toggle("no-glass", !s.glass);
  i18n.lang = s.language;
  root.lang = s.language;
}

export async function saveSettings(s: Settings) {
  app.settings = await api.setSettings(s);
  applyTheme(app.settings);
}

/** Global speed limit: `null` lifts it. The last real limit is remembered for the toggle. */
export async function setGlobalLimit(limit: number | null) {
  if (!app.settings) return;
  if (limit) {
    app.lastLimit = limit;
    saveLayout();
  }
  await run(() => saveSettings({ ...$state.snapshot(app.settings!), speedLimit: limit }));
}

export async function init() {
  setInterval(() => {
    clock.now = Math.floor(Date.now() / 1000);
    sampleSpeeds();
  }, 1000);
  systemDark.addEventListener("change", () => app.settings && applyTheme(app.settings));
  await Promise.all([
    onEngineEvent(apply),
    onPending((p) => {
      if (!app.pending.some((x) => x.key === p.key)) app.pending.push(p);
    }),
    onClipboard((url) => (app.clipboardUrl = url)),
    onResync(() => {
      void refresh();
      void syncPending();
    }),
    onRefresh((r) => {
      app.refreshing = null;
      if (r.ok) app.notice = t("refresh.done");
      else app.error = errorText(r.code) ?? r.message ?? t("err.title");
    }),
  ]);
  const [settings, list, pending, info, refreshing, queues, mini] = await Promise.all([
    api.getSettings(),
    api.list(),
    api.pending(),
    api.appInfo(),
    api.refreshTarget(),
    api.listQueues(),
    api.miniOpen(),
  ]);
  // Only the main window shows dialogs; the mini and progress windows never take a link.
  if (isMainWindow) setInterval(() => void syncPending(), 2000);
  app.queues = queues ?? [];
  app.mini = !!mini;
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
