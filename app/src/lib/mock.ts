// Fake backend so the UI can be developed in a plain browser (`npm run dev`).
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { DownloadInfo, GrabResult, Queue, Settings, VideoProbe } from "./types";

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
    detail: null,
    media: null,
    queue: "main",
    kind: "http",
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
  dl({
    id: "g",
    filename: "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film.webm",
    url: "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
    category: "video",
    totalSize: 612 * MB,
    downloaded: 612 * MB,
    status: "probing",
    detail: "merge",
    position: 7,
    media: { format: "313+251", height: 2160, container: "webm", mp3: false, streams: [590 * MB, 22 * MB], ytdlp: false },
  }),
];

downloads.push(
  dl({
    id: "h",
    filename: "apod-2026-11-10-orion.jpg",
    url: "https://apod.nasa.gov/apod/image/2611/orion.jpg",
    category: "image",
    totalSize: 6.4 * MB,
    downloaded: 6.4 * MB,
    status: "completed",
    createdAt: now - 86400,
    completedAt: now - 86300,
    position: 8,
  }),
  dl({
    id: "i",
    filename: "dataset-openimages-subset.tar.zst",
    url: "https://data.example.org/openimages/dataset-openimages-subset.tar.zst",
    category: "archive",
    totalSize: 18400 * MB,
    downloaded: 8100 * MB,
    status: "paused",
    createdAt: now - 3 * 86400,
    position: 9,
    connections: 16,
    segments: segs(18400 * MB, 16, [0.5, 0.45, 0.4, 0.5, 0.42, 0.44, 0.47, 0.41, 0.46, 0.43, 0.4, 0.48, 0.45, 0.42, 0.44, 0.39]),
  }),
);
downloads.push(
  dl({
    id: "j",
    filename: "debian-13.1.0-amd64-netinst.iso",
    url: "magnet:?xt=urn:btih:6f84758b0ddd8dc05840bf932a77935d8b5b8b93",
    category: "archive",
    kind: "torrent",
    queue: "night",
    totalSize: 754 * MB,
    downloaded: 412 * MB,
    status: "downloading",
    speed: 3.1 * MB,
    eta: 110,
    activeConnections: 23,
    position: 10,
    segments: [{ start: 0, end: 754 * MB - 1, downloaded: 412 * MB }],
  }),
  dl({
    id: "k",
    filename: "gcc-15.2.0.tar.xz",
    url: "ftp://ftp.gnu.org/gnu/gcc/gcc-15.2.0/gcc-15.2.0.tar.xz",
    category: "archive",
    kind: "ftp",
    queue: "night",
    totalSize: 98 * MB,
    position: 11,
  }),
);
// Spread the sample downloads over the day so "Added" shows real times.
downloads.slice(0, 7).forEach((d, i) => (d.createdAt = now - 600 - i * 1740));
for (const d of downloads.slice(9)) d.createdAt = now - 240;

const video: VideoProbe = {
  pageUrl: "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
  title: "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film",
  duration: 635,
  uploader: "Blender",
  site: "Youtube",
  live: false,
  choices: [
    { id: "315+251", kind: "video", label: "2160p60", detail: "WEBM · VP9", size: 1210 * MB, container: "webm", height: 2160 },
    { id: "308+251", kind: "video", label: "1440p60", detail: "WEBM · VP9", size: 520 * MB, container: "webm", height: 1440 },
    { id: "299+140", kind: "video", label: "1080p60", detail: "MP4 · H.264", size: 310 * MB, container: "mp4", height: 1080 },
    { id: "298+140", kind: "video", label: "720p60", detail: "MP4 · H.264", size: 160 * MB, container: "mp4", height: 720 },
    { id: "18", kind: "video", label: "360p", detail: "MP4 · H.264", size: 38 * MB, container: "mp4", height: 360 },
    { id: "140", kind: "audio", label: "M4A", detail: "AAC · 129 kbps", size: 10 * MB, container: "m4a", height: null },
    { id: "mp3:251", kind: "audio", label: "MP3", detail: "MP3 · 192 kbps", size: 15 * MB, container: "mp3", height: null },
  ],
};

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
  schedule: { enabled: true, start: "01:00", stop: "06:30", days: [0, 1, 2, 3, 4] },
  afterQueue: "nothing",
  video: { ytdlpPath: "", ffmpegPath: "", autoUpdate: true },
  accent: "oxide",
  density: "comfortable",
  glass: true,
  sound: true,
  progressWindow: true,
};

let queues: Queue[] = [
  { id: "main", name: "", maxActive: 3, speedLimit: null, schedule: { enabled: true, start: "01:00", stop: "06:30", days: [0, 1, 2, 3, 4] } },
  { id: "night", name: "Night", maxActive: 2, speedLimit: 2 * MB, schedule: { enabled: true, start: "23:00", stop: "06:00", days: [] } },
];

