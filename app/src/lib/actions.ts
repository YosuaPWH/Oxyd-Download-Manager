import { api } from "./api";
import { app, isActive, run, selectedDownloads } from "./store.svelte";
import type { DownloadInfo } from "./types";

export function canStart(d: DownloadInfo | undefined): boolean {
  return !!d && ["paused", "failed", "cancelled"].includes(d.status);
}

export function canPause(d: DownloadInfo | undefined): boolean {
  return !!d && isActive(d);
}

/** Space / double click: pause running downloads, resume stopped ones, open finished ones. */
export function primaryAction(d: DownloadInfo) {
  if (d.status === "completed") return run(() => api.openFile(d.id));
  if (canPause(d)) return run(() => api.pause(d.id));
  return run(() => api.start(d.id));
}

export function askRemove(d: DownloadInfo | DownloadInfo[]) {
  const list = Array.isArray(d) ? d : [d];
  if (list.length) app.removing = list;
}

/** Resume every selected download that is stopped. */
export function startSelected() {
  const ids = selectedDownloads().filter(canStart).map((d) => d.id);
  return run(async () => {
    for (const id of ids) await api.start(id);
  });
}

/** Pause every selected download that is running or waiting. */
export function pauseSelected() {
  const ids = selectedDownloads().filter(canPause).map((d) => d.id);
  return run(async () => {
    for (const id of ids) await api.pause(id);
  });
}

/** Site of a download, for the line under its name. */
export function hostOf(d: DownloadInfo): string {
  if (d.kind === "torrent") return "";
  try {
    return new URL(d.finalUrl ?? d.url).host;
  } catch {
    return "";
  }
}
