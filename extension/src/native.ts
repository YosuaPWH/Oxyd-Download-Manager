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

export type Reply =
  | { v: 1; type: "ack"; id: string; status: "accepted" | "rejected" | "error"; message?: string }
  | { v: 1; type: "pong"; appVersion: string };

/** Send one message to the native host, failing after `timeout` ms. */
export function sendNative(msg: object, timeout = TIMEOUT_MS): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Oxyd did not answer in time")), timeout);
    chrome.runtime.sendNativeMessage(HOST, msg, (reply?: Reply) => {
      clearTimeout(timer);
      const err = chrome.runtime.lastError;
      if (err) reject(new Error(err.message));
      else if (!reply) reject(new Error("empty reply"));
      else resolve(reply);
    });
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
export async function cookieHeader(url: string, storeId?: string): Promise<string | undefined> {
  try {
    const cookies = await chrome.cookies.getAll(storeId ? { url, storeId } : { url });
    const header = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
    return header || undefined;
  } catch {
    return undefined;
  }
}

export function browserName(): string {
  const brands = (navigator as Navigator & { userAgentData?: { brands: { brand: string }[] } })
    .userAgentData?.brands;
  if (brands?.some((b) => /edge/i.test(b.brand))) return "edge";
  return /Edg\//.test(navigator.userAgent) ? "edge" : "chrome";
}
