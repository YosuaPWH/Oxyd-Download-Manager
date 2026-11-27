import { api } from "./browser-api.ts";
import type { Candidate } from "./filters.ts";

export interface Attachment extends Candidate {
  finalUrl: string;
  incognito: boolean;
  replayable: boolean;
  cookies?: string;
  referrer?: string;
  userAgent?: string;
}

interface Navigation {
  url: string;
  replayable: boolean;
  cookies?: string;
  referrer?: string;
  userAgent?: string;
}

function header(headers: chrome.webRequest.HttpHeader[] | undefined, name: string): string | undefined {
  return headers?.find((item) => item.name.toLowerCase() === name)?.value;
}

export function attachmentName(disposition: string): string | undefined {
  const extended = /;\s*filename\*\s*=\s*UTF-8'[^']*'([^;]+)/i.exec(disposition);
  if (extended) {
    try {
      return decodeURIComponent(extended[1].trim());
    } catch {
      // Fall back to the plain filename when the encoded value is malformed.
    }
  }
  const plain = /;\s*filename\s*=\s*(?:"((?:[^"\\]|\\.)*)"|([^;]*))/i.exec(disposition);
  const name = plain?.[1]?.replace(/\\(["\\])/g, "$1") || plain?.[2]?.trim() || undefined;
  // Firefox can expose raw UTF-8 header bytes as Latin-1 code units. Reinterpret
  // them only if the complete name is valid UTF-8, so genuine Latin-1 stays intact.
  if (!name || !/^[\x00-\xff]+$/.test(name) || /^[\x00-\x7f]+$/.test(name)) return name;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(name, (c) => c.charCodeAt(0)));
  } catch {
    return name;
  }
}

/** Firefox can hold an attachment response while the desktop decides whether to accept it. */
export function watchFirefoxAttachments(accept: (item: Attachment) => Promise<boolean>) {
  const filter: chrome.webRequest.RequestFilter = {
    urls: ["http://*/*", "https://*/*"], types: ["main_frame", "sub_frame"],
  };
  const requests = new Map<string, Navigation>();
  api.webRequest.onBeforeSendHeaders.addListener((details): undefined => {
    if (details.tabId < 0) return;
    const headers = details.requestHeaders;
    const previous = requests.get(details.requestId);
    requests.set(details.requestId, {
      url: previous?.url ?? details.url,
      replayable: headers !== undefined && previous?.replayable !== false && details.method === "GET"
        && header(headers, "authorization") === undefined && header(headers, "range") === undefined,
      // Use only the cookies Firefox actually sent on this hop, including its container
      // and SameSite policy. Redirects overwrite the previous hop's header values.
      cookies: header(headers, "cookie"),
      referrer: header(headers, "referer"),
      userAgent: header(headers, "user-agent"),
    });
  }, filter, ["requestHeaders"]);

  const forget = ({ requestId }: { requestId: string }) => { requests.delete(requestId); };
  api.webRequest.onCompleted.addListener(forget, filter);
  api.webRequest.onErrorOccurred.addListener(forget, filter);

  // Chrome's type definitions omit the Promise return Firefox supports for blocking handlers.
  type Details = chrome.webRequest.OnHeadersReceivedDetails & { incognito?: boolean };
  type BlockingResponse = chrome.webRequest.BlockingResponse;
  const intercept = (details: Details): BlockingResponse | Promise<BlockingResponse> => {
    if (details.statusCode !== 200 && details.statusCode !== 206) return {};
    const request = requests.get(details.requestId);
    if (!request) return {};
    const disposition = header(details.responseHeaders, "content-disposition");
    if (!disposition || !/^\s*attachment(?:\s*;|\s*$)/i.test(disposition)) return {};
    const size = Number(header(details.responseHeaders, "content-length"));
    return accept({
      url: request.url,
      finalUrl: details.url,
      filename: attachmentName(disposition),
      mime: header(details.responseHeaders, "content-type")?.split(";")[0].trim(),
      totalBytes: Number.isSafeInteger(size) && size > 0 ? size : undefined,
      incognito: !!details.incognito,
      replayable: request.replayable && details.statusCode === 200 && details.method === "GET",
      cookies: request.cookies,
      referrer: request.referrer,
      userAgent: request.userAgent,
    }).then((accepted) => accepted ? { cancel: true } : {}, () => ({}));
  };
  api.webRequest.onHeadersReceived.addListener(intercept as (details: Details) => BlockingResponse,
    filter, ["blocking", "responseHeaders"]);
}
