// Content script: a "Download" button over videos, like IDM's. Hovering a video shows it;
// clicking lists the qualities Oxyd found (through yt-dlp) or the streams the page loaded.
// Everything lives in a closed shadow root so the page's styles and scripts cannot touch it.
import type { ContentMessage, DownloadResult, MenuEntry, VideoOptions } from "./messages.ts";
import { isVideoSite, postUrl } from "./media.ts";

const MIN_W = 200;
const MIN_H = 120;
const HIDE_AFTER = 800;
const PREFETCH_AFTER = 500;
const CACHE_MS = 10 * 60_000;
const IS_YOUTUBE = /(^|\.)youtube(-nocookie)?\.com$/.test(location.hostname);

const indonesian = chrome.i18n.getUILanguage().startsWith("id");
const TEXT = indonesian
  ? {
      download: "Unduh",
      title: "Unduh video ini dengan Oxyd",
      finding: "Mencari kualitas…",
      best: "Kualitas terbaik",
      bestHint: "Langsung unduh tanpa menunggu daftar",
      video: "Video",
      audio: "Hanya audio",
      streams: "Stream di halaman ini",
      none: "Belum ada video yang bisa diunduh. Putar videonya lalu coba lagi.",
      sending: "Mengirim…",
      sent: "Dikirim ke Oxyd",
      failed: "Gagal",
    }
  : {
      download: "Download",
      title: "Download this video with Oxyd",
      finding: "Finding qualities…",
      best: "Best quality",
      bestHint: "Download right away, without waiting for the list",
      video: "Video",
      audio: "Audio only",
      streams: "Streams on this page",
      none: "No downloadable video yet. Play it and try again.",
      sending: "Sending…",
      sent: "Sent to Oxyd",
      failed: "Failed",
    };

const CSS = `
:host { all: initial; }
.wrap { position: fixed; left: var(--x); top: var(--y); display: none; flex-direction: column; align-items: flex-end;
  gap: 6px; pointer-events: none; font: 13px/1.35 system-ui, -apple-system, "Segoe UI", sans-serif; color: #111; }
.wrap.show { display: flex; }
.pill, .menu { pointer-events: auto; }
.pill { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px 0 10px; border: 0;
  border-radius: 999px; background: rgba(17, 17, 17, 0.86); color: #fff; font: 600 13px/1 inherit; cursor: pointer;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.3); backdrop-filter: blur(6px); }
.pill:hover { background: #111; }
.pill:focus-visible { outline: 2px solid #4c8dff; outline-offset: 2px; }
.pill.done { background: #15803d; }
.pill.err { background: #b91c1c; }
.menu { display: none; box-sizing: border-box; width: 100%; min-width: 220px; max-height: 360px; overflow-y: auto; padding: 6px; border-radius: 10px;
  background: #fff; box-shadow: 0 8px 30px rgba(0, 0, 0, 0.28); }
.menu.open { display: block; }
.menu.up { position: absolute; bottom: 36px; }
.head { padding: 6px 8px 8px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.group { padding: 8px 8px 4px; font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.04em; }
.item { display: flex; align-items: baseline; gap: 10px; width: 100%; padding: 7px 8px; border: 0; border-radius: 6px;
  background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
.item:hover, .item:focus-visible { background: #f0f2f5; outline: none; }
.l { min-width: 64px; font-weight: 600; font-variant-numeric: tabular-nums; }
.d { flex: 1; min-width: 0; color: #555; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.note, .empty, .loading { padding: 8px; color: #555; }
@media (prefers-color-scheme: dark) {
  .menu { background: #1f1f22; color: #eee; }
  .item:hover, .item:focus-visible { background: #2c2c31; }
  .d, .note, .empty, .loading, .group { color: #aaa; }
}`;

const ICON =
  '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" ' +
  'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v12m0 0-5-5m5 5 5-5M5 20h14"/></svg>';

