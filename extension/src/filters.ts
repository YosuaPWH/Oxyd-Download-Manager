import type { ExtSettings } from "./settings.ts";

/** The parts of `chrome.downloads.DownloadItem` the filter looks at. */
export interface Candidate {
  url: string;
  finalUrl?: string;
  filename?: string;
  mime?: string;
  totalBytes?: number;
  fileSize?: number;
  byExtensionId?: string;
}

export type Decision = { capture: true } | { capture: false; reason: string };

export function extensionOf(name: string): string {
  const base = name.split(/[\\/]/).pop() ?? "";
  const clean = base.split(/[?#]/)[0];
  const dot = clean.lastIndexOf(".");
  return dot > 0 ? clean.slice(dot + 1).toLowerCase() : "";
}

function urlFileName(url: string): string {
  try {
    const path = new URL(url).pathname;
    return decodeURIComponent(path.split("/").filter(Boolean).pop() ?? "");
  } catch {
    return "";
  }
}

export function hostMatches(url: string, domains: string[]): boolean {
  let host: string;
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return false;
  }
  return domains.some((d) => {
    const dom = d.toLowerCase().replace(/^\*?\./, "");
    return dom !== "" && (host === dom || host.endsWith(`.${dom}`));
  });
}

/** Decide whether a browser download should be handed to Oxyd. */
export function shouldCapture(
  item: Candidate,
  s: ExtSettings,
  ctx: { selfId: string; bypass: Set<string>; skipNext: boolean },
): Decision {
  const url = item.finalUrl || item.url;
  if (!s.enabled) return { capture: false, reason: "disabled" };
  if (ctx.skipNext) return { capture: false, reason: "skip-next" };
  if (item.byExtensionId && item.byExtensionId === ctx.selfId) {
    return { capture: false, reason: "own-download" };
  }
  if (ctx.bypass.has(item.url) || ctx.bypass.has(url)) return { capture: false, reason: "bypass" };
  if (!/^https?:/i.test(url)) return { capture: false, reason: "scheme" };
  if (hostMatches(url, s.excludedDomains) || hostMatches(item.url, s.excludedDomains)) {
    return { capture: false, reason: "excluded-domain" };
  }
  const size = item.totalBytes && item.totalBytes > 0 ? item.totalBytes : item.fileSize;
  if (size && size > 0 && size < s.minSize) return { capture: false, reason: "too-small" };
  if (s.extensions.length) {
    const ext = extensionOf(item.filename || "") || extensionOf(urlFileName(url));
    if (!s.extensions.includes(ext)) return { capture: false, reason: "extension" };
  }
  return { capture: true };
}
