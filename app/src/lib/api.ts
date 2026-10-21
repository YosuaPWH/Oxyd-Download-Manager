import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { errorText } from "./i18n.svelte";
import type {
  DownloadInfo,
  DuplicatePolicy,
  EngineEvent,
  NewDownload,
  PendingAdd,
  ProbeInfo,
  RefreshResult,
  Settings,
} from "./types";

export const api = {
  list: () => invoke<DownloadInfo[]>("list_downloads"),
  probe: (url: string) => invoke<ProbeInfo>("probe_url", { url }),
  add: (req: NewDownload) => invoke<DownloadInfo>("add_download", { req }),
  start: (id: string) => invoke<void>("start_download", { id }),
  pause: (id: string) => invoke<void>("pause_download", { id }),
  cancel: (id: string) => invoke<void>("cancel_download", { id }),
  restart: (id: string) => invoke<void>("restart_download", { id }),
  remove: (id: string, deleteFile: boolean) => invoke<void>("remove_download", { id, deleteFile }),
  refreshUrl: (id: string, url: string) => invoke<DownloadInfo>("refresh_url", { id, url }),
  setLimit: (id: string, limit: number | null) => invoke<void>("set_download_limit", { id, limit }),
  move: (id: string, delta: number) => invoke<void>("move_download", { id, delta }),
  pauseAll: () => invoke<void>("pause_all"),
  resumeAll: () => invoke<void>("resume_all"),
  openFile: (id: string) => invoke<void>("open_file", { id }),
  openFolder: (id: string) => invoke<void>("open_folder", { id }),
  copy: (text: string) => invoke<void>("copy_text", { text }),
  clipboardLink: () => invoke<string | null>("clipboard_link"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  pickFolder: (start: string | null) => invoke<string | null>("pick_folder", { start }),
  suggestDir: (filename: string) => invoke<string>("suggest_dir", { filename }),
  pathExists: (dir: string, filename: string) => invoke<boolean>("path_exists", { dir, filename }),
  appInfo: () => invoke<{ version: string; dataDir: string }>("app_info"),
  pending: () => invoke<PendingAdd[]>("pending_downloads"),
  probePending: (key: string) => invoke<ProbeInfo>("probe_pending", { key }),
  confirmPending: (
    key: string,
    options: {
      filename: string | null;
      dir: string | null;
      start: boolean;
      connections: number | null;
      onConflict: DuplicatePolicy | null;
      sizeHint: number | null;
      checksum: string | null;
    },
  ) => invoke<DownloadInfo>("confirm_pending", { key, options }),
  discardPending: (key: string) => invoke<void>("discard_pending", { key }),
  beginRefresh: (id: string) => invoke<string | null>("begin_refresh", { id }),
  cancelRefresh: () => invoke<void>("cancel_refresh"),
  refreshTarget: () => invoke<string | null>("refresh_target"),
  findUnfinished: (filename: string, size: number | null) =>
    invoke<DownloadInfo | null>("find_unfinished", { filename, size }),
  resumePending: (key: string, id: string) => invoke<DownloadInfo>("resume_pending", { key, id }),
  resumeWithUrl: (id: string, url: string) => invoke<DownloadInfo>("resume_with_url", { id, url }),
  clearCompleted: () => invoke<number>("clear_completed"),
};

export function onEngineEvent(cb: (e: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>("oxyd://event", (e) => cb(e.payload));
}

export function onPending(cb: (p: PendingAdd) => void): Promise<UnlistenFn> {
  return listen<PendingAdd>("oxyd://pending", (e) => cb(e.payload));
}

export function onClipboard(cb: (url: string) => void): Promise<UnlistenFn> {
  return listen<string>("oxyd://clipboard", (e) => cb(e.payload));
}

export function onRefresh(cb: (r: RefreshResult) => void): Promise<UnlistenFn> {
  return listen<RefreshResult>("oxyd://refresh", (e) => cb(e.payload));
}

export function onResync(cb: () => void): Promise<UnlistenFn> {
  return listen("oxyd://resync", () => cb());
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    const err = e as { code?: string; message: unknown };
    return errorText(err.code) ?? String(err.message);
  }
  return String(e);
}