function send<T>(msg: ContentMessage): Promise<T | undefined> {
  return chrome.runtime.sendMessage(msg).catch(() => undefined) as Promise<T | undefined>;
}

/** The element the button sits on (YouTube: the player, not its bare <video>). */
function targetOf(v: HTMLVideoElement): Element | null {
  if (IS_YOUTUBE) {
    // The main and Shorts players; not the muted previews on feeds.
    return v.closest("#movie_player, #shorts-player, ytd-reel-video-renderer .html5-video-player");
  }
  return v;
}

function big(r: DOMRect): boolean {
  return r.width >= MIN_W && r.height >= MIN_H;
}

function inside(r: DOMRect, x: number, y: number): boolean {
  return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
}

/** The post a video on a feed belongs to, so the app reads that post and not the feed. */
function pageOf(v: HTMLVideoElement): string {
  const here = location.href;
  const single = postUrl(here, here);
  if (single) return single;
  const tile = postUrl(v.closest<HTMLAnchorElement>("a[href]")?.href, here);
  if (tile) return tile;
  const post = v.closest('article, [role="article"], shreddit-post');
  if (post) {
    const links = [post.getAttribute("permalink"), ...[...post.querySelectorAll<HTMLAnchorElement>("a[href]")].map((a) => a.href)];
    for (const href of links) {
      const u = postUrl(href, here);
      if (u) return u;
    }
  }
  return here;
}

function title(): string {
  const t = IS_YOUTUBE ? document.title.replace(/^\(\d+\)\s*/, "").replace(/ - YouTube$/, "") : document.title;
  return t.trim() || location.hostname;
}

class VideoButton {
  private host: HTMLElement | null = null;
  private wrap!: HTMLDivElement;
  private pill!: HTMLButtonElement;
  private label!: HTMLSpanElement;
  private menu!: HTMLDivElement;
  private videos = new Set<HTMLVideoElement>();
  private visible = new Set<HTMLVideoElement>();
  private io: IntersectionObserver | null = null;
  private mo: MutationObserver | null = null;
  private current: HTMLVideoElement | null = null;
  private menuOpen = false;
  private overUi = false;
  private hideTimer: number | undefined;
  private prefetchTimer: number | undefined;
  private frame = 0;
  private x = -1;
  private y = -1;
  private cache = new Map<string, { at: number; res: Promise<VideoOptions | undefined> }>();
  private contextVideo: HTMLVideoElement | null = null;
  private enabled = false;

  setEnabled(on: boolean) {
    if (on === this.enabled) return;
    this.enabled = on;
    if (on) this.start();
    else this.stop();
  }

