import { api, firefox } from "./browser-api.ts";
export const HOST = "com.oxyd.bridge";
export const TIMEOUT_MS = 3000;

export interface AddDownload {
  v: 1;
  type: "add_download";
  id: string;
  url: string;
  filename?: string;
  size?: number;
  mime?: string;
  referrer?: string;
  userAgent?: string;
  cookies?: string;
  source?: string;
}

export interface VideoChoice {
  id: string;
  kind: "video" | "audio";
  label: string;
  detail: string;
  size?: number;
}

export interface VideoInfo {
  v: 1;
  type: "video_info";
  id: string;
  title: string;
  duration?: number;
  site?: string;
  live: boolean;
  choices: VideoChoice[];
}

export type Reply =
  | {
      v: 1;
      type: "ack";
      id: string;
      status: "accepted" | "rejected" | "error";
      message?: string;
      /** Why it failed: `video`, `tool`, `network`, ... */
      code?: string;
    }
  | { v: 1; type: "pong"; appVersion: string; captureSettings?: import("./settings.ts").ExtSettings }
  | VideoInfo;

/** Reading a page with yt-dlp (downloading yt-dlp first, the first time) takes a while. */
export const VIDEO_TIMEOUT_MS = 120_000;

/** Send one message to the native host, failing after `timeout` ms. */
export function sendNative(msg: object, timeout = TIMEOUT_MS): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Oxyd did not answer in time")), timeout);
    const succeed = (reply?: Reply) => {
      clearTimeout(timer);
      if (!reply) reject(new Error("empty reply"));
      else resolve(reply);
    };
    const fail = (e: unknown) => {
      clearTimeout(timer);
      reject(e);
    };
    if (firefox) {
      void Promise.resolve(firefox.runtime.sendNativeMessage(HOST, msg)).then(succeed, fail);
    } else {
      api.runtime.sendNativeMessage(HOST, msg, (reply?: Reply) => {
        const err = api.runtime.lastError;
        if (err) fail(new Error(err.message));
        else succeed(reply);
      });
    }
  });
}

export async function ping(): Promise<string | null> {
  try {
    const r = await sendNative({ v: 1, type: "ping" }, 2000);
    return r.type === "pong" ? r.appVersion : null;
  } catch {
    return null;
  }
}

/** Cookies for `url` as a `Cookie` header value (HttpOnly cookies included). */
export async function cookieHeader(url: string, storeId?: string | false): Promise<string | undefined> {
  if (storeId === false) return undefined;
  try {
    const cookies = await api.cookies.getAll(storeId ? { url, storeId } : { url });
    const header = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
    return header || undefined;
  } catch {
    return undefined;
  }
}

export function browserName(): string {
  if (firefox) return "firefox";
  const brands = (navigator as Navigator & { userAgentData?: { brands: { brand: string }[] } })
    .userAgentData?.brands;
  if (brands?.some((b) => /edge/i.test(b.brand))) return "edge";
  return /Edg\//.test(navigator.userAgent) ? "edge" : "chrome";
}
