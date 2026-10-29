// Messages between the content script (video button) and the service worker.
import type { SniffedStream } from "./media.ts";

/** One line of the video button's menu. */
export interface MenuEntry {
  /** Unique within one menu. */
  id: string;
  group: "video" | "audio" | "stream";
  label: string;
  detail: string;
  /** Quality offered by the app (yt-dlp), or `best`. */
  choice?: string;
  /** Stream the browser loaded, for pages yt-dlp does not know. */
  stream?: SniffedStream;
}

export interface VideoOptions {
  title: string;
  /** Page the qualities belong to (on a feed: the post). */
  pageUrl: string;
  entries: MenuEntry[];
  /** Why the list is short or empty. */
  notice?: string;
  /** Oxyd was not running and was not started (a lookup ahead of a click). */
  offline?: boolean;
}

export type ContentMessage =
  | { type: "video-options"; pageUrl: string; title: string; src?: string; launch: boolean }
  | { type: "video-download"; pageUrl: string; title: string; entry: MenuEntry };

export interface DownloadResult {
  ok: boolean;
  error?: string;
}