  private start() {
    this.io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const v = e.target as HTMLVideoElement;
        if (e.isIntersecting) this.visible.add(v);
        else this.visible.delete(v);
      }
    });
    this.scan();
    let queued = false;
    this.mo = new MutationObserver(() => {
      if (queued) return;
      queued = true;
      requestAnimationFrame(() => {
        queued = false;
        this.scan();
      });
    });
    this.mo.observe(document.documentElement, { childList: true, subtree: true });
    addEventListener("pointermove", this.onMove, { capture: true, passive: true });
    addEventListener("scroll", this.onScroll, { capture: true, passive: true });
    addEventListener("resize", this.onScroll, { passive: true });
    document.addEventListener("fullscreenchange", this.onFullscreen);
    document.addEventListener("pointerdown", this.onDown, true);
    document.addEventListener("keydown", this.onKey, true);
    document.addEventListener("contextmenu", this.onContextMenu, true);
    // Single-page sites change the video without loading a page.
    addEventListener("popstate", this.onNavigate);
    document.addEventListener("yt-navigate-finish", this.onNavigate);
  }

  private stop() {
    this.io?.disconnect();
    this.mo?.disconnect();
    this.videos.clear();
    this.visible.clear();
    removeEventListener("pointermove", this.onMove, { capture: true });
    removeEventListener("scroll", this.onScroll, { capture: true });
    removeEventListener("resize", this.onScroll);
    document.removeEventListener("fullscreenchange", this.onFullscreen);
    document.removeEventListener("pointerdown", this.onDown, true);
    document.removeEventListener("keydown", this.onKey, true);
    document.removeEventListener("contextmenu", this.onContextMenu, true);
    removeEventListener("popstate", this.onNavigate);
    document.removeEventListener("yt-navigate-finish", this.onNavigate);
    this.hide(true);
    this.host?.remove();
    this.host = null;
  }

  private scan() {
    for (const v of document.querySelectorAll("video")) {
      if (this.videos.has(v)) continue;
      this.videos.add(v);
      this.io?.observe(v);
    }
    for (const v of this.videos) {
      if (v.isConnected) continue;
      this.videos.delete(v);
      this.visible.delete(v);
      this.io?.unobserve(v);
    }
  }

  private ensureHost() {
    if (this.host) return;
    const host = document.createElement("oxyd-video-button");
    host.style.setProperty("all", "initial", "important");
    host.style.setProperty("position", "fixed", "important");
    host.style.setProperty("inset", "0 auto auto 0", "important");
    host.style.setProperty("z-index", "2147483647", "important");
    host.style.setProperty("pointer-events", "none", "important");
    const root = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = CSS;
    this.wrap = document.createElement("div");
    this.wrap.className = "wrap";
    this.pill = document.createElement("button");
    this.pill.className = "pill";
    this.pill.type = "button";
    this.pill.title = TEXT.title;
    this.pill.setAttribute("aria-haspopup", "menu");
    this.pill.innerHTML = ICON;
    this.label = document.createElement("span");
    this.label.textContent = TEXT.download;
    this.pill.append(this.label);
    this.menu = document.createElement("div");
    this.menu.className = "menu";
    this.menu.setAttribute("role", "menu");
    this.wrap.append(this.pill, this.menu);
    root.append(style, this.wrap);
    // Only the user's own clicks: a page must not start downloads with its synthetic events.
    this.pill.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.isTrusted) void this.toggleMenu();
    });
    for (const el of [this.pill, this.menu]) {
      el.addEventListener("pointerenter", () => {
        this.overUi = true;
        clearTimeout(this.hideTimer);
      });
      el.addEventListener("pointerleave", () => {
        this.overUi = false;
        this.scheduleHide();
      });
    }
    this.host = host;
    this.attach();
  }

  /** In fullscreen only the fullscreen element is drawn: the button must be inside it. */
  private attach() {
    if (!this.host) return;
    const fs = document.fullscreenElement;
    const parent = fs && fs.tagName !== "VIDEO" ? fs : document.documentElement;
    if (this.host.parentNode !== parent) parent.append(this.host);
  }

  private onMove = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    this.x = e.clientX;
    this.y = e.clientY;
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.hitTest();
    });
  };

  private onScroll = () => {
    if (this.current) requestAnimationFrame(() => this.place());
  };

  private hitTest() {
    if (this.menuOpen || this.overUi) return;
    let found: HTMLVideoElement | null = null;
    const order = this.current ? [this.current, ...this.visible] : [...this.visible];
    for (const v of order) {
      const t = v.isConnected ? targetOf(v) : null;
      if (!t) continue;
      const r = t.getBoundingClientRect();
      if (inside(r, this.x, this.y) && big(r)) {
        found = v;
        break;
      }
    }
    if (found) {
      clearTimeout(this.hideTimer);
      this.hideTimer = undefined;
      if (found !== this.current) this.show(found);
    } else if (this.current) {
      this.scheduleHide();
    }
  }

  private show(v: HTMLVideoElement) {
    if (document.fullscreenElement?.tagName === "VIDEO") return;
    this.ensureHost();
    this.attach();
    this.current = v;
    this.reset();
    this.place();
    this.wrap.classList.add("show");
    // Read the qualities while the pointer rests on the video, so the menu opens ready.
    clearTimeout(this.prefetchTimer);
    this.prefetchTimer = window.setTimeout(() => {
      if (this.current === v) void this.options(false);
    }, PREFETCH_AFTER);
  }

  private scheduleHide() {
    if (this.hideTimer || this.menuOpen) return;
    this.hideTimer = window.setTimeout(() => {
      this.hideTimer = undefined;
      if (!this.overUi && !this.menuOpen) this.hide();
    }, HIDE_AFTER);
  }

  private hide(force = false) {
    if (!this.host || (!force && (this.menuOpen || this.overUi))) return;
    clearTimeout(this.prefetchTimer);
    this.closeMenu();
    this.wrap.classList.remove("show");
    this.current = null;
  }

  private place() {
    const v = this.current;
    const t = v?.isConnected ? targetOf(v) : null;
    if (!t || !this.host) return this.hide(true);
    const r = t.getBoundingClientRect();
    const vw = document.documentElement.clientWidth || innerWidth;
    const vh = document.documentElement.clientHeight || innerHeight;
    // The button sits in the video's top right corner, its menu right-aligned under it.
    const width = 300;
    const right = Math.min(r.right, vw) - 10;
    const x = Math.min(Math.max(8, right - width), Math.max(8, vw - width - 8));
    const y = Math.max(r.top, 0) + 10;
    this.wrap.style.setProperty("--x", `${x}px`);
    this.wrap.style.setProperty("--y", `${y}px`);
    this.wrap.style.setProperty("width", `${Math.min(width, right - x)}px`);
    this.menu.classList.toggle("up", y + 400 > vh && y > 400);
  }

  private onFullscreen = () => {
    this.attach();
    if (document.fullscreenElement?.tagName === "VIDEO") this.hide(true);
    else if (this.current) this.place();
  };

  private onNavigate = () => {
    this.cache.clear();
    this.hide(true);
  };

  private onDown = (e: Event) => {
    if (this.menuOpen && this.host && !e.composedPath().includes(this.host)) {
      this.closeMenu();
      this.scheduleHide();
    }
  };

  private onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape" && this.menuOpen) {
      this.closeMenu();
      this.pill.focus();
    }
  };

  private onContextMenu = (e: MouseEvent) => {
    const own = e.composedPath().find((n): n is HTMLVideoElement => n instanceof HTMLVideoElement);
    this.contextVideo =
      own ??
      [...this.visible].find((v) => {
        const t = targetOf(v);
        return !!t && inside(t.getBoundingClientRect(), e.clientX, e.clientY);
      }) ??
      null;
  };

  /** "Download video with Oxyd" from the right-click menu. False when no video is there. */
  openFromContextMenu(): boolean {
    const v = this.contextVideo;
    this.contextVideo = null;
    if (!this.enabled || !v?.isConnected || !targetOf(v)) return false;
    this.hide(true);
    this.show(v);
    void this.toggleMenu();
    return true;
  }

  private context() {
    const v = this.current;
    const src = v?.currentSrc || v?.src || "";
    const pageUrl = v ? pageOf(v) : location.href;
    return {
      pageUrl,
      title: title(),
      src: /^https?:/.test(src) ? src : undefined,
      key: `${pageUrl}|${src}`,
    };
  }

  /** The menu's entries; `launch` starts Oxyd when it is not running (on a click). */
  private options(launch: boolean): Promise<VideoOptions | undefined> {
    const c = this.context();
    const hit = this.cache.get(c.key);
    if (hit && Date.now() - hit.at < CACHE_MS) {
      return hit.res.then((r) => (r?.offline && launch ? this.fetch(c, true) : r));
    }
    return this.fetch(c, launch);
  }

  private fetch(c: ReturnType<VideoButton["context"]>, launch: boolean) {
    const res = send<VideoOptions>({ type: "video-options", pageUrl: c.pageUrl, title: c.title, src: c.src, launch });
    this.cache.set(c.key, { at: Date.now(), res });
    // Failures and empty lists are asked again next time.
    void res.then((r) => {
      if (!r || !r.entries.length || r.offline) this.cache.delete(c.key);
    });
    return res;
  }

  private closeMenu() {
    if (!this.host) return;
    this.menuOpen = false;
    this.menu.classList.remove("open");
    this.pill.setAttribute("aria-expanded", "false");
  }

  private item(entry: MenuEntry, pageUrl: string, title: string): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = "item";
    b.type = "button";
    b.setAttribute("role", "menuitem");
    const l = document.createElement("span");
    l.className = "l";
    l.textContent = entry.label;
    const d = document.createElement("span");
    d.className = "d";
    d.textContent = entry.detail;
    b.append(l, d);
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      if (e.isTrusted) void this.choose(entry, pageUrl, title);
    });
    return b;
  }

  private line(cls: string, text: string): HTMLDivElement {
    const d = document.createElement("div");
    d.className = cls;
    d.textContent = text;
    return d;
  }

  private async toggleMenu() {
    if (this.menuOpen) return this.closeMenu();
    this.menuOpen = true;
    this.pill.setAttribute("aria-expanded", "true");
    this.menu.classList.add("open");
    this.place();
    const c = this.context();
    this.menu.replaceChildren(this.line("loading", TEXT.finding));
    // On a single video of a known site "Best quality" works before the list is in.
    if (isVideoSite(c.pageUrl) && postUrl(c.pageUrl, c.pageUrl)) {
      this.menu.append(
        this.item({ id: "best", group: "video", label: TEXT.best, detail: TEXT.bestHint, choice: "best" }, c.pageUrl, c.title),
      );
    }
    const res = await this.options(true);
    if (!this.menuOpen) return;
    this.render(res, c.title);
  }

  private render(res: VideoOptions | undefined, fallbackTitle: string) {
    const title = res?.title || fallbackTitle;
    const nodes: Node[] = [this.line("head", title)];
    if (res?.notice) nodes.push(this.line("note", res.notice));
    if (!res?.entries.length) nodes.push(this.line("empty", TEXT.none));
    const groups: [MenuEntry["group"], string][] = [
      ["video", TEXT.video],
      ["audio", TEXT.audio],
      ["stream", TEXT.streams],
    ];
    for (const [g, label] of groups) {
      const list = res?.entries.filter((e) => e.group === g) ?? [];
      if (!list.length) continue;
      nodes.push(this.line("group", label));
      for (const e of list) nodes.push(this.item(e, res!.pageUrl, title));
    }
    this.menu.replaceChildren(...nodes);
  }

  private async choose(entry: MenuEntry, pageUrl: string, title: string) {
    this.closeMenu();
    this.label.textContent = TEXT.sending;
    const r = await send<DownloadResult>({ type: "video-download", pageUrl, title, entry });
    this.pill.classList.add(r?.ok ? "done" : "err");
    this.label.textContent = r?.ok ? TEXT.sent : TEXT.failed;
    if (!r?.ok && r?.error) this.pill.title = r.error;
    setTimeout(() => this.reset(), 2500);
  }

  private reset() {
    if (!this.host) return;
    this.pill.classList.remove("done", "err");
    this.pill.title = TEXT.title;
    this.label.textContent = TEXT.download;
  }
}

declare global {
  interface Window {
    __oxydVideoButton?: VideoButton;
  }
}

if (!window.__oxydVideoButton) {
  const button = new VideoButton();
  window.__oxydVideoButton = button;
  const apply = (on: unknown) => button.setEnabled(on !== false);
  chrome.storage.sync
    .get({ videoButton: true })
    .then((s) => apply(s.videoButton))
    .catch(() => apply(true));
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "sync" && changes.videoButton) apply(changes.videoButton.newValue);
  });
  chrome.runtime.onMessage.addListener((msg: { type?: string }, _sender, respond) => {
    if (msg?.type !== "open-video-menu") return false;
    respond(button.openFromContextMenu());
    return false;
  });
}
