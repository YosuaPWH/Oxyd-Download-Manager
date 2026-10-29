export type Status =
  | "queued"
  | "probing"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export type Category = "video" | "music" | "document" | "archive" | "program" | "other";

export interface Segment {
  start: number;
  end: number;
  downloaded: number;
}

export interface DownloadInfo {
  id: string;
  url: string;
  finalUrl: string | null;
  filename: string;
  dir: string;
  savePath: string;
  totalSize: number | null;
  downloaded: number;
  status: Status;
  resumable: boolean;
  etag: string | null;
  lastModified: string | null;
  mime: string | null;
  category: Category;
  connections: number;
  speedLimit: number | null;
  checksum: string | null;
  error: string | null;
  /** Machine readable code of `error`, e.g. "network" or "link_expired". */
  errorCode: string | null;
  position: number;
  createdAt: number;
  updatedAt: number;
  completedAt: number | null;
  speed: number;
  eta: number | null;
  activeConnections: number;
  segments: Segment[];
  /** Unix time of the next automatic retry after a temporary failure. */
  retryAt: number | null;
  /** What a running video download does besides downloading (`page`, `merge`, `tool:yt-dlp:40`). */
  detail: string | null;
  /** Set for a video taken from a web page; `url` is then the page. */
  media: MediaSpec | null;
}

export interface MediaSpec {
  format: string;
  height: number | null;
  container: string;
  mp3: boolean;
  streams: number[];
  ytdlp: boolean;
}

export interface VideoChoice {
  /** `137+140`, `18`, `mp3:140` */
  id: string;
  kind: "video" | "audio";
  label: string;
  detail: string;
  size: number | null;
  container: string;
  height: number | null;
}

export interface VideoProbe {
  pageUrl: string;
  title: string;
  duration: number | null;
  uploader: string | null;
  site: string | null;
  live: boolean;
  choices: VideoChoice[];
}

export interface ToolStatus {
  tool: "yt-dlp" | "ffmpeg" | "deno";
  path: string | null;
  version: string | null;
  managed: boolean;
}

export interface ProbeInfo {
  finalUrl: string;
  size: number | null;
  acceptRanges: boolean;
  etag: string | null;
  lastModified: string | null;
  filename: string | null;
  mime: string | null;
}

export type DuplicatePolicy = "rename" | "overwrite" | "ask";
export type AfterQueue = "nothing" | "sleep" | "shutdown";

export interface Settings {
  downloadDir: string;
  categoryFolders: boolean;
  connections: number;
  maxActive: number;
  speedLimit: number | null;
  duplicatePolicy: DuplicatePolicy;
  confirmNew: boolean;
  autostart: boolean;
  notifications: boolean;
  clipboardMonitor: boolean;
  proxy: string | null;
  theme: "system" | "light" | "dark";
  language: "id" | "en";
  schedule: { enabled: boolean; start: string | null; stop: string | null };
  afterQueue: AfterQueue;
  video: { ytdlpPath: string; ffmpegPath: string; autoUpdate: boolean };
}

export interface PendingMatch {
  id: string;
  filename: string;
  downloaded: number;
  totalSize: number | null;
}

export interface PendingAdd {
  key: string;
  url: string;
  filename: string | null;
  size: number | null;
  mime: string | null;
  referrer: string | null;
  source: string | null;
  /** Unfinished download this link probably continues. */
  matched: PendingMatch | null;
  /** A video of a web page waiting for its quality. */
  video: { probe: VideoProbe; choice: string | null } | null;
}

/** Result of giving a waiting download its new link from the browser. */
export interface RefreshResult {
  id: string;
  ok: boolean;
  code: string | null;
  message: string | null;
}

export interface NewDownload {
  url: string;
  filename?: string | null;
  dir?: string | null;
  connections?: number | null;
  speedLimit?: number | null;
  checksum?: string | null;
  sizeHint?: number | null;
  start: boolean;
  onConflict?: DuplicatePolicy | null;
}

export type EngineEvent =
  | { type: "added" | "updated" | "completed" | "failed"; download: DownloadInfo }
  | {
      type: "progress";
      id: string;
      downloaded: number;
      total: number | null;
      speed: number;
      eta: number | null;
      activeConnections: number;
      segments: Segment[];
    }
  | { type: "removed"; id: string }
  | { type: "queueFinished" };

export interface CmdError {
  code: string;
  message: string;
}