export function installMock() {
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "list_downloads":
          return new URLSearchParams(location.search).has("empty") ? [] : downloads;
        case "add_download": {
          const url = String((a.req as { url: string }).url);
          const name = decodeURIComponent(url.split("/").pop() || "download.bin");
          const d = dl({ id: `n${downloads.length}`, filename: name, url, position: downloads.length + 1, createdAt: Math.floor(Date.now() / 1000), status: (a.req as { start: boolean }).start ? "queued" : "paused" });
          downloads.push(d);
          void emit("oxyd://event", { type: "added", download: d });
          return d;
        }
        case "list_queues":
          return queues;
        case "save_queue": {
          const q = a.queue as Queue;
          if (!q.id) q.id = `q${queues.length}`;
          queues = queues.some((x) => x.id === q.id) ? queues.map((x) => (x.id === q.id ? q : x)) : [...queues, q];
          void emit("oxyd://event", { type: "queues", queues });
          return q;
        }
        case "delete_queue":
          queues = queues.filter((x) => x.id !== a.id);
          void emit("oxyd://event", { type: "queues", queues });
          return null;
        case "move_to_queue":
          for (const d of downloads) {
            if ((a.ids as string[]).includes(d.id)) {
              d.queue = a.queue as string;
              void emit("oxyd://event", { type: "updated", download: d });
            }
          }
          return null;
        case "grab_page": {
          const base = "https://images.example.org/gallery";
          const items: GrabResult["items"] = [
            ...Array.from({ length: 14 }, (_, i) => ({ url: `${base}/photo-${String(i + 1).padStart(3, "0")}.jpg`, name: `photo-${String(i + 1).padStart(3, "0")}.jpg`, category: "image" as const, page: String(a.url) })),
            { url: `${base}/gallery-full.zip`, name: "gallery-full.zip", category: "archive", page: String(a.url) },
            { url: "https://cdn.example.org/making-of.mp4", name: "making-of.mp4", category: "video", page: String(a.url) },
            { url: `${base}/credits.pdf`, name: "credits.pdf", category: "document", page: String(a.url) },
          ];
          return { items, pages: 3, skipped: 0, truncated: false } satisfies GrabResult;
        }
        case "run_tool": {
          const jobId = `job${Date.now()}`;
          const first = downloads.find((d) => d.id === (a.ids as string[])[0]);
          const kind = (a.job as { type: string }).type;
          let progress = 0;
          const tick = () => {
            progress = Math.min(1, progress + 0.04);
            void emit("oxyd://event", {
              type: "tool",
              job: { id: jobId, kind, name: first?.filename ?? "", state: progress >= 1 ? "done" : "running", progress, done: progress >= 1 ? 1 : 0, total: 1, output: progress >= 1 ? `C:\\Users\\me\\Downloads\\${first?.filename.replace(/\.[^.]+$/, "")}.${kind === "zip" ? "zip" : "mp3"}` : null, error: null },
            });
            if (progress < 1) setTimeout(tick, 250);
          };
          tick();
          return jobId;
        }
        case "toggle_mini":
          return false;
        case "mini_open":
          return false;
        case "remove_download":
          void emit("oxyd://event", { type: "removed", id: a.id });
          return null;
        case "get_settings":
          return settings;
        case "set_settings":
          settings = a.settings as Settings;
          return settings;
        case "app_info":
          return { version: "0.4.0", dataDir: "C:\\Users\\me\\AppData\\Local\\Oxyd" };
        case "pending_downloads":
          // `?video` opens the "Download video" dialog as if the browser sent a video.
          return new URLSearchParams(location.search).has("video")
            ? [{ key: "v1", url: video.pageUrl, filename: null, size: null, mime: null, referrer: null, source: "chrome", matched: null, video: { probe: video, choice: "299+140" } }]
            : [];
        case "probe_video":
          return video;
        case "video_file_name":
          return `${String(a.title).replace(/[\\/:*?"<>|]/g, "_")}.${a.container}`;
        case "tool_status":
          return [
            { tool: "yt-dlp", path: "C:\\Users\\me\\AppData\\Local\\Oxyd\\tools\\yt-dlp\\yt-dlp.exe", version: "2026.10.26", managed: true },
            { tool: "ffmpeg", path: "C:\\Users\\me\\AppData\\Local\\Oxyd\\tools\\ffmpeg\\ffmpeg.exe", version: "latest", managed: true },
            { tool: "deno", path: null, version: null, managed: false },
          ];
        case "update_ytdlp":
          return null;
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
            mime: /youtube|instagram|facebook|tiktok/.test(String(a.url)) ? "text/html" : "application/octet-stream",
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
