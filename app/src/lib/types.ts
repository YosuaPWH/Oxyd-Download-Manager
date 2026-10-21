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
}

export interface PendingAdd {
  key: string;
  url: string;
  filename: string | null;
  size: number | null;
  mime: string | null;
  referrer: string | null;
  source: string | null;
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
