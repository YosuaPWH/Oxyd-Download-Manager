// Fake backend so the UI can be developed in a plain browser (`npm run dev`).
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { DownloadInfo, Settings } from "./types";

const MB = 1024 * 1024;
const now = Math.floor(Date.now() / 1000);

function dl(p: Partial<DownloadInfo> & Pick<DownloadInfo, "id" | "filename">): DownloadInfo {
  const total = p.totalSize ?? 100 * MB;
  return {
    url: `https://example.com/files/${p.filename}`,
    finalUrl: null,
    dir: "C:\\Users\\me\\Downloads",
    savePath: `C:\\Users\\me\\Downloads\\${p.filename}`,
    totalSize: total,
    downloaded: 0,
    status: "queued",
    resumable: true,
    etag: null,
    lastModified: null,
    mime: null,
    category: "other",
    connections: 8,
    speedLimit: null,
    checksum: null,
    error: null,
    errorCode: null,
    position: 1,
    createdAt: now - 600,
    updatedAt: now,
    completedAt: null,
    speed: 0,
    eta: null,
    activeConnections: 0,
    segments: [],
    retryAt: null,
    ...p,
  };
}

function segs(total: number, n: number, frac: number[]) {
  const len = Math.floor(total / n);
  return Array.from({ length: n }, (_, i) => ({
    start: i * len,
    end: i === n - 1 ? total - 1 : (i + 1) * len - 1,
    downloaded: Math.floor(len * (frac[i] ?? 0)),
  }));
}

const downloads: DownloadInfo[] = [
  dl({
    id: "a",
    filename: "ubuntu-24.04-desktop-amd64.iso",
    url: "https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso",
    category: "archive",
    totalSize: 5800 * MB,
    downloaded: 2400 * MB,
    status: "downloading",
    speed: 11.4 * MB,
    eta: 298,
    activeConnections: 8,
    position: 1,
    segments: segs(5800 * MB, 8, [0.6, 0.4, 0.45, 0.3, 0.5, 0.35, 0.4, 0.3]),
  }),
  dl({
    id: "b",
    filename: "Big_Buck_Bunny_4K.mp4",
    url: "https://download.blender.org/demo/movies/BBB/Big_Buck_Bunny_4K.mp4",
    category: "video",
    totalSize: 890 * MB,
    downloaded: 890 * MB,
    status: "completed",
    completedAt: now - 120,
    position: 2,
  }),
  dl({
    id: "c",
    filename: "VSCodeUserSetup-x64.exe",
    url: "https://update.code.visualstudio.com/latest/win32-x64-user/stable",
    category: "program",
    totalSize: 98 * MB,
    downloaded: 41 * MB,
    status: "paused",
    position: 3,
    segments: segs(98 * MB, 4, [0.5, 0.4, 0.4, 0.4]),
  }),
  dl({
    id: "d",
    filename: "artemis-ii-press-kit.pdf",
    url: "https://www.nasa.gov/wp-content/uploads/artemis-ii-press-kit.pdf",
    category: "document",
    totalSize: 12 * MB,
    status: "failed",
    error: "network error: error sending request for url (https://www.nasa.gov/wp-content/uploads/artemis-ii-press-kit.pdf)",
    errorCode: "network",
    retryAt: now + 45,
    position: 4,
  }),
  dl({
    id: "e",
    filename: "podcast-ep42.mp3",
    url: "https://cdn.longtailpod.fm/episodes/podcast-ep42.mp3",
    category: "music",
    totalSize: 64 * MB,
    position: 5,
  }),
  dl({
    id: "f",
    filename: "blender-4.2.3-windows-x64.msi",
    url: "https://download.blender.org/release/Blender4.2/blender-4.2.3-windows-x64.msi",
    category: "program",
    totalSize: 342.6 * MB,
    downloaded: 98.3 * MB,
    status: "downloading",
    speed: 2.6 * MB,
    eta: 94,
    activeConnections: 4,
    position: 6,
    segments: segs(342.6 * MB, 4, [0.4, 0.3, 0.25, 0.2]),
  }),
];

let settings: Settings = {
  downloadDir: "C:\\Users\\me\\Downloads",
  categoryFolders: false,
  connections: 8,
  maxActive: 3,
  speedLimit: null,
  duplicatePolicy: "rename",
  confirmNew: true,
  autostart: false,
  notifications: true,
  clipboardMonitor: false,
  proxy: null,
  theme: "system",
  // `?lang=en` shows the English interface (used for the README screenshots).
  language: new URLSearchParams(location.search).get("lang") === "en" ? "en" : "id",
  schedule: { enabled: false, start: null, stop: null },
  afterQueue: "nothing",
};

export function installMock() {
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "list_downloads":
          return downloads;
        case "get_settings":
          return settings;
        case "set_settings":
          settings = a.settings as Settings;
          return settings;
        case "app_info":
          return { version: "0.1.3", dataDir: "C:\\Users\\me\\AppData\\Local\\Oxyd" };
        case "pending_downloads":
          return [];
        case "refresh_target":
          return null;
        case "clear_completed":
          return 1;
        case "begin_refresh":
          return "https://example.com/files";
        case "find_unfinished":
          return downloads.find(
            (d) => d.filename.toLowerCase() === String(a.filename).toLowerCase() && d.status !== "completed",
          ) ?? null;
        case "clipboard_link":
          return downloads[0].url;
        case "suggest_dir":
          return settings.downloadDir;
        case "path_exists":
          return a.filename === "Big_Buck_Bunny_4K.mp4";
        case "probe_url":
          return {
            finalUrl: a.url,
            // A link to a file already in the list reports that file's size.
            size: downloads.find((d) => d.url === a.url)?.totalSize ?? 734 * MB,
            acceptRanges: true,
            etag: '"x"',
            lastModified: null,
            filename: String(a.url).split("/").pop() || "download.bin",
            mime: "application/octet-stream",
          };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  // Animate the running downloads; their speed wanders around a base like a real line.
  const running = downloads.filter((d) => d.status === "downloading");
  const base = new Map(running.map((d) => [d.id, d.speed]));
  setInterval(() => {
    for (const d of running) {
      d.speed = base.get(d.id)! * (0.88 + Math.random() * 0.24);
      d.downloaded = Math.min(d.totalSize!, d.downloaded + d.speed / 4);
      const n = d.segments.length;
      d.segments.forEach((s) => (s.downloaded = Math.min(s.end - s.start + 1, s.downloaded + d.speed / 4 / n)));
      void emit("oxyd://event", {
        type: "progress",
        id: d.id,
        downloaded: d.downloaded,
        total: d.totalSize,
        speed: d.speed,
        eta: Math.round((d.totalSize! - d.downloaded) / d.speed),
        activeConnections: d.activeConnections,
        segments: d.segments,
      });
    }
  }, 250);
}
