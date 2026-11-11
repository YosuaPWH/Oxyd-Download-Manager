// Batch downloads: many links pasted at once, or one link with ranges in brackets.
//   https://x.org/photo-[001-120].jpg   numbers, zero padded like the first bound
//   https://x.org/part-[1-20:5].zip     with a step
//   https://x.org/disk-[a-c].iso        letters

/** Most links one batch may hold: a typo like [1-9999999] must not freeze the window. */
export const BATCH_LIMIT = 1000;

export interface Batch {
  urls: string[];
  /** Words that looked like text, not links. */
  invalid: number;
  /** Links that appeared more than once. */
  duplicates: number;
  /** The patterns describe more than `BATCH_LIMIT` links; the rest was dropped. */
  truncated: boolean;
}

const RANGE = /\[(\d+)-(\d+)(?::(\d+))?\]|\[([a-z])-([a-z])\]|\[([A-Z])-([A-Z])\]/;

function rangeValues(m: RegExpMatchArray, limit: number): string[] {
  const out: string[] = [];
  if (m[1] !== undefined) {
    const from = parseInt(m[1], 10);
    const to = parseInt(m[2], 10);
    const step = Math.max(1, parseInt(m[3] ?? "1", 10));
    // "001" asks for three digits; "1-100" asks for none.
    const width = m[1].length > 1 && m[1].startsWith("0") ? m[1].length : 0;
    const dir = from <= to ? 1 : -1;
    for (let v = from; dir > 0 ? v <= to : v >= to; v += step * dir) {
      out.push(String(v).padStart(width, "0"));
      if (out.length > limit) break;
    }
    return out;
  }
  const a = (m[4] ?? m[6]).charCodeAt(0);
  const b = (m[5] ?? m[7]).charCodeAt(0);
  const dir = a <= b ? 1 : -1;
  for (let c = a; dir > 0 ? c <= b : c >= b; c += dir) out.push(String.fromCharCode(c));
  return out;
}

/** Every link a pattern describes, left range varying slowest. At most `limit + 1` results. */
export function expandPattern(pattern: string, limit = BATCH_LIMIT): string[] {
  let done: string[] = [""];
  let rest = pattern;
  for (;;) {
    const m = rest.match(RANGE);
    if (!m || m.index === undefined) break;
    const head = rest.slice(0, m.index);
    const values = rangeValues(m, limit);
    const next: string[] = [];
    outer: for (const prefix of done) {
      for (const v of values) {
        next.push(prefix + head + v);
        if (next.length > limit) break outer;
      }
    }
    done = next;
    rest = rest.slice(m.index + m[0].length);
  }
  return done.map((p) => p + rest);
}

function isLink(text: string): boolean {
  try {
    const u = new URL(text);
    return (u.protocol === "http:" || u.protocol === "https:") && !!u.host;
  } catch {
    return false;
  }
}

/** Links in pasted or dropped text: one per line or separated by spaces, patterns expanded. */
export function parseBatch(text: string, limit = BATCH_LIMIT): Batch {
  const seen = new Set<string>();
  const batch: Batch = { urls: [], invalid: 0, duplicates: 0, truncated: false };
  for (const word of text.split(/\s+/)) {
    if (!word) continue;
    for (const url of expandPattern(word, limit)) {
      if (!isLink(url)) {
        batch.invalid++;
        break;
      }
      if (seen.has(url)) {
        batch.duplicates++;
        continue;
      }
      if (batch.urls.length >= limit) {
        batch.truncated = true;
        return batch;
      }
      seen.add(url);
      batch.urls.push(url);
    }
  }
  return batch;
}

/** Last path segment of a link, decoded, for the preview list. */
export function linkName(url: string): string {
  try {
    const u = new URL(url);
    const last = u.pathname.split("/").filter(Boolean).pop();
    return last ? decodeURIComponent(last) : u.host;
  } catch {
    return url;
  }
}
