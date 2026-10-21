import { api } from "./api";
import { app, isActive, run } from "./store.svelte";
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

export function askRemove(d: DownloadInfo) {
  app.removing = d;
}
