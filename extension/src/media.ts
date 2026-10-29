// Videos on web pages: which sites yt-dlp reads, which link is a single post, and which
// responses the browser loaded are media streams.

/** Sites whose pages yt-dlp reads (sub domains included). */
export const VIDEO_SITES = [
  "youtube.com", "youtu.be", "youtube-nocookie.com",
  "instagram.com", "facebook.com", "fb.watch", "threads.net", "threads.com",
  "tiktok.com", "x.com", "twitter.com", "reddit.com", "linkedin.com", "pinterest.com",
  "vimeo.com", "dailymotion.com", "twitch.tv", "streamable.com", "rumble.com",
  "bilibili.com", "bilibili.tv", "soundcloud.com", "bandcamp.com", "9gag.com",
  "vk.com", "ok.ru", "tumblr.com", "imgur.com", "bsky.app", "kick.com",
];

function host(url: string): string | null {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return null;
  }
}

export function isVideoSite(url: string): boolean {
  const h = host(url);
  return !!h && VIDEO_SITES.some((s) => h === s || h.endsWith(`.${s}`));
}

/** Paths of a single post or video, per site. */
const POST_PATHS: [string, RegExp][] = [
  ["youtube.com", /^\/(watch|shorts\/[\w-]+|embed\/[\w-]+|live\/[\w-]+)/],
  ["youtu.be", /^\/[\w-]+/],
  ["youtube-nocookie.com", /^\/embed\/[\w-]+/],
  ["instagram.com", /^\/(?:[\w.]+\/)?(p|reel|reels|tv)\/[\w-]+/],
  ["facebook.com", /^\/(watch|reel\/|share\/[vr]\/|[\w.]+\/videos\/|video\.php|story\.php|permalink\.php|[\w.]+\/posts\/)/],
  ["fb.watch", /^\/[\w-]+/],
  ["x.com", /^\/\w+\/status\/\d+/],
  ["twitter.com", /^\/\w+\/status\/\d+/],
  ["tiktok.com", /^\/@[\w.-]+\/(video|photo)\/\d+/],
  ["reddit.com", /^\/r\/\w+\/comments\/\w+/],
  ["threads.net", /^\/@[\w.]+\/post\/[\w-]+/],
  ["threads.com", /^\/@[\w.]+\/post\/[\w-]+/],
  ["linkedin.com", /^\/(posts\/|feed\/update\/)/],
  ["vimeo.com", /^\/\d+/],
  ["dailymotion.com", /^\/video\/\w+/],
  ["bsky.app", /^\/profile\/[^/]+\/post\/\w+/],
];

/**
 * `href` (resolved against `base`) when it links to a single post or video on a site yt-dlp
 * reads, else undefined. YouTube links keep only `v` and `t`: `list` would make it a
 * playlist.
 */
export function postUrl(href: string | null | undefined, base: string): string | undefined {
  if (!href) return undefined;
  let u: URL;
  try {
    u = new URL(href, base);
  } catch {
    return undefined;
  }
  if (u.protocol !== "https:" && u.protocol !== "http:") return undefined;
  const h = u.hostname.toLowerCase();
  const rule = POST_PATHS.find(([site]) => h === site || h.endsWith(`.${site}`));
  if (!rule || !rule[1].test(u.pathname)) return undefined;
  if (rule[0] === "youtube.com" && u.pathname === "/watch") {
    const v = u.searchParams.get("v");
    if (!v) return undefined;
    const keep = new URLSearchParams({ v });
    const t = u.searchParams.get("t");
    if (t) keep.set("t", t);
    u.search = keep.toString();
  }
  u.hash = "";
  return u.toString();
}

export type StreamKind = "hls" | "dash" | "file";

/** What a response the browser received is, if it is media worth offering. */
export function classifyMedia(r: {
  url: string;
  contentType?: string;
  size?: number;
}): StreamKind | null {
  let path: string;
  try {
    const u = new URL(r.url);
    if (u.protocol !== "https:" && u.protocol !== "http:") return null;
    path = u.pathname.toLowerCase();
  } catch {
    return null;
  }
  const type = (r.contentType ?? "").split(";")[0].trim().toLowerCase();
  if (/mpegurl/.test(type) || path.endsWith(".m3u8")) return "hls";
  if (type === "application/dash+xml" || path.endsWith(".mpd")) return "dash";
  // Pieces of a stream (HLS/DASH segments) are not files of their own.
  if (/\.(ts|m4s|m4v|aac|vtt|webvtt|key)$/.test(path) || /\/(seg|chunk|frag)[-_]?\d/.test(path)) return null;
  const media = /^(video|audio)\//.test(type) || /\.(mp4|webm|mkv|mov|m4a|mp3|ogg|opus|flac|wav)$/.test(path);
  if (!media) return null;
  // Small files are previews, ads and notification sounds.
  if (r.size !== undefined && r.size < 512 * 1024) return null;
  return "file";
}

/** A stream seen on a page. */
export interface SniffedStream {
  url: string;
  kind: StreamKind;
  size?: number;
  mime?: string;
  /** When it was seen (epoch ms). */
  seen: number;
}

/** Add `s` to `list` (newest first, without duplicates, at most `max`). */
export function addStream(list: SniffedStream[], s: SniffedStream, max = 30): SniffedStream[] {
  const key = streamKey(s.url);
  const rest = list.filter((x) => streamKey(x.url) !== key);
  return [s, ...rest].slice(0, max);
}

/** Two requests of one stream differ only in byte-range or signature parameters. */
export function streamKey(url: string): string {
  try {
    const u = new URL(url);
    for (const p of ["range", "bytestart", "byteend", "start", "end", "_nc_rid", "expires", "signature", "sig", "token"]) {
      u.searchParams.delete(p);
    }
    return `${u.host}${u.pathname}?${u.searchParams}`;
  } catch {
    return url;
  }
}

export function formatBytes(n: number | undefined): string {
  if (n === undefined || n <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

/** Label for a sniffed stream: its kind, quality hint from the URL and size. */
export function streamLabel(s: SniffedStream): { label: string; detail: string } {
  const quality = /(?:^|[^\d])(2160|1440|1080|720|480|360|240)p?(?:[^\d]|$)/.exec(s.url)?.[1];
  const label = s.kind === "hls" ? "HLS" : s.kind === "dash" ? "DASH" : (s.mime?.split("/")[1] ?? "video").toUpperCase();
  const detail = [quality ? `${quality}p` : "", formatBytes(s.size)].filter(Boolean).join(" · ");
  return { label, detail };
}
